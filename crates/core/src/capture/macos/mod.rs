//! The macOS backend: the microphone through `cpal`, system audio through a Core Audio
//! process tap, with ScreenCaptureKit as fallback (cargo feature `screencapturekit`).
//!
//! Needs macOS 14.2 or later (the app's minimum) for the process tap. Permissions are
//! granted to the signed app: a binary started from a terminal inherits the terminal's.

// Core Audio, AVFoundation and the IOProc callback are C and Objective-C APIs; the
// unsafe code is confined to this module and documented at each block.
#![allow(unsafe_code)]

mod mic;
mod permission;
#[cfg(feature = "screencapturekit")]
mod sck;
mod tap;

use super::{
    ActiveStream, CaptureBackend, CaptureError, DeviceInfo, FaultReporter, FrameSink,
    PermissionState, Result, Source,
};

/// Host time in nanoseconds (`mach_absolute_time`), the clock of every buffer timestamp.
pub fn host_time_ns() -> u64 {
    // SAFETY: plain C functions with no preconditions.
    unsafe {
        objc2_core_audio::AudioConvertHostTimeToNanos(objc2_core_audio::AudioGetCurrentHostTime())
    }
}

/// The macOS capture backend.
#[derive(Debug, Default)]
pub struct MacBackend {
    _private: (),
}

impl MacBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Input devices, for a microphone picker.
    pub fn input_devices(&self) -> Result<Vec<DeviceInfo>> {
        mic::input_devices()
    }
}

impl CaptureBackend for MacBackend {
    fn now_ns(&self) -> u64 {
        host_time_ns()
    }

    fn microphone_permission(&self) -> PermissionState {
        permission::microphone()
    }

    fn request_microphone_permission(&self) -> PermissionState {
        permission::request_microphone()
    }

    fn probe_system_audio_permission(&self) -> PermissionState {
        tap::probe()
    }

    fn default_device(&self, source: Source) -> Result<Option<DeviceInfo>> {
        match source {
            Source::Microphone => Ok(mic::default_device()),
            Source::System => tap::default_output_device(),
        }
    }

    fn open(
        &self,
        source: Source,
        device: Option<&str>,
        sink: FrameSink,
        faults: FaultReporter,
    ) -> Result<Box<dyn ActiveStream>> {
        match source {
            Source::Microphone => mic::open(device, sink, faults),
            Source::System => open_system(sink, faults),
        }
    }
}

#[cfg(not(feature = "screencapturekit"))]
fn open_system(sink: FrameSink, _faults: FaultReporter) -> Result<Box<dyn ActiveStream>> {
    tap::open(sink)
}

#[cfg(feature = "screencapturekit")]
fn open_system(sink: FrameSink, faults: FaultReporter) -> Result<Box<dyn ActiveStream>> {
    // The tap is tried first: it needs only "System Audio Recording", while
    // ScreenCaptureKit needs screen recording access, which macOS asks to confirm again
    // periodically. The sink is handed back when the tap cannot even be created.
    match tap::try_open(sink) {
        Ok(stream) => Ok(stream),
        Err((tap_err, Some(sink))) => sck::open(sink, faults).map_err(|sck_err| {
            CaptureError::new(
                sck_err.code,
                format!(
                    "process tap: {tap_err}; ScreenCaptureKit: {}",
                    sck_err.detail
                ),
            )
        }),
        Err((tap_err, None)) => Err(tap_err),
    }
}

/// An `OSStatus` as an error, with its four-character code when it has one.
fn os_error(code: &'static str, what: &str, status: i32) -> CaptureError {
    let fourcc: String = status
        .to_be_bytes()
        .iter()
        .map(|&c| if c.is_ascii_graphic() { c as char } else { '.' })
        .collect();
    CaptureError::new(
        code,
        format!("{what} failed: OSStatus {status} ('{fourcc}')"),
    )
}
