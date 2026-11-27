//! Fallback: system audio via ScreenCaptureKit (macOS 13+). Needs the
//! Screen & System Audio Recording permission (TCC kTCCServiceScreenCapture),
//! which is broader than the process tap's "System Audio Recording Only".
//! Build with `--features sck`.

use crate::sink::RtSide;
use anyhow::{Result, anyhow};
use screencapturekit::prelude::*;
use std::sync::Mutex;

struct Handler {
    rt: Mutex<(RtSide, Vec<f32>)>,
}

impl SCStreamOutputTrait for Handler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, kind: SCStreamOutputType) {
        if !matches!(kind, SCStreamOutputType::Audio) {
            return;
        }
        let Ok(abl) = sample.audio_buffer_list() else { return };
        // Audio PTS is on the host-time clock: same time base as the mic track.
        let host_ns = sample.presentation_timestamp().as_seconds().map(|s| (s * 1e9) as u64).unwrap_or(0);
        let mut g = self.rt.lock().unwrap();
        let (rt, scratch) = &mut *g;
        let n = abl.num_buffers();
        let as_f32 = |b: &[u8]| -> Vec<f32> { b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect() };
        if n == 1 {
            let b = abl.get(0).unwrap();
            let s = as_f32(b.data());
            rt.push(&s, b.number_channels.max(1) as usize, host_ns, f64::NAN);
        } else if n > 1 {
            let chans: Vec<Vec<f32>> = (0..n).filter_map(|i| abl.get(i)).map(|b| as_f32(b.data())).collect();
            let frames = chans.iter().map(|c| c.len()).min().unwrap_or(0);
            scratch.clear();
            for f in 0..frames {
                for c in &chans {
                    scratch.push(c[f]);
                }
            }
            rt.push(scratch, chans.len(), host_ns, f64::NAN);
        }
    }
}

pub fn start(rt: RtSide, rate: u32, channels: u16) -> Result<SCStream> {
    let content = SCShareableContent::get().map_err(|e| anyhow!("SCShareableContent::get failed (Screen Recording permission?): {e:?}"))?;
    let display = content.displays().into_iter().next().ok_or_else(|| anyhow!("no display"))?;
    let filter = SCContentFilter::create().with_display(&display).with_excluding_windows(&[]).build().map_err(|e| anyhow!("{e:?}"))?;
    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_captures_audio(true)
        .with_excludes_current_process_audio(true)
        .with_sample_rate(rate as i32)
        .with_channel_count(channels as i32);
    let mut stream = SCStream::new(&filter, &config).map_err(|e| anyhow!("{e:?}"))?;
    stream
        .add_output_handler(Handler { rt: Mutex::new((rt, Vec::with_capacity(8192))) }, SCStreamOutputType::Audio)
        .map_err(|e| anyhow!("{e:?}"))?;
    stream.start_capture().map_err(|e| anyhow!("start_capture failed: {e:?}"))?;
    Ok(stream)
}
