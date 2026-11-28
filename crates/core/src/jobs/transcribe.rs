//! Transcribes a meeting's tracks and stores the result as a new version.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{JobContext, JobError, JobHandler};
use crate::storage::{Id, Job, JobKind, JobState, MeetingStatus, StorageError};
use crate::transcription::{
    self, EngineLoader, MemoryMonitor, TrackAudio, TranscriptionError, error_code,
    is_supported_language,
};

/// How often memory is sampled during a transcription.
const MEMORY_SAMPLING: Duration = Duration::from_millis(200);

/// Handler for [`JobKind::Transcribe`].
///
/// The meeting goes to `processing` while it runs and to `ready` when the transcript is
/// stored. A failure that retrying won't fix (no model, missing audio), or the last
/// failed attempt, marks it `failed` with the error code; a cancelled transcription marks
/// it `failed` with [`error_code::CANCELLED`]. When the app stops, the job goes back in
/// the queue and the meeting stays `processing` until it runs again.
///
/// The transcript and the `ready` status are written in one transaction
/// ([`crate::storage::Library::complete_transcription`]), so a retry never stores a
/// second version. If the meeting goes to the trash while the job runs, the job fails
/// with [`error_code::MEETING_DELETED`] and writes nothing to the meeting, not even the
/// failure; restoring the meeting marks it `failed` with that code
/// ([`crate::storage::Library::restore`]).
///
/// The model is loaded for each transcription and dropped afterwards, so its memory
/// (about 2.8 GB at peak) is held only while it runs; the peak is measured and logged.
pub struct Transcribe {
    loader: Arc<dyn EngineLoader>,
}

impl std::fmt::Debug for Transcribe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transcribe").finish_non_exhaustive()
    }
}

/// What a finished transcription measured.
#[derive(Debug, Clone, PartialEq)]
struct TranscriptionReport {
    segments: usize,
    /// Length of the longest track.
    audio: Duration,
    elapsed: Duration,
    /// Highest resident memory of the process during the transcription.
    peak_memory_bytes: Option<u64>,
}

impl Transcribe {
    pub fn new(loader: impl EngineLoader + 'static) -> Self {
        Self {
            loader: Arc::new(loader),
        }
    }

    fn transcribe(
        &self,
        job: &Job,
        ctx: &JobContext,
    ) -> Result<TranscriptionReport, TranscriptionError> {
        let started = Instant::now();
        let id = job.target_id;
        let storage = |err: StorageError| match err {
            // The meeting went to the trash: retrying cannot help, and nothing is written.
            StorageError::NotFound { .. } => {
                TranscriptionError::permanent(error_code::MEETING_DELETED, err)
            }
            err => TranscriptionError::retryable(super::error_code::STORAGE, err),
        };
        let (tracks, audio) = {
            let mut library = ctx.library();
            let source = library.source(id).map_err(storage)?;
            let meeting = library.meeting(id).map_err(storage)?;
            if meeting.status == MeetingStatus::Recording {
                return Err(TranscriptionError::retryable(
                    error_code::STILL_RECORDING,
                    id,
                ));
            }
            if meeting.audio_deleted_at.is_some() {
                return Err(TranscriptionError::permanent(
                    error_code::AUDIO_MISSING,
                    "the meeting's audio was deleted",
                ));
            }
            if let Some(language) = &source.language
                && !is_supported_language(language)
            {
                return Err(TranscriptionError::permanent(
                    error_code::LANGUAGE_UNSUPPORTED,
                    language,
                ));
            }
            let root = library.root().to_path_buf();
            let tracks: Vec<TrackAudio> = library
                .tracks(id)
                .map_err(storage)?
                .into_iter()
                .map(|track| TrackAudio {
                    role: track.role,
                    audio_path: root.join(track.audio_path),
                    timing_path: track.timing_path.map(|path| root.join(path)),
                })
                .collect();
            if tracks.is_empty() {
                return Err(TranscriptionError::permanent(
                    error_code::NO_AUDIO,
                    "the meeting has no tracks",
                ));
            }
            library
                .set_meeting_status(id, MeetingStatus::Processing, None)
                .map_err(storage)?;
            let audio = tracks
                .iter()
                .filter_map(|t| transcription::WavSource::open(&t.audio_path).ok())
                .map(|source| source.duration_s())
                .fold(0.0, f64::max);
            (tracks, Duration::from_secs_f64(audio))
        };

        let memory = MemoryMonitor::start(MEMORY_SAMPLING);
        let segments = {
            let mut engine = self.loader.load()?;
            let segments = transcription::transcribe_tracks(
                engine.as_mut(),
                &tracks,
                &mut |fraction| ctx.progress(fraction * 0.99),
                &|| ctx.should_stop(),
            )?;
            (engine.description(), segments)
        };
        let peak_memory_bytes = memory.finish();
        let (engine, segments) = segments;

        // One transaction, which checks first that the meeting is not in the trash.
        ctx.library()
            .complete_transcription(id, &engine, &segments)
            .map_err(storage)?;
        // The transcript is stored: failing now would make a retry transcribe and store
        // it again, so a summary that cannot be queued is logged instead (the user can
        // still ask for it, `summary::regenerate`).
        if let Err(err) = crate::summary::enqueue_after_transcription(ctx, id) {
            eprintln!("trama: cannot queue the summary of meeting {id}: {err}");
        }
        ctx.progress(1.0);
        Ok(TranscriptionReport {
            segments: segments.len(),
            audio,
            elapsed: started.elapsed(),
            peak_memory_bytes,
        })
    }

    /// Records a failure on the meeting when it is final.
    fn record_failure(
        &self,
        job: &Job,
        ctx: &JobContext,
        err: &TranscriptionError,
    ) -> Result<(), JobError> {
        let id = job.target_id;
        let code = if err.code == error_code::STOPPED {
            // Stopped by the user, or by the app shutting down (then it runs again).
            if ctx.library().job(job.id)?.state != JobState::Cancelled {
                return Ok(());
            }
            error_code::CANCELLED
        } else if err.code == error_code::STILL_RECORDING
            || (err.retryable && job.attempts < job.max_attempts)
        {
            return Ok(());
        } else {
            err.code
        };
        match ctx.library().set_meeting_failed(id, code) {
            // In the trash: nothing to write; restoring it settles the status.
            Ok(_) | Err(StorageError::NotFound { .. }) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }
}

impl JobHandler for Transcribe {
    fn kind(&self) -> JobKind {
        JobKind::Transcribe
    }

    fn run(&self, job: &Job, ctx: &JobContext) -> Result<(), JobError> {
        match self.transcribe(job, ctx) {
            Ok(report) => {
                log_report(job.target_id, &report);
                Ok(())
            }
            Err(err) => {
                self.record_failure(job, ctx, &err)?;
                Err(if err.code == error_code::STOPPED {
                    JobError::stopped()
                } else {
                    JobError {
                        code: err.code.to_owned(),
                        detail: err.detail,
                        retryable: err.retryable,
                    }
                })
            }
        }
    }
}

/// Diagnostics on stderr, like the rest of the core: length, speed and peak memory.
fn log_report(meeting: Id, report: &TranscriptionReport) {
    let elapsed = report.elapsed.as_secs_f64();
    let speed = report.audio.as_secs_f64() / elapsed.max(1e-3);
    let memory = report
        .peak_memory_bytes
        .map_or("unknown".to_owned(), |bytes| {
            format!("{:.2} GB", bytes as f64 / 1e9)
        });
    eprintln!(
        "trama: transcribed meeting {meeting}: {} segments, {:.0} s of audio in {elapsed:.0} s ({speed:.1}x real time), peak memory {memory}",
        report.segments,
        report.audio.as_secs_f64(),
    );
}
