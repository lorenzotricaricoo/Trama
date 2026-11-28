//! Meeting audio capture: two tracks, the user's microphone and the system audio of the
//! other participants, written progressively to the library.
//!
//! The design comes from spike #11 and is described in
//! `docs/architecture/audio-capture.md`. In short:
//!
//! - A [`CaptureBackend`] opens one stream per [`Source`]. The audio callback only copies
//!   samples into a lock-free ring buffer through a [`FrameSink`]; it never blocks.
//! - One writer thread per track drains the ring buffer into a 32-bit float WAV under
//!   `audio/<meeting id>/`, patches the header and syncs the file every second, and
//!   writes a timing sidecar (`<track>.timing.jsonl`) that maps file frames to the host
//!   clock. A crash loses at most the last writer pass (about 20 ms); [`repair_wav`]
//!   recovers what the header does not yet count.
//! - One supervisor thread per track reopens the stream when the device changes, fails
//!   or stalls. Gaps are filled with silence and recorded in the sidecar, so both files
//!   stay on the recording's time line; [`Timeline`] removes the remaining clock drift.
//! - Live levels and notable events go to a [`CaptureObserver`].
//!
//! Errors carry stable codes ([`error_code`]); the interface translates them.

mod backend;
mod convert;
mod recorder;
mod timing;
pub mod wav;
mod writer;

#[cfg(target_os = "macos")]
pub mod macos;

use std::fmt;

pub use backend::{ActiveStream, CaptureBackend, FaultReporter, FrameSink};
pub use recorder::{
    Recording, RecordingOptions, RecordingSummary, TrackSummary, Tuning, repair_meeting_audio,
};
pub use timing::{Timeline, TimingLine};
pub use wav::{WavInfo, repair_wav};

use crate::storage::Role;

/// Stable error codes of the capture layer, for [`CaptureError::code`] and
/// [`CaptureEvent`]. The interface translates them; they never change meaning.
pub mod error_code {
    /// The user denied microphone access in System Settings.
    pub const MICROPHONE_DENIED: &str = "microphone_denied";
    /// Microphone access is blocked by a policy (parental controls, MDM).
    pub const MICROPHONE_RESTRICTED: &str = "microphone_restricted";
    /// The microphone has delivered only digital silence since the start: access is
    /// probably denied, or the input is muted in hardware.
    pub const MICROPHONE_SILENT: &str = "microphone_silent";
    /// System audio has delivered only digital silence since the start: "System Audio
    /// Recording" is probably not granted. macOS reports no error in that case.
    pub const SYSTEM_AUDIO_SILENT: &str = "system_audio_silent";
    /// System audio recording was denied (ScreenCaptureKit fallback).
    pub const SYSTEM_AUDIO_DENIED: &str = "system_audio_denied";
    /// Neither the Core Audio process tap nor the fallback could capture system audio.
    pub const SYSTEM_AUDIO_UNAVAILABLE: &str = "system_audio_unavailable";
    /// There is no input device (microphone).
    pub const NO_INPUT_DEVICE: &str = "no_input_device";
    /// There is no output device to tap.
    pub const NO_OUTPUT_DEVICE: &str = "no_output_device";
    /// The chosen device is gone.
    pub const DEVICE_UNAVAILABLE: &str = "device_unavailable";
    /// The device exists but the stream could not be opened.
    pub const DEVICE_OPEN_FAILED: &str = "device_open_failed";
    /// The stream reported an error and was closed.
    pub const STREAM_FAILED: &str = "stream_failed";
    /// The stream stopped delivering audio and was reopened.
    pub const STREAM_STALLED: &str = "stream_stalled";
    /// The audio buffer overflowed: the disk could not keep up and audio was lost.
    pub const FRAMES_DROPPED: &str = "frames_dropped";
    /// The track's files could not be written (disk full, permissions).
    pub const WRITE_FAILED: &str = "write_failed";
    /// The track reached the 4 GiB limit of a WAV file; the rest is not recorded.
    pub const TRACK_SIZE_LIMIT: &str = "track_size_limit";
    /// The meeting already has audio files; they are never overwritten.
    pub const ALREADY_RECORDED: &str = "already_recorded";
    /// Neither track was requested.
    pub const NOTHING_TO_RECORD: &str = "nothing_to_record";
    /// A file is not a WAV written by Trama.
    pub const INVALID_WAV: &str = "invalid_wav";
    /// An internal thread failed; a bug.
    pub const INTERNAL: &str = "internal";
}

/// What went wrong, as a stable code plus technical detail for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureError {
    /// One of [`error_code`].
    pub code: &'static str,
    /// Technical detail, for logs; not shown as is.
    pub detail: String,
}

impl CaptureError {
    pub fn new(code: &'static str, detail: impl ToString) -> Self {
        Self {
            code,
            detail: detail.to_string(),
        }
    }

    pub(crate) fn io(code: &'static str, err: std::io::Error) -> Self {
        Self::new(code, err)
    }
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.detail.is_empty() {
            f.write_str(self.code)
        } else {
            write!(f, "{}: {}", self.code, self.detail)
        }
    }
}

impl std::error::Error for CaptureError {}

/// Result type of the capture layer.
pub type Result<T, E = CaptureError> = std::result::Result<T, E>;

/// Which side of the meeting a track records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// The user's microphone: the `me` track.
    Microphone,
    /// What the Mac plays, the other participants: the `others` track.
    System,
}

impl Source {
    /// Both sources, in track order.
    pub const ALL: [Source; 2] = [Source::Microphone, Source::System];

    /// Role of the track in the data model.
    pub const fn role(self) -> Role {
        match self {
            Self::Microphone => Role::Me,
            Self::System => Role::Others,
        }
    }

    /// Stable name, used in sidecars and logs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Microphone => "microphone",
            Self::System => "system",
        }
    }

    /// Code of the "only silence since the start" warning for this source.
    pub(crate) const fn silent_code(self) -> &'static str {
        match self {
            Self::Microphone => error_code::MICROPHONE_SILENT,
            Self::System => error_code::SYSTEM_AUDIO_SILENT,
        }
    }
}

/// Sample rate and channel count of a stream or a track. Samples are always `f32`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

/// An audio device, for change detection and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Stable identifier while the device is connected.
    pub id: String,
    /// Name shown by the system.
    pub name: String,
}

/// Whether the app may capture a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    /// The user said no; only System Settings can change it.
    Denied,
    /// Blocked by a policy the user cannot change.
    Restricted,
    /// The user has not been asked yet.
    NotDetermined,
    /// The system offers no way to know (system audio on macOS, see
    /// [`CaptureBackend::probe_system_audio_permission`]).
    Unknown,
}

/// Live level of a track, for meters. Values are linear, 1.0 is full scale; float input
/// can exceed it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Level {
    pub source: Source,
    /// Highest absolute sample in the window.
    pub peak: f32,
    /// Root mean square over the window.
    pub rms: f32,
}

impl Level {
    /// Peak in dBFS, at least -120.
    pub fn peak_dbfs(&self) -> f32 {
        to_dbfs(self.peak)
    }

    /// RMS in dBFS, at least -120.
    pub fn rms_dbfs(&self) -> f32 {
        to_dbfs(self.rms)
    }
}

fn to_dbfs(linear: f32) -> f32 {
    if linear <= 1e-6 {
        -120.0
    } else {
        (20.0 * linear.log10()).max(-120.0)
    }
}

/// Something worth telling the interface about while recording.
#[derive(Debug, Clone, PartialEq)]
pub enum CaptureEvent {
    /// A stream was opened on a device (at the start and after every reopen).
    DeviceOpened {
        source: Source,
        device: DeviceInfo,
        format: StreamFormat,
        backend: &'static str,
    },
    /// The first audio of the track arrived, `offset_ms` after the recording started.
    /// System audio starts only when the Mac first plays something; the gap is filled
    /// with silence.
    TrackStarted { source: Source, offset_ms: u64 },
    /// The default device changed; the stream follows it.
    DeviceChanged { source: Source, device: DeviceInfo },
    /// The stream was lost (`code`: [`error_code::STREAM_FAILED`],
    /// [`error_code::STREAM_STALLED`], [`error_code::DEVICE_UNAVAILABLE`], ...). The
    /// supervisor keeps trying to reopen it.
    TrackInterrupted {
        source: Source,
        code: &'static str,
        detail: String,
    },
    /// A warning or a failure of the track that does not stop the other one: silence
    /// since the start, dropped frames, write errors, the size limit.
    TrackProblem {
        source: Source,
        code: &'static str,
        detail: String,
    },
}

/// Receives levels and events. Called from the capture threads: it must not block.
pub trait CaptureObserver: Send + Sync {
    /// About 20 times a second per track while audio flows.
    fn level(&self, _level: Level) {}
    fn event(&self, _event: CaptureEvent) {}
}

/// An observer that ignores everything.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoObserver;

impl CaptureObserver for NoObserver {}

/// File names of a track inside the meeting's audio folder.
pub(crate) fn track_file_names(source: Source) -> (String, String) {
    let stem = source.role().as_str();
    (format!("{stem}.wav"), format!("{stem}.timing.jsonl"))
}
