//! Converts a stream's buffers to the track's format, when a device change brings a
//! different sample rate or channel count (a Bluetooth headset switching to 16 kHz, for
//! example). Linear interpolation: enough for speech, and a track keeps one format.

use super::StreamFormat;

/// Streaming converter from one format to another.
#[derive(Debug)]
pub(crate) struct Converter {
    input: StreamFormat,
    output: StreamFormat,
    /// Input frames per output frame.
    step: f64,
    /// Position of the next output frame, in input frames, where 0 is `prev`.
    pos: f64,
    /// Last input frame of the previous buffer, in the output channel layout.
    prev: Vec<f32>,
    mapped: Vec<f32>,
}

impl Converter {
    pub fn new(input: StreamFormat, output: StreamFormat) -> Self {
        Self {
            input,
            output,
            step: f64::from(input.sample_rate) / f64::from(output.sample_rate),
            pos: 1.0,
            prev: vec![0.0; usize::from(output.channels)],
            mapped: Vec::new(),
        }
    }

    /// Output frames that `frames` input frames will roughly produce.
    pub fn output_frames(&self, frames: u64) -> u64 {
        (frames as f64 / self.step).round() as u64
    }

    /// Converts one buffer of interleaved input samples, appending to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let in_ch = usize::from(self.input.channels);
        let out_ch = usize::from(self.output.channels);
        let frames = input.len() / in_ch;
        if frames == 0 {
            return;
        }
        // 1. Channel layout.
        self.mapped.clear();
        for frame in input.chunks_exact(in_ch) {
            if out_ch == in_ch {
                self.mapped.extend_from_slice(frame);
            } else if out_ch == 1 {
                self.mapped.push(frame.iter().sum::<f32>() / in_ch as f32);
            } else {
                for c in 0..out_ch {
                    self.mapped.push(frame[c.min(in_ch - 1)]);
                }
            }
        }
        if self.input.sample_rate == self.output.sample_rate {
            out.extend_from_slice(&self.mapped);
            return;
        }
        // 2. Sample rate. Frame i of [prev, mapped...] is prev for i = 0.
        let frame = |i: usize, c: usize, prev: &[f32], mapped: &[f32]| {
            if i == 0 {
                prev[c]
            } else {
                mapped[(i - 1) * out_ch + c]
            }
        };
        while self.pos <= frames as f64 + 1e-9 {
            let i = self.pos.floor() as usize;
            let frac = (self.pos - i as f64) as f32;
            for c in 0..out_ch {
                let a = frame(i, c, &self.prev, &self.mapped);
                let b = if i < frames {
                    frame(i + 1, c, &self.prev, &self.mapped)
                } else {
                    a
                };
                out.push(a + (b - a) * frac);
            }
            self.pos += self.step;
        }
        self.pos -= frames as f64;
        self.prev
            .copy_from_slice(&self.mapped[(frames - 1) * out_ch..frames * out_ch]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(sample_rate: u32, channels: u16) -> StreamFormat {
        StreamFormat {
            sample_rate,
            channels,
        }
    }

    #[test]
    fn same_format_is_a_copy() {
        let mut c = Converter::new(fmt(48_000, 2), fmt(48_000, 2));
        let mut out = Vec::new();
        c.process(&[0.1, 0.2, 0.3, 0.4], &mut out);
        assert_eq!(out, [0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn mixes_down_to_mono_and_up_to_stereo() {
        let mut down = Converter::new(fmt(48_000, 2), fmt(48_000, 1));
        let mut out = Vec::new();
        down.process(&[0.2, 0.4, -1.0, 1.0], &mut out);
        assert_eq!(out, [0.3, 0.0]);

        let mut up = Converter::new(fmt(48_000, 1), fmt(48_000, 2));
        out.clear();
        up.process(&[0.5, -0.5], &mut out);
        assert_eq!(out, [0.5, 0.5, -0.5, -0.5]);
    }

    #[test]
    fn resamples_keeping_length_and_shape() {
        // One second of a 200 Hz sine at 16 kHz, in uneven buffers, to 48 kHz.
        let input: Vec<f32> = (0..16_000)
            .map(|i| (2.0 * std::f32::consts::PI * 200.0 * i as f32 / 16_000.0).sin())
            .collect();
        let mut c = Converter::new(fmt(16_000, 1), fmt(48_000, 1));
        let mut out = Vec::new();
        for chunk in input.chunks(333) {
            c.process(chunk, &mut out);
        }
        assert!((out.len() as i64 - 48_000).abs() <= 3, "{}", out.len());
        for (i, s) in out.iter().enumerate().take(47_990) {
            let expected = (2.0 * std::f32::consts::PI * 200.0 * i as f32 / 48_000.0).sin();
            assert!((s - expected).abs() < 0.01, "frame {i}: {s} vs {expected}");
        }
    }

    #[test]
    fn downsamples() {
        let mut c = Converter::new(fmt(48_000, 1), fmt(16_000, 1));
        let mut out = Vec::new();
        for _ in 0..100 {
            c.process(&[0.25; 480], &mut out);
        }
        assert!((out.len() as i64 - 16_000).abs() <= 1);
        assert!(out[1..].iter().all(|s| (s - 0.25).abs() < 1e-6));
        assert_eq!(c.output_frames(480), 160);
    }
}
