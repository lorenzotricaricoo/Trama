//! A recording in progress: one supervisor and one writer thread per track.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::backend::{Fault, FaultReporter, SinkStats, frame_channel};
use super::wav::{WavInfo, repair_wav};
use super::writer::{self, WriterConfig, WriterMsg, WriterOutcome};
use super::{
    ActiveStream, CaptureBackend, CaptureError, CaptureEvent, CaptureObserver, DeviceInfo,
    PermissionState, Result, Source, StreamFormat, error_code, track_file_names,
};
use crate::storage::{Id, Library, NewTrack, Track};

/// Timings of the capture threads. The defaults suit real devices; tests shorten them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tuning {
    /// How often the supervisor checks whether the default device changed.
    pub device_poll: Duration,
    /// A stream that delivered audio and then nothing for this long is reopened.
    pub stall_timeout: Duration,
    /// How often to retry opening a lost stream.
    pub retry_every: Duration,
    /// How often the writer drains the ring buffer: what a crash can lose.
    pub writer_pass: Duration,
    /// How often the WAV header is patched and the files synced to disk.
    pub checkpoint_every: Duration,
    /// Digital silence for this long since the start raises a warning.
    pub silent_after: Duration,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            device_poll: Duration::from_millis(500),
            stall_timeout: Duration::from_secs(3),
            retry_every: Duration::from_secs(1),
            writer_pass: Duration::from_millis(20),
            checkpoint_every: Duration::from_secs(1),
            silent_after: Duration::from_secs(10),
        }
    }
}

/// What to record and where.
#[derive(Debug, Clone)]
pub struct RecordingOptions {
    /// Root folder of the library ([`Library::root`]).
    pub library_root: PathBuf,
    /// The meeting the audio belongs to; files go to [`Library::meeting_audio_dir`].
    pub meeting_id: Id,
    /// Record the `me` track.
    pub microphone: bool,
    /// Record the `others` track.
    pub system_audio: bool,
    /// Microphone to use, by [`DeviceInfo::id`]; `None` follows the system default,
    /// also when it changes during the recording.
    pub microphone_device: Option<String>,
    /// Channels of each track. Mono by default: the tracks are for speech, and a mono
    /// 48 kHz track stays under the 4 GiB WAV limit for six hours.
    pub channels: u16,
    pub tuning: Tuning,
    /// Largest data size of a track; the WAV limit unless a test lowers it.
    pub max_track_bytes: u64,
}

impl RecordingOptions {
    /// Both tracks, default devices, mono.
    pub fn new(library_root: impl Into<PathBuf>, meeting_id: Id) -> Self {
        Self {
            library_root: library_root.into(),
            meeting_id,
            microphone: true,
            system_audio: true,
            microphone_device: None,
            channels: 1,
            tuning: Tuning::default(),
            max_track_bytes: u64::from(u32::MAX),
        }
    }
}

/// How one track went.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackSummary {
    pub source: Source,
    /// Relative to the library root.
    pub audio_path: PathBuf,
    /// Relative to the library root.
    pub timing_path: PathBuf,
    /// The first device captured.
    pub device: Option<String>,
    pub format: Option<StreamFormat>,
    /// Frames in the WAV, silence included.
    pub frames: u64,
    pub duration_ms: u64,
    /// When the first audio arrived, after the start of the recording.
    pub first_audio_ms: Option<u64>,
    /// Frames of silence written to fill gaps.
    pub padded_frames: u64,
    /// Frames lost in buffer overflows (also counted as padding).
    pub dropped_frames: u64,
    /// Streams opened: more than one after device changes or failures.
    pub segments: u32,
    /// Times the stream was lost.
    pub interruptions: u32,
    /// Codes of the problems reported during the recording ([`error_code`]).
    pub problems: Vec<&'static str>,
}

/// How a recording went.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordingSummary {
    /// From start to stop on the host clock.
    pub duration_ms: u64,
    pub tracks: Vec<TrackSummary>,
}

struct TrackRuntime {
    source: Source,
    device: String,
    audio_path: PathBuf,
    timing_path: PathBuf,
    supervisor: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<WriterOutcome>>,
    writer_tx: Sender<WriterMsg>,
}

/// A recording in progress. Stop it with [`stop`](Self::stop); dropping it also stops
/// it and finishes the files.
pub struct Recording {
    backend: Arc<dyn CaptureBackend>,
    meeting_id: Id,
    tracks: Vec<TrackRuntime>,
    stop: Arc<AtomicBool>,
    start_ns: u64,
    stopped: bool,
}

impl std::fmt::Debug for Recording {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recording")
            .field("meeting_id", &self.meeting_id)
            .field("start_ns", &self.start_ns)
            .finish_non_exhaustive()
    }
}

/// Checks microphone access, asking the user if they have not been asked yet.
fn ensure_microphone(backend: &dyn CaptureBackend) -> Result<()> {
    let state = match backend.microphone_permission() {
        PermissionState::NotDetermined => backend.request_microphone_permission(),
        other => other,
    };
    match state {
        PermissionState::Granted | PermissionState::Unknown => Ok(()),
        PermissionState::Restricted => Err(CaptureError::new(
            error_code::MICROPHONE_RESTRICTED,
            "microphone access is restricted",
        )),
        PermissionState::Denied | PermissionState::NotDetermined => Err(CaptureError::new(
            error_code::MICROPHONE_DENIED,
            "microphone access was not granted",
        )),
    }
}

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

impl Recording {
    /// Opens the streams and starts writing. Returns once both streams are open, or
    /// with the first error (and nothing left behind). Asks for microphone access if
    /// the user has not been asked yet, which blocks until they answer.
    pub fn start(
        backend: Arc<dyn CaptureBackend>,
        options: RecordingOptions,
        observer: Arc<dyn CaptureObserver>,
    ) -> Result<Self> {
        let sources: Vec<Source> = Source::ALL
            .into_iter()
            .filter(|s| match s {
                Source::Microphone => options.microphone,
                Source::System => options.system_audio,
            })
            .collect();
        if sources.is_empty() {
            return Err(CaptureError::new(error_code::NOTHING_TO_RECORD, ""));
        }
        if options.microphone {
            ensure_microphone(backend.as_ref())?;
        }

        let rel_dir = Library::meeting_audio_dir(options.meeting_id);
        let dir = options.library_root.join(&rel_dir);
        std::fs::create_dir_all(&dir).map_err(|e| CaptureError::io(error_code::WRITE_FAILED, e))?;
        for source in &sources {
            let (wav, timing) = track_file_names(*source);
            for name in [wav, timing] {
                if dir.join(&name).exists() {
                    return Err(CaptureError::new(
                        error_code::ALREADY_RECORDED,
                        rel_dir.join(name).display(),
                    ));
                }
            }
        }

        let mut recording = Recording {
            start_ns: backend.now_ns(),
            backend,
            meeting_id: options.meeting_id,
            tracks: Vec::new(),
            stop: Arc::new(AtomicBool::new(false)),
            stopped: false,
        };
        let unix_ms = unix_ms();
        for source in sources {
            if let Err(err) = recording.start_track(source, &options, &observer, &dir, unix_ms) {
                let _ = recording.finish();
                for track in &recording.tracks {
                    let _ = std::fs::remove_file(options.library_root.join(&track.audio_path));
                    let _ = std::fs::remove_file(options.library_root.join(&track.timing_path));
                }
                // Only removed if empty.
                let _ = std::fs::remove_dir(&dir);
                return Err(err);
            }
        }
        Ok(recording)
    }

    fn start_track(
        &mut self,
        source: Source,
        options: &RecordingOptions,
        observer: &Arc<dyn CaptureObserver>,
        dir: &Path,
        unix_ms: i64,
    ) -> Result<()> {
        let (wav_name, timing_name) = track_file_names(source);
        let rel_dir = Library::meeting_audio_dir(options.meeting_id);
        let tuning = options.tuning;
        let (writer_tx, writer_rx) = mpsc::channel();
        let cfg = WriterConfig {
            source,
            wav_path: dir.join(&wav_name),
            timing_path: dir.join(&timing_name),
            max_channels: options.channels.max(1),
            recording_start_ns: self.start_ns,
            unix_ms,
            observer: observer.clone(),
            pass: tuning.writer_pass,
            checkpoint_every: tuning.checkpoint_every,
            silent_after: tuning.silent_after,
            max_data_bytes: options.max_track_bytes,
        };
        let spawn_err = |e: std::io::Error| CaptureError::new(error_code::INTERNAL, e);
        let writer = std::thread::Builder::new()
            .name(format!("capture-writer-{}", source.as_str()))
            .spawn(move || writer::run(cfg, writer_rx))
            .map_err(spawn_err)?;

        let (faults_tx, faults_rx) = mpsc::channel();
        let supervisor = Supervisor {
            source,
            backend: self.backend.clone(),
            pinned: match source {
                Source::Microphone => options.microphone_device.clone(),
                Source::System => None,
            },
            writer_tx: writer_tx.clone(),
            observer: observer.clone(),
            stop: self.stop.clone(),
            tuning,
            faults_tx,
            faults_rx,
            generation: 0,
        };
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let handle = std::thread::Builder::new()
            .name(format!("capture-{}", source.as_str()))
            .spawn(move || supervisor.run(ready_tx));
        let mut track = TrackRuntime {
            source,
            device: String::new(),
            audio_path: rel_dir.join(wav_name),
            timing_path: rel_dir.join(timing_name),
            supervisor: None,
            writer: Some(writer),
            writer_tx,
        };
        let result = match handle {
            Ok(handle) => {
                track.supervisor = Some(handle);
                ready_rx.recv().unwrap_or_else(|_| {
                    Err(CaptureError::new(error_code::INTERNAL, "supervisor died"))
                })
            }
            Err(e) => Err(spawn_err(e)),
        };
        if let Ok(device) = &result {
            track.device.clone_from(device);
        }
        self.tracks.push(track);
        result.map(|_| ())
    }

    /// Host time the recording started at (see [`CaptureBackend::now_ns`]).
    pub fn start_ns(&self) -> u64 {
        self.start_ns
    }

    pub fn meeting_id(&self) -> Id {
        self.meeting_id
    }

    /// The tracks being recorded, ready for [`Library::add_track`].
    pub fn new_tracks(&self) -> Vec<NewTrack> {
        self.tracks
            .iter()
            .map(|t| NewTrack {
                role: t.source.role(),
                audio_path: t.audio_path.clone(),
                timing_path: Some(t.timing_path.clone()),
                device: Some(t.device.clone()).filter(|d| !d.is_empty()),
            })
            .collect()
    }

    /// Registers the tracks in the library. Call it right after [`start`](Self::start),
    /// so that the files are known even if the app stops during the recording.
    pub fn register_tracks(&self, library: &mut Library) -> crate::storage::Result<Vec<Track>> {
        self.new_tracks()
            .iter()
            .map(|t| library.add_track(self.meeting_id, t))
            .collect()
    }

    /// Stops the streams, finishes the files and reports how the tracks went.
    pub fn stop(mut self) -> Result<RecordingSummary> {
        self.finish()
    }

    fn finish(&mut self) -> Result<RecordingSummary> {
        self.stopped = true;
        self.stop.store(true, Ordering::Release);
        let mut panicked = false;
        for track in &mut self.tracks {
            if let Some(handle) = track.supervisor.take() {
                panicked |= handle.join().is_err();
            }
        }
        let stop_ns = self.backend.now_ns().max(self.start_ns);
        let mut summaries = Vec::new();
        for track in &mut self.tracks {
            let _ = track.writer_tx.send(WriterMsg::Finish { stop_ns });
            let Some(handle) = track.writer.take() else {
                continue;
            };
            let Ok(outcome) = handle.join() else {
                panicked = true;
                continue;
            };
            let duration_ms = outcome
                .format
                .map_or(0, |f| outcome.frames * 1000 / u64::from(f.sample_rate));
            summaries.push(TrackSummary {
                source: track.source,
                audio_path: track.audio_path.clone(),
                timing_path: track.timing_path.clone(),
                device: outcome.device,
                format: outcome.format,
                frames: outcome.frames,
                duration_ms,
                first_audio_ms: outcome.first_audio_ms,
                padded_frames: outcome.padded_frames,
                dropped_frames: outcome.dropped_frames,
                segments: outcome.segments,
                interruptions: outcome.interruptions,
                problems: outcome.problems,
            });
        }
        if panicked {
            return Err(CaptureError::new(
                error_code::INTERNAL,
                "a capture thread panicked",
            ));
        }
        Ok(RecordingSummary {
            duration_ms: (stop_ns - self.start_ns) / 1_000_000,
            tracks: summaries,
        })
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        if !self.stopped {
            let _ = self.finish();
        }
    }
}

/// Repairs the WAV headers of a meeting whose recording was interrupted (the app
/// stopped or crashed), so the audio up to the interruption is readable. Returns the
/// repaired files, relative to the library root, with what they contain.
pub fn repair_meeting_audio(
    library_root: &Path,
    meeting_id: Id,
) -> Result<Vec<(PathBuf, WavInfo)>> {
    let rel_dir = Library::meeting_audio_dir(meeting_id);
    let mut repaired = Vec::new();
    for source in Source::ALL {
        let rel = rel_dir.join(track_file_names(source).0);
        let path = library_root.join(&rel);
        if path.exists() {
            repaired.push((rel, repair_wav(&path)?));
        }
    }
    Ok(repaired)
}

struct ActiveTrack {
    _stream: Box<dyn ActiveStream>,
    device: DeviceInfo,
    stats: Arc<SinkStats>,
    seen_callbacks: u64,
    /// Host time (backend clock) the supervisor last saw a new buffer at.
    last_progress_ns: u64,
}

/// Keeps one stream open for the whole recording: reopens it when the default device
/// changes, when it fails, and when it stalls.
struct Supervisor {
    source: Source,
    backend: Arc<dyn CaptureBackend>,
    pinned: Option<String>,
    writer_tx: Sender<WriterMsg>,
    observer: Arc<dyn CaptureObserver>,
    stop: Arc<AtomicBool>,
    tuning: Tuning,
    faults_tx: Sender<Fault>,
    faults_rx: Receiver<Fault>,
    generation: u64,
}

impl Supervisor {
    fn open(&mut self) -> Result<ActiveTrack> {
        self.generation += 1;
        let (sink, frames, stats) = frame_channel();
        let faults = FaultReporter::new(self.generation, self.faults_tx.clone());
        let stream = self
            .backend
            .open(self.source, self.pinned.as_deref(), sink, faults)?;
        let format = stream.format();
        let device = stream.device();
        let backend = stream.backend();
        let _ = self.writer_tx.send(WriterMsg::Segment {
            frames,
            format,
            device: device.clone(),
            backend,
        });
        self.observer.event(CaptureEvent::DeviceOpened {
            source: self.source,
            device: device.clone(),
            format,
            backend,
        });
        Ok(ActiveTrack {
            _stream: stream,
            device,
            stats,
            seen_callbacks: 0,
            last_progress_ns: self.backend.now_ns(),
        })
    }

    /// Closes a lost stream and tells the writer and the observer.
    fn interrupt(&mut self, active: &mut Option<ActiveTrack>, code: &'static str, detail: String) {
        if active.take().is_none() {
            return;
        }
        let _ = self.writer_tx.send(WriterMsg::Interrupted {
            host_ns: self.backend.now_ns(),
            code,
        });
        self.observer.event(CaptureEvent::TrackInterrupted {
            source: self.source,
            code,
            detail,
        });
    }

    fn run(mut self, ready: SyncSender<Result<String>>) {
        let mut active = match self.open() {
            Ok(track) => {
                let _ = ready.send(Ok(track.device.name.clone()));
                Some(track)
            }
            Err(err) => {
                let _ = ready.send(Err(err));
                return;
            }
        };
        drop(ready);
        let step = self.tuning.device_poll.min(Duration::from_millis(100));
        let mut last_poll = Instant::now();
        let mut last_attempt = Instant::now();
        let mut retry_now = false;
        let mut failure_reported = false;
        loop {
            // Disconnected cannot happen: `self.faults_tx` keeps the channel open.
            let fault = self.faults_rx.recv_timeout(step).ok();
            if self.stop.load(Ordering::Acquire) {
                break;
            }
            if let Some(fault) = fault
                && fault.generation == self.generation
                && active.is_some()
            {
                self.interrupt(&mut active, fault.code, fault.detail);
                retry_now = true;
            }

            // Measured on the backend's clock, the one the audio runs on.
            let now_ns = self.backend.now_ns();
            let stall_ns = self.tuning.stall_timeout.as_nanos() as u64;
            let stalled = active.as_mut().is_some_and(|track| {
                let seen = track.stats.callbacks.load(Ordering::Relaxed);
                if seen != track.seen_callbacks {
                    track.seen_callbacks = seen;
                    track.last_progress_ns = now_ns;
                    false
                } else {
                    seen > 0 && now_ns.saturating_sub(track.last_progress_ns) >= stall_ns
                }
            });
            if stalled {
                let detail = format!("no audio for {:?}", self.tuning.stall_timeout);
                self.interrupt(&mut active, error_code::STREAM_STALLED, detail);
                retry_now = true;
            }

            if self.pinned.is_none() && last_poll.elapsed() >= self.tuning.device_poll {
                last_poll = Instant::now();
                if let Some(track) = &active
                    && let Ok(Some(default)) = self.backend.default_device(self.source)
                    && default.id != track.device.id
                {
                    active = None;
                    self.observer.event(CaptureEvent::DeviceChanged {
                        source: self.source,
                        device: default,
                    });
                    retry_now = true;
                }
            }

            if active.is_none() && (retry_now || last_attempt.elapsed() >= self.tuning.retry_every)
            {
                retry_now = false;
                last_attempt = Instant::now();
                match self.open() {
                    Ok(track) => {
                        active = Some(track);
                        failure_reported = false;
                    }
                    Err(err) => {
                        if !failure_reported {
                            failure_reported = true;
                            self.observer.event(CaptureEvent::TrackInterrupted {
                                source: self.source,
                                code: err.code,
                                detail: err.detail,
                            });
                        }
                    }
                }
            }
        }
        drop(active);
    }
}
