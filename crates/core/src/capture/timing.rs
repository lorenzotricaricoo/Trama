//! The timing sidecar of a track, and the time line that realigns tracks.
//!
//! A sidecar is a JSON Lines file next to the WAV. Every line is one [`TimingLine`];
//! anchors pair a frame of the file with the host time it was captured at, about once a
//! second and on both sides of every discontinuity. Mapping both tracks to the host clock
//! through their anchors removes their different start times, gaps and clock drift, with
//! no resampling.

use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{CaptureError, Result, error_code};

/// Version of the sidecar format, in [`TimingLine::Start`].
pub const TIMING_VERSION: u32 = 1;

/// One line of a timing sidecar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum TimingLine {
    /// First line: the track's format and the recording's time origin.
    Start {
        version: u32,
        source: String,
        sample_rate: u32,
        channels: u16,
        /// Host time the recording started at; recording time 0.
        recording_start_ns: u64,
        /// Wall clock at the start, UTC milliseconds since the Unix epoch.
        unix_ms: i64,
    },
    /// A stream was opened; its buffers follow.
    Segment {
        frame: u64,
        device_id: String,
        device_name: String,
        backend: String,
        sample_rate: u32,
        channels: u16,
    },
    /// File frame `frame` was captured at host time `host_ns`.
    Anchor { frame: u64, host_ns: u64 },
    /// `frames` frames of silence were written at `frame`: before the first audio
    /// (`start`), over a gap in the stream (`gap`), for audio lost in an overflow
    /// (`dropped`), or up to the stop time (`end`).
    Pad {
        frame: u64,
        frames: u64,
        reason: String,
    },
    /// The stream was lost at host time `host_ns`.
    Interrupted { host_ns: u64, code: String },
    /// Last line of a track that stopped normally.
    Stop {
        frame: u64,
        host_ns: u64,
        unix_ms: i64,
    },
}

/// Maps the frames of one track to recording time, from its sidecar.
#[derive(Debug, Clone, PartialEq)]
pub struct Timeline {
    sample_rate: u32,
    recording_start_ns: u64,
    /// (frame, host_ns), sorted by frame.
    points: Vec<(u64, u64)>,
    lines: Vec<TimingLine>,
}

impl Timeline {
    /// Reads a sidecar. A torn last line, left by a crash, is ignored.
    pub fn load(path: &Path) -> Result<Self> {
        let invalid = |why: String| {
            CaptureError::new(
                error_code::INVALID_WAV,
                format!("{}: {why}", path.display()),
            )
        };
        let file = std::fs::File::open(path).map_err(|e| invalid(e.to_string()))?;
        let mut lines = Vec::new();
        let raw: Vec<String> = BufReader::new(file)
            .lines()
            .collect::<std::io::Result<_>>()
            .map_err(|e| invalid(e.to_string()))?;
        let last = raw.len().saturating_sub(1);
        for (i, text) in raw.iter().enumerate() {
            if text.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<TimingLine>(text) {
                Ok(line) => lines.push(line),
                Err(_) if i == last => break,
                Err(e) => return Err(invalid(format!("line {}: {e}", i + 1))),
            }
        }
        Self::from_lines(lines).map_err(invalid)
    }

    /// Builds a time line from parsed lines.
    pub fn from_lines(lines: Vec<TimingLine>) -> std::result::Result<Self, String> {
        let Some(TimingLine::Start {
            sample_rate,
            recording_start_ns,
            ..
        }) = lines.first()
        else {
            return Err("the sidecar does not begin with a start line".into());
        };
        let (sample_rate, recording_start_ns) = (*sample_rate, *recording_start_ns);
        if sample_rate == 0 {
            return Err("sample rate 0".into());
        }
        let mut points: Vec<(u64, u64)> = lines
            .iter()
            .filter_map(|line| match line {
                TimingLine::Anchor { frame, host_ns } | TimingLine::Stop { frame, host_ns, .. } => {
                    Some((*frame, *host_ns))
                }
                _ => None,
            })
            .collect();
        points.sort_by_key(|&(frame, _)| frame);
        points.dedup_by_key(|&mut (frame, _)| frame);
        Ok(Self {
            sample_rate,
            recording_start_ns,
            points,
            lines,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The lines of the sidecar, for diagnostics.
    pub fn lines(&self) -> &[TimingLine] {
        &self.lines
    }

    /// Host time of a frame, interpolating between anchors and extrapolating at the
    /// nominal rate outside them.
    pub fn host_ns(&self, frame: f64) -> f64 {
        let ns_per_frame = 1e9 / f64::from(self.sample_rate);
        let pts = &self.points;
        match pts.len() {
            0 => self.recording_start_ns as f64 + frame * ns_per_frame,
            1 => pts[0].1 as f64 + (frame - pts[0].0 as f64) * ns_per_frame,
            _ => {
                let i = pts.partition_point(|&(f, _)| (f as f64) <= frame);
                if i == 0 {
                    pts[0].1 as f64 + (frame - pts[0].0 as f64) * ns_per_frame
                } else if i == pts.len() {
                    let (f, h) = pts[pts.len() - 1];
                    h as f64 + (frame - f as f64) * ns_per_frame
                } else {
                    let (f0, h0) = pts[i - 1];
                    let (f1, h1) = pts[i];
                    let t = (frame - f0 as f64) / (f1 - f0) as f64;
                    h0 as f64 + t * (h1 as f64 - h0 as f64)
                }
            }
        }
    }

    /// Recording time of a frame, in milliseconds since the recording started.
    pub fn recording_ms(&self, frame: f64) -> f64 {
        (self.host_ns(frame) - self.recording_start_ns as f64) / 1e6
    }

    /// The frame captured at a recording time, the inverse of [`recording_ms`](Self::recording_ms).
    pub fn frame_at(&self, recording_ms: f64) -> f64 {
        let target = self.recording_start_ns as f64 + recording_ms * 1e6;
        let ns_per_frame = 1e9 / f64::from(self.sample_rate);
        let pts = &self.points;
        if pts.len() < 2 {
            let (f, h) = pts.first().copied().unwrap_or((0, self.recording_start_ns));
            return f as f64 + (target - h as f64) / ns_per_frame;
        }
        let i = pts.partition_point(|&(_, h)| (h as f64) <= target);
        let (a, b) = if i == 0 {
            return pts[0].0 as f64 + (target - pts[0].1 as f64) / ns_per_frame;
        } else if i == pts.len() {
            let (f, h) = pts[pts.len() - 1];
            return f as f64 + (target - h as f64) / ns_per_frame;
        } else {
            (pts[i - 1], pts[i])
        };
        if b.1 == a.1 {
            return a.0 as f64;
        }
        let t = (target - a.1 as f64) / (b.1 as f64 - a.1 as f64);
        a.0 as f64 + t * (b.0 - a.0) as f64
    }

    /// Clock drift of the device against the host clock, in parts per million, from a
    /// least-squares fit of the anchors. Positive: the device runs fast. `None` with
    /// fewer than three anchors.
    pub fn drift_ppm(&self) -> Option<f64> {
        if self.points.len() < 3 {
            return None;
        }
        let (f0, h0) = self.points[0];
        let xs: Vec<f64> = self
            .points
            .iter()
            .map(|&(_, h)| (h - h0) as f64 / 1e9)
            .collect();
        let ys: Vec<f64> = self.points.iter().map(|&(f, _)| (f - f0) as f64).collect();
        let n = xs.len() as f64;
        let mx = xs.iter().sum::<f64>() / n;
        let my = ys.iter().sum::<f64>() / n;
        let sxy: f64 = xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum();
        let sxx: f64 = xs.iter().map(|x| (x - mx) * (x - mx)).sum();
        if sxx == 0.0 {
            return None;
        }
        Some((sxy / sxx / f64::from(self.sample_rate) - 1.0) * 1e6)
    }

    /// Recording time of the first captured audio, after the start padding; `None` if
    /// the track never received audio.
    pub fn first_audio_ms(&self) -> Option<f64> {
        self.lines.iter().find_map(|line| match line {
            TimingLine::Segment { frame, .. } => Some(self.recording_ms(*frame as f64)),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(rate: u32) -> TimingLine {
        TimingLine::Start {
            version: TIMING_VERSION,
            source: "microphone".into(),
            sample_rate: rate,
            channels: 1,
            recording_start_ns: 1_000_000_000,
            unix_ms: 0,
        }
    }

    /// A track captured for 60 minutes by a device whose clock runs `ppm` off, starting
    /// `offset_s` after the recording, with an anchor every second of audio.
    fn drifting(rate: u32, ppm: f64, offset_s: f64) -> Timeline {
        let mut lines = vec![start(rate)];
        let actual = f64::from(rate) * (1.0 + ppm / 1e6);
        for k in 0..=3600u64 {
            let frame = k * u64::from(rate);
            let host = 1e9 + offset_s * 1e9 + frame as f64 / actual * 1e9;
            lines.push(TimingLine::Anchor {
                frame,
                host_ns: host as u64,
            });
        }
        Timeline::from_lines(lines).unwrap()
    }

    #[test]
    fn realigns_two_tracks_on_separate_clocks_over_an_hour() {
        // A headset 50 ppm fast against built-in audio 20 ppm slow, the system track
        // starting 4 s late. Played side by side at the nominal rates, the same instant
        // ends up 252 ms apart after an hour.
        let mic = drifting(48_000, 50.0, 0.0);
        let system = drifting(44_100, -20.0, 4.0);
        assert!((mic.drift_ppm().unwrap() - 50.0).abs() < 0.01);
        assert!((system.drift_ppm().unwrap() + 20.0).abs() < 0.01);
        let mut worst: f64 = 0.0;
        let mut naive_worst: f64 = 0.0;
        for second in (5..3600).step_by(7) {
            // Something both tracks heard, `second` s into the recording: where it lands
            // in each file, given each device's real clock.
            let t = f64::from(second);
            let mic_frame = t * 48_000.0 * (1.0 + 50e-6);
            let system_frame = (t - 4.0) * 44_100.0 * (1.0 - 20e-6);
            let error = mic.recording_ms(mic_frame) - system.recording_ms(system_frame);
            worst = worst.max(error.abs());
            let naive = mic_frame / 48.0 - (system_frame / 44.1 + 4_000.0);
            naive_worst = naive_worst.max(naive.abs());
        }
        assert!(worst < 0.1, "misaligned by {worst} ms after realignment");
        assert!(naive_worst > 240.0, "naive misalignment {naive_worst} ms");
    }

    #[test]
    fn maps_frames_both_ways() {
        let tl = drifting(16_000, 0.0, 2.5);
        assert!((tl.recording_ms(0.0) - 2_500.0).abs() < 1e-6);
        assert!((tl.recording_ms(16_000.0) - 3_500.0).abs() < 1e-6);
        assert!((tl.frame_at(3_500.0) - 16_000.0).abs() < 1e-3);
        // Outside the anchors, at the nominal rate.
        assert!((tl.frame_at(0.0) + 40_000.0).abs() < 1e-3);
    }

    #[test]
    fn needs_a_start_line() {
        let err = Timeline::from_lines(vec![TimingLine::Anchor {
            frame: 0,
            host_ns: 0,
        }]);
        assert!(err.is_err());
    }

    #[test]
    fn ignores_a_torn_last_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("me.timing.jsonl");
        let mut text = serde_json::to_string(&start(48_000)).unwrap();
        text.push('\n');
        text.push_str(r#"{"t":"anchor","frame":0,"host_ns":1000000000}"#);
        text.push('\n');
        text.push_str(r#"{"t":"anchor","fra"#);
        std::fs::write(&path, &text).unwrap();
        let tl = Timeline::load(&path).unwrap();
        assert_eq!(tl.lines().len(), 2);

        // A damaged line in the middle is an error, not silently skipped.
        text.push_str("\n{}\n");
        std::fs::write(&path, &text).unwrap();
        assert!(Timeline::load(&path).is_err());
    }
}
