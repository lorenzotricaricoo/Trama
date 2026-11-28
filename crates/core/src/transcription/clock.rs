//! Placing a track's frames on the meeting's time line.
//!
//! The recorder writes a timing sidecar next to each track; its [`Timeline`] maps a frame
//! of the file to recording time through the sidecar's anchors, which removes start
//! offsets, gaps and clock drift (`docs/architecture/audio-capture.md`). A track without
//! a usable sidecar is placed at its nominal rate instead ([`NominalClock`]).

use crate::capture::Timeline;

/// Maps a frame of a track (at the file's sample rate) to recording time.
pub trait TrackClock {
    /// Milliseconds since the recording started.
    fn recording_ms(&self, frame: f64) -> f64;
}

impl TrackClock for Timeline {
    fn recording_ms(&self, frame: f64) -> f64 {
        Timeline::recording_ms(self, frame)
    }
}

/// A track that starts with the recording and runs at its nominal rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NominalClock {
    sample_rate: u32,
}

impl NominalClock {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate: sample_rate.max(1),
        }
    }
}

impl TrackClock for NominalClock {
    fn recording_ms(&self, frame: f64) -> f64 {
        frame * 1000.0 / f64::from(self.sample_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::TimingLine;

    #[test]
    fn the_nominal_clock_counts_frames() {
        assert_eq!(NominalClock::new(48_000).recording_ms(48_000.0), 1_000.0);
    }

    #[test]
    fn the_recorder_timeline_places_a_late_track() {
        let timeline = Timeline::from_lines(vec![
            TimingLine::Start {
                version: 1,
                source: "system".into(),
                sample_rate: 48_000,
                channels: 1,
                recording_start_ns: 1_000_000_000,
                unix_ms: 0,
            },
            TimingLine::Anchor {
                frame: 0,
                host_ns: 3_500_000_000,
            },
        ])
        .unwrap();
        let clock: &dyn TrackClock = &timeline;
        assert!((clock.recording_ms(48_000.0) - 3_500.0).abs() < 1e-6);
    }
}
