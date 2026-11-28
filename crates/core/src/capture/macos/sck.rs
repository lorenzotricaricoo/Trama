//! Fallback: system audio through ScreenCaptureKit, used only when the process tap cannot
//! be created. It needs the broader "Screen & System Audio Recording" permission, which
//! macOS asks the user to confirm again periodically.
//!
//! The `screencapturekit` crate links a Swift bridge: binaries need `/usr/lib/swift` on
//! their rpath (`build.rs` adds it for this crate's tests and examples).

use std::sync::Mutex;

use screencapturekit::prelude::*;

use super::super::{
    ActiveStream, CaptureError, DeviceInfo, FaultReporter, FrameSink, Result, StreamFormat,
    error_code,
};

const FORMAT: StreamFormat = StreamFormat {
    sample_rate: 48_000,
    channels: 2,
};

struct Handler {
    state: Mutex<(FrameSink, Vec<f32>)>,
}

fn floats(bytes: &[u8]) -> impl Iterator<Item = f32> + '_ {
    bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

impl SCStreamOutputTrait for Handler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, kind: SCStreamOutputType) {
        if !matches!(kind, SCStreamOutputType::Audio) {
            return;
        }
        let Ok(list) = sample.audio_buffer_list() else {
            return;
        };
        // Presentation times are on the host clock, like the microphone's timestamps.
        let host_ns = sample
            .presentation_timestamp()
            .as_seconds()
            .map_or(0, |s| (s * 1e9) as u64);
        // Called on a serial dispatch queue, not a real-time thread: a lock is fine.
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let (sink, scratch) = &mut *state;
        let count = list.num_buffers();
        scratch.clear();
        if count == 1 {
            let Some(buffer) = list.get(0) else { return };
            scratch.extend(floats(buffer.data()));
            sink.push(scratch, buffer.number_channels.max(1) as usize, host_ns);
        } else if count > 1 {
            let channels: Vec<Vec<f32>> = (0..count)
                .filter_map(|i| list.get(i))
                .map(|b| floats(b.data()).collect())
                .collect();
            let frames = channels.iter().map(Vec::len).min().unwrap_or(0);
            for i in 0..frames {
                scratch.extend(channels.iter().map(|c| c[i]));
            }
            sink.push(scratch, channels.len(), host_ns);
        }
    }
}

struct SckStream {
    stream: SCStream,
}

impl ActiveStream for SckStream {
    fn format(&self) -> StreamFormat {
        FORMAT
    }
    fn device(&self) -> DeviceInfo {
        DeviceInfo {
            id: "screencapturekit".into(),
            name: "ScreenCaptureKit".into(),
        }
    }
    fn backend(&self) -> &'static str {
        "screencapturekit"
    }
}

impl Drop for SckStream {
    fn drop(&mut self) {
        let _ = self.stream.stop_capture();
    }
}

pub(super) fn open(sink: FrameSink, _faults: FaultReporter) -> Result<Box<dyn ActiveStream>> {
    let content = SCShareableContent::get()
        .map_err(|e| CaptureError::new(error_code::SYSTEM_AUDIO_DENIED, format!("{e:?}")))?;
    let display = content
        .displays()
        .into_iter()
        .next()
        .ok_or_else(|| CaptureError::new(error_code::SYSTEM_AUDIO_UNAVAILABLE, "no display"))?;
    let unavailable = |e: &dyn std::fmt::Debug| {
        CaptureError::new(error_code::SYSTEM_AUDIO_UNAVAILABLE, format!("{e:?}"))
    };
    let filter = SCContentFilter::create()
        .with_display(&display)
        .with_excluding_windows(&[])
        .build()
        .map_err(|e| unavailable(&e))?;
    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_captures_audio(true)
        .with_excludes_current_process_audio(true)
        .with_sample_rate(FORMAT.sample_rate as i32)
        .with_channel_count(i32::from(FORMAT.channels));
    let mut stream = SCStream::new(&filter, &config).map_err(|e| unavailable(&e))?;
    stream
        .add_output_handler(
            Handler {
                state: Mutex::new((sink, Vec::with_capacity(8_192))),
            },
            SCStreamOutputType::Audio,
        )
        .map_err(|e| unavailable(&e))?;
    stream
        .start_capture()
        .map_err(|e| CaptureError::new(error_code::SYSTEM_AUDIO_DENIED, format!("{e:?}")))?;
    Ok(Box::new(SckStream { stream }))
}
