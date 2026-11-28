//! Local transcription: a meeting's tracks turned into one timeline of text.
//!
//! Spike #10 and ADR 0007 chose Parakeet Ultra (fp32) run by sherpa-onnx, with the Silero
//! voice activity detector cutting the audio into stretches of speech: 1.0 s of silence
//! closes a stretch, a stretch lasts at most 30 s, and 500 ms of audio is added on each
//! side before it is decoded. See `docs/architecture/transcription.md`.
//!
//! The work is split so that everything but the model can be tested without it:
//!
//! - [`AudioSource`] reads a track as 16 kHz mono samples ([`WavSource`] for the WAV
//!   files written by the recorder);
//! - [`VoiceDetector`] finds the stretches of speech and [`SpeechRecognizer`] turns one
//!   stretch into text; an [`Engine`] provides both, and an [`EngineLoader`] loads it
//!   ([`SherpaLoader`] for the real model, fakes in the tests);
//! - [`transcribe_track`] drives detector and recognizer over one track;
//! - [`TrackClock`] places a track's samples on the meeting's time line, from the
//!   recorder's timing sidecar (the capture [`crate::capture::Timeline`]);
//! - [`transcribe_tracks`] does all of it for the `me` and `others` tracks and merges
//!   them into one list of segments, ready for [`crate::storage::Library::add_transcription`].
//!
//! The background job that runs it for a meeting is [`crate::jobs::Transcribe`].
//!
//! The model recognizes the language by itself, sentence by sentence, so English terms
//! in an Italian meeting are transcribed as spoken. The meeting language only has to be
//! one the model knows ([`is_supported_language`]).

mod audio;
mod clock;
mod memory;
mod pipeline;
mod sherpa;

use std::path::PathBuf;

pub use audio::{AudioSource, SliceSource, WavSource};
pub use clock::{NominalClock, TrackClock};
pub use memory::MemoryMonitor;
pub use pipeline::{SpokenSegment, transcribe_track};
pub use sherpa::SherpaLoader;

use crate::storage::{NewSegment, Role};

/// Sample rate the detector and the model work at.
pub const SAMPLE_RATE: u32 = 16_000;

/// Languages of the model (Parakeet TDT 0.6b v3 and its Parakeet Ultra post-training),
/// as ISO 639-1 codes.
pub const SUPPORTED_LANGUAGES: &[&str] = &[
    "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "hr", "hu", "it", "lt", "lv", "mt",
    "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "uk",
];

/// Whether the model knows a language, given as a BCP 47 tag (`it`, `en-GB`).
pub fn is_supported_language(tag: &str) -> bool {
    let primary = tag.split(['-', '_']).next().unwrap_or_default();
    SUPPORTED_LANGUAGES
        .iter()
        .any(|lang| lang.eq_ignore_ascii_case(primary))
}

/// Error codes of [`TranscriptionError`], stable for the interface to translate.
pub mod error_code {
    /// The model is not downloaded (or only partly).
    pub const MODEL_MISSING: &str = "model_missing";
    /// A model file failed its checksum; downloading it again fixes it.
    pub const MODEL_CORRUPT: &str = "model_corrupt";
    /// The runtime could not load the model or the detector (memory, damaged file).
    pub const MODEL_LOAD_FAILED: &str = "model_load_failed";
    /// The meeting has no tracks.
    pub const NO_AUDIO: &str = "no_audio";
    /// A track's audio file is gone (removed by the user, or never written).
    pub const AUDIO_MISSING: &str = "audio_missing";
    /// A track's audio file is not a WAV the engine can read.
    pub const AUDIO_INVALID: &str = "audio_invalid";
    /// Reading a track's audio failed (a disk error).
    pub const AUDIO_READ_FAILED: &str = "audio_read_failed";
    /// The recognizer gave no result for a stretch of speech.
    pub const RECOGNITION_FAILED: &str = "recognition_failed";
    /// The meeting language is not one the model knows.
    pub const LANGUAGE_UNSUPPORTED: &str = "language_unsupported";
    /// The meeting is still being recorded.
    pub const STILL_RECORDING: &str = "still_recording";
    /// The user cancelled the transcription (stored on the meeting).
    pub const CANCELLED: &str = "transcription_cancelled";
    /// The work stopped because it was asked to; same code as
    /// [`crate::jobs::JobError::stopped`].
    pub const STOPPED: &str = "stopped";
}

/// Why a transcription failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {detail}")]
pub struct TranscriptionError {
    /// One of [`error_code`].
    pub code: &'static str,
    /// Technical detail for diagnostics; not shown as is.
    pub detail: String,
    /// Whether trying again later may succeed.
    pub retryable: bool,
}

impl TranscriptionError {
    pub fn permanent(code: &'static str, detail: impl ToString) -> Self {
        Self {
            code,
            detail: detail.to_string(),
            retryable: false,
        }
    }

    pub fn retryable(code: &'static str, detail: impl ToString) -> Self {
        Self {
            code,
            detail: detail.to_string(),
            retryable: true,
        }
    }

    pub fn stopped() -> Self {
        Self::retryable(error_code::STOPPED, "")
    }
}

/// Result of transcription operations.
pub type Result<T> = std::result::Result<T, TranscriptionError>;

/// How speech is cut into stretches before decoding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VadSettings {
    /// Speech probability above which a window counts as speech.
    pub threshold: f32,
    /// Silence that closes a stretch, in seconds.
    pub min_silence_s: f32,
    /// Shortest stretch kept, in seconds.
    pub min_speech_s: f32,
    /// Longest stretch, in seconds; longer speech is split.
    pub max_speech_s: f32,
    /// Audio added before and after each stretch when it is decoded, in milliseconds.
    pub padding_ms: u32,
}

impl VadSettings {
    /// Settings chosen by spike #10 (ADR 0007): 2.15% WER on Italian FLEURS.
    pub const ADR_0007: Self = Self {
        threshold: 0.5,
        min_silence_s: 1.0,
        min_speech_s: 0.25,
        max_speech_s: 30.0,
        padding_ms: 500,
    };

    /// The padding in samples at [`SAMPLE_RATE`].
    pub fn padding_samples(&self) -> u64 {
        u64::from(self.padding_ms) * u64::from(SAMPLE_RATE) / 1000
    }
}

impl Default for VadSettings {
    fn default() -> Self {
        Self::ADR_0007
    }
}

/// A stretch of speech found by a [`VoiceDetector`], in samples at [`SAMPLE_RATE`] from
/// the start of the track; `end` is exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeechSpan {
    pub start: u64,
    pub end: u64,
}

/// Finds stretches of speech in a stream of 16 kHz mono samples.
pub trait VoiceDetector {
    /// Feeds the next samples.
    fn accept(&mut self, samples: &[f32]);
    /// Ends the stream: speech still open becomes a stretch.
    fn flush(&mut self);
    /// The next stretch found, oldest first.
    fn pop(&mut self) -> Option<SpeechSpan>;
    /// Whether the detector is inside speech right now (a stretch is still open).
    fn in_speech(&self) -> bool;
}

/// Turns one stretch of 16 kHz mono speech into text.
pub trait SpeechRecognizer {
    fn recognize(&mut self, samples: &[f32]) -> Result<String>;
}

/// A loaded transcription engine.
pub trait Engine {
    /// Engine and model, stored with the transcription
    /// ([`crate::storage::Version::engine`]).
    fn description(&self) -> String;
    /// How the engine cuts speech.
    fn settings(&self) -> VadSettings;
    /// A new detector, for one track.
    fn detector(&mut self) -> Result<Box<dyn VoiceDetector>>;
    /// The recognizer, shared by every track.
    fn recognizer(&mut self) -> &mut dyn SpeechRecognizer;
}

/// Loads an engine. The model takes gigabytes of memory, so it is loaded for each
/// transcription and dropped after it.
pub trait EngineLoader: Send + Sync {
    fn load(&self) -> Result<Box<dyn Engine>>;
}

/// One track of a meeting, with absolute paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackAudio {
    pub role: Role,
    pub audio_path: PathBuf,
    /// The recorder's timing sidecar, if any; without it the track is placed at its
    /// nominal sample rate from the start of the recording.
    pub timing_path: Option<PathBuf>,
}

/// Transcribes a meeting's tracks and merges them into one timeline.
///
/// `progress` receives the fraction done (weighted by the length of each track);
/// `should_stop` is checked between stretches, and the work ends with
/// [`error_code::STOPPED`] soon after it returns true.
pub fn transcribe_tracks(
    engine: &mut dyn Engine,
    tracks: &[TrackAudio],
    progress: &mut dyn FnMut(f32),
    should_stop: &dyn Fn() -> bool,
) -> Result<Vec<NewSegment>> {
    if tracks.is_empty() {
        return Err(TranscriptionError::permanent(
            error_code::NO_AUDIO,
            "the meeting has no tracks",
        ));
    }
    let mut sources = tracks
        .iter()
        .map(|track| WavSource::open(&track.audio_path))
        .collect::<Result<Vec<_>>>()?;
    let lengths: Vec<f64> = sources.iter().map(|s| s.duration_s().max(1e-3)).collect();
    let total: f64 = lengths.iter().sum();

    let settings = engine.settings();
    let mut segments = Vec::new();
    let mut done = 0.0;
    for ((track, source), length) in tracks.iter().zip(&mut sources).zip(&lengths) {
        let clock = track_clock(track, source.sample_rate());
        let mut detector = engine.detector()?;
        let spoken = transcribe_track(
            source,
            detector.as_mut(),
            engine.recognizer(),
            &settings,
            &mut |fraction| progress(((done + fraction as f64 * length) / total) as f32),
            should_stop,
        )?;
        let frames_per_sample = f64::from(source.sample_rate()) / f64::from(SAMPLE_RATE);
        segments.extend(spoken.into_iter().map(|segment| {
            let start = clock.recording_ms(segment.start as f64 * frames_per_sample);
            let end = clock.recording_ms(segment.end as f64 * frames_per_sample);
            let start_ms = start.floor().max(0.0) as i64;
            NewSegment {
                role: track.role,
                start_ms,
                end_ms: (end.ceil() as i64).max(start_ms),
                text: segment.text,
            }
        }));
        done += length;
        progress((done / total) as f32);
    }
    Ok(merge(segments))
}

/// The clock of a track: its sidecar, or the nominal rate if it has none or the sidecar
/// can't be read (the realignment is lost, not the transcript, and the problem is logged).
fn track_clock(track: &TrackAudio, sample_rate: u32) -> Box<dyn TrackClock> {
    let nominal = Box::new(NominalClock::new(sample_rate));
    let Some(path) = &track.timing_path else {
        return nominal;
    };
    match crate::capture::Timeline::load(path) {
        Ok(clock) => Box::new(clock),
        Err(err) => {
            eprintln!(
                "trama: placing the {} track at its nominal rate: {err}",
                track.role.as_str()
            );
            nominal
        }
    }
}

/// Puts the segments of every track on one timeline: by start time, then end time, `me`
/// before `others` at the same instant. Segments without text are dropped.
pub fn merge(mut segments: Vec<NewSegment>) -> Vec<NewSegment> {
    segments.retain(|segment| !segment.text.trim().is_empty());
    segments.sort_by_key(|segment| {
        (
            segment.start_ms,
            segment.end_ms,
            matches!(segment.role, Role::Others),
        )
    });
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_are_matched_by_their_primary_subtag() {
        assert!(is_supported_language("it"));
        assert!(is_supported_language("en-GB"));
        assert!(is_supported_language("PT_br"));
        assert!(!is_supported_language("ja"));
        assert!(!is_supported_language(""));
    }

    #[test]
    fn merge_orders_both_tracks_on_one_timeline() {
        let seg = |role, start_ms, end_ms, text: &str| NewSegment {
            role,
            start_ms,
            end_ms,
            text: text.into(),
        };
        let merged = merge(vec![
            seg(Role::Me, 0, 1_000, "ciao"),
            seg(Role::Me, 5_000, 6_000, "bene"),
            seg(Role::Others, 2_000, 3_000, "come stai?"),
            seg(Role::Others, 5_000, 6_000, "sì"),
            seg(Role::Others, 7_000, 8_000, "  "),
        ]);
        let texts: Vec<_> = merged.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["ciao", "come stai?", "bene", "sì"]);
    }

    #[test]
    fn the_padding_is_counted_in_samples() {
        assert_eq!(VadSettings::ADR_0007.padding_samples(), 8_000);
    }
}
