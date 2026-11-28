//! The writer thread of one track: drains the ring buffers of the current stream into
//! the WAV and the timing sidecar.
//!
//! The file is kept on the recording's time line: silence fills the time before the
//! first audio, the gaps between streams and inside a stream, audio lost in an overflow,
//! and the time after the last audio up to the stop. Every fill is recorded in the
//! sidecar with an anchor on each side, so the mapping to host time stays exact.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use super::backend::{FrameSource, Meta};
use super::convert::Converter;
use super::timing::{TIMING_VERSION, TimingLine};
use super::wav::WavWriter;
use super::{CaptureEvent, CaptureObserver, DeviceInfo, Level, Source, StreamFormat, error_code};

/// A jump in host time larger than this between two buffers is a gap, filled with
/// silence. Callback jitter is well under a millisecond.
const GAP_NS: f64 = 50e6;
/// Levels are sent for every 50 ms of audio: 20 a second.
const LEVELS_PER_SECOND: u64 = 20;
/// At most one "frames dropped" event per this many seconds, to avoid a flood.
const DROPPED_EVENT_EVERY: Duration = Duration::from_secs(10);

/// Messages from the supervisor, in order with the streams they describe.
pub(crate) enum WriterMsg {
    /// A new stream was opened; the previous one is already closed.
    Segment {
        frames: FrameSource,
        format: StreamFormat,
        device: DeviceInfo,
        backend: &'static str,
    },
    /// The stream was lost at `host_ns`.
    Interrupted { host_ns: u64, code: &'static str },
    /// The recording stopped at `stop_ns`; finish the files.
    Finish { stop_ns: u64 },
}

pub(crate) struct WriterConfig {
    pub source: Source,
    pub wav_path: PathBuf,
    pub timing_path: PathBuf,
    /// Channels of the track; streams with more are mixed down.
    pub max_channels: u16,
    pub recording_start_ns: u64,
    pub unix_ms: i64,
    pub observer: Arc<dyn CaptureObserver>,
    pub pass: Duration,
    pub checkpoint_every: Duration,
    pub silent_after: Duration,
    pub max_data_bytes: u64,
}

/// What the writer did, for the recording summary.
#[derive(Debug, Clone, Default)]
pub(crate) struct WriterOutcome {
    pub format: Option<StreamFormat>,
    pub frames: u64,
    pub padded_frames: u64,
    pub dropped_frames: u64,
    pub segments: u32,
    pub interruptions: u32,
    pub first_audio_ms: Option<u64>,
    pub device: Option<String>,
    pub problems: Vec<&'static str>,
}

struct Segment {
    frames: FrameSource,
    format: StreamFormat,
    device: DeviceInfo,
    backend: &'static str,
    converter: Converter,
    started: bool,
    /// Host time just after the last buffer of this stream.
    end_ns: f64,
}

struct Files {
    wav: WavWriter,
    timing: BufWriter<File>,
}

struct TrackWriter {
    cfg: WriterConfig,
    files: Option<Files>,
    segment: Option<Segment>,
    /// Host time just after the last buffer written, across streams.
    end_ns: Option<f64>,
    next_anchor: u64,
    /// Host time of the last anchor: anchors never go back in time, even when a new
    /// stream's first buffer overlaps the end of the previous one.
    last_anchor_ns: f64,
    out: Vec<f32>,
    /// One buffer made contiguous: the ring's wrap point is not frame-aligned for every
    /// channel count, so its two halves cannot be converted separately.
    contiguous: Vec<f32>,
    // Levels.
    level_peak: f32,
    level_sum: f64,
    level_frames: u64,
    // Silence since the start.
    audio_frames: u64,
    heard_something: bool,
    silent_warned: bool,
    // Failures: once set, audio is drained and discarded.
    stopped_writing: bool,
    last_dropped_event: Option<Instant>,
    last_checkpoint: Instant,
    outcome: WriterOutcome,
}

/// Runs the writer until [`WriterMsg::Finish`].
pub(crate) fn run(cfg: WriterConfig, rx: Receiver<WriterMsg>) -> WriterOutcome {
    let pass = cfg.pass;
    let mut w = TrackWriter {
        cfg,
        files: None,
        segment: None,
        end_ns: None,
        next_anchor: 0,
        last_anchor_ns: 0.0,
        out: Vec::with_capacity(1 << 15),
        contiguous: Vec::with_capacity(1 << 15),
        level_peak: 0.0,
        level_sum: 0.0,
        level_frames: 0,
        audio_frames: 0,
        heard_something: false,
        silent_warned: false,
        stopped_writing: false,
        last_dropped_event: None,
        last_checkpoint: Instant::now(),
        outcome: WriterOutcome::default(),
    };
    loop {
        match rx.recv_timeout(pass) {
            Ok(WriterMsg::Segment {
                frames,
                format,
                device,
                backend,
            }) => {
                w.drain();
                w.begin_segment(frames, format, device, backend);
            }
            Ok(WriterMsg::Interrupted { host_ns, code }) => {
                w.drain();
                w.segment = None;
                w.outcome.interruptions += 1;
                w.line(&TimingLine::Interrupted {
                    host_ns,
                    code: code.to_owned(),
                });
            }
            Ok(WriterMsg::Finish { stop_ns }) => {
                w.drain();
                w.finish(Some(stop_ns));
                return w.outcome;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                // The recorder went away without stopping: finish at the last audio.
                w.drain();
                w.finish(None);
                return w.outcome;
            }
        }
        w.drain();
        if w.last_checkpoint.elapsed() >= w.cfg.checkpoint_every {
            w.checkpoint();
        }
    }
}

impl TrackWriter {
    fn rate(&self) -> f64 {
        self.files
            .as_ref()
            .map_or(1.0, |f| f64::from(f.wav.format().sample_rate))
    }

    fn file_frames(&self) -> u64 {
        self.files.as_ref().map_or(0, |f| f.wav.frames())
    }

    fn begin_segment(
        &mut self,
        frames: FrameSource,
        format: StreamFormat,
        device: DeviceInfo,
        backend: &'static str,
    ) {
        if self.files.is_none() && !self.stopped_writing {
            let track = StreamFormat {
                sample_rate: format.sample_rate,
                channels: format.channels.clamp(1, self.cfg.max_channels.max(1)),
            };
            match self.create_files(track) {
                Ok(files) => {
                    self.files = Some(files);
                    self.outcome.format = Some(track);
                    self.line(&TimingLine::Start {
                        version: TIMING_VERSION,
                        source: self.cfg.source.as_str().to_owned(),
                        sample_rate: track.sample_rate,
                        channels: track.channels,
                        recording_start_ns: self.cfg.recording_start_ns,
                        unix_ms: self.cfg.unix_ms,
                    });
                }
                Err(err) => self.fail(error_code::WRITE_FAILED, &err),
            }
        }
        if self.outcome.device.is_none() {
            self.outcome.device = Some(device.name.clone());
        }
        let track = self.outcome.format.unwrap_or(format);
        self.outcome.segments += 1;
        self.segment = Some(Segment {
            frames,
            format,
            device,
            backend,
            converter: Converter::new(format, track),
            started: false,
            end_ns: 0.0,
        });
    }

    fn create_files(&self, format: StreamFormat) -> io::Result<Files> {
        let wav =
            WavWriter::create_with_limit(&self.cfg.wav_path, format, self.cfg.max_data_bytes)?;
        let timing = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.cfg.timing_path)?;
        Ok(Files {
            wav,
            timing: BufWriter::new(timing),
        })
    }

    /// Processes every buffer of the current stream that has arrived.
    fn drain(&mut self) {
        let Some(mut seg) = self.segment.take() else {
            return;
        };
        let in_ch = usize::from(seg.format.channels.max(1));
        while let Ok(meta) = seg.frames.metas.pop() {
            let wanted = meta.frames as usize * in_ch;
            if !meta.dropped && seg.frames.samples.slots() < wanted {
                // Cannot happen: samples are pushed before their meta.
                self.problem(error_code::INTERNAL, "samples missing for a buffer");
                break;
            }
            self.buffer(&mut seg, meta);
        }
        self.segment = Some(seg);
        self.flush_timing();
    }

    fn buffer(&mut self, seg: &mut Segment, meta: Meta) {
        let rate = self.rate();
        let host = meta.host_ns as f64;
        if !seg.started {
            seg.started = true;
            match self.end_ns {
                None => {
                    // The first audio of the track.
                    let start = self.cfg.recording_start_ns as f64;
                    let offset_ms = ((host - start) / 1e6).max(0.0) as u64;
                    if host > start {
                        let n = ((host - start) * rate / 1e9).round() as u64;
                        self.pad("start", n, start, host);
                    }
                    self.outcome.first_audio_ms = Some(offset_ms);
                    self.cfg.observer.event(CaptureEvent::TrackStarted {
                        source: self.cfg.source,
                        offset_ms,
                    });
                }
                Some(end) if host > end => {
                    let n = ((host - end) * rate / 1e9).round() as u64;
                    self.pad("gap", n, end, host);
                }
                Some(_) => {}
            }
            let frame = self.file_frames();
            self.line(&TimingLine::Segment {
                frame,
                device_id: seg.device.id.clone(),
                device_name: seg.device.name.clone(),
                backend: seg.backend.to_owned(),
                sample_rate: seg.format.sample_rate,
                channels: seg.format.channels,
            });
            self.anchor(host);
        } else if host - seg.end_ns > GAP_NS {
            let n = ((host - seg.end_ns) * rate / 1e9).round() as u64;
            self.pad("gap", n, seg.end_ns, host);
        }
        if self.file_frames() >= self.next_anchor {
            self.anchor(host);
        }

        let duration = f64::from(meta.frames) * 1e9 / f64::from(seg.format.sample_rate);
        if meta.dropped {
            let n = seg.converter.output_frames(u64::from(meta.frames));
            self.outcome.dropped_frames += n;
            self.pad("dropped", n, host, host + duration);
            let quiet = self
                .last_dropped_event
                .is_some_and(|t| t.elapsed() < DROPPED_EVENT_EVERY);
            if !quiet {
                self.last_dropped_event = Some(Instant::now());
                self.problem(
                    error_code::FRAMES_DROPPED,
                    format!("{} frames", meta.frames),
                );
            }
        } else {
            let wanted = meta.frames as usize * usize::from(seg.format.channels.max(1));
            let chunk = seg
                .frames
                .samples
                .read_chunk(wanted)
                .expect("checked by drain");
            let (a, b) = chunk.as_slices();
            self.contiguous.clear();
            self.contiguous.extend_from_slice(a);
            self.contiguous.extend_from_slice(b);
            chunk.commit_all();
            self.out.clear();
            seg.converter.process(&self.contiguous, &mut self.out);
            let out = std::mem::take(&mut self.out);
            self.audio(&out);
            self.out = out;
        }
        seg.end_ns = host + duration;
        self.end_ns = Some(seg.end_ns);
    }

    /// Writes converted audio and updates levels and the silence check.
    fn audio(&mut self, samples: &[f32]) {
        let Some(format) = self.outcome.format else {
            return;
        };
        let channels = usize::from(format.channels);
        let frames = (samples.len() / channels) as u64;
        self.write(|wav| wav.write(samples));

        for s in samples {
            let a = s.abs();
            self.level_peak = self.level_peak.max(a);
            self.level_sum += f64::from(*s) * f64::from(*s);
            if *s != 0.0 {
                self.heard_something = true;
            }
        }
        self.level_frames += frames;
        if self.level_frames >= u64::from(format.sample_rate) / LEVELS_PER_SECOND {
            let count = (self.level_frames * channels as u64) as f64;
            self.cfg.observer.level(Level {
                source: self.cfg.source,
                peak: self.level_peak,
                rms: (self.level_sum / count).sqrt() as f32,
            });
            self.level_peak = 0.0;
            self.level_sum = 0.0;
            self.level_frames = 0;
        }

        self.audio_frames += frames;
        let silent_frames = self.cfg.silent_after.as_secs_f64() * f64::from(format.sample_rate);
        if !self.heard_something && !self.silent_warned && self.audio_frames as f64 >= silent_frames
        {
            self.silent_warned = true;
            self.problem(
                self.cfg.source.silent_code(),
                format!("{} s of digital silence", self.cfg.silent_after.as_secs()),
            );
        }
    }

    /// Writes `frames` of silence from host time `from` to `to`, with an anchor on each side.
    fn pad(&mut self, reason: &str, frames: u64, from: f64, to: f64) {
        if frames == 0 {
            return;
        }
        let frame = self.file_frames();
        self.anchor_at(frame, from);
        self.write(|wav| wav.write_silence(frames));
        self.outcome.padded_frames += frames;
        self.line(&TimingLine::Pad {
            frame,
            frames,
            reason: reason.to_owned(),
        });
        self.anchor_at(self.file_frames(), to);
    }

    fn anchor(&mut self, host: f64) {
        self.anchor_at(self.file_frames(), host);
    }

    fn anchor_at(&mut self, frame: u64, host: f64) {
        let host = host.max(self.last_anchor_ns);
        self.last_anchor_ns = host;
        self.line(&TimingLine::Anchor {
            frame,
            host_ns: host.max(0.0).round() as u64,
        });
        self.next_anchor = frame + self.rate() as u64;
    }

    fn finish(&mut self, stop_ns: Option<u64>) {
        let start = self.cfg.recording_start_ns as f64;
        let rate = self.rate();
        let end = match (self.end_ns, stop_ns) {
            (Some(end), Some(stop)) if stop as f64 - end > GAP_NS => {
                let n = ((stop as f64 - end) * rate / 1e9).round() as u64;
                self.pad("end", n, end, stop as f64);
                stop as f64
            }
            (Some(end), _) => end,
            (None, Some(stop)) => {
                // No audio at all (system audio when nothing played): silence throughout.
                let n = ((stop as f64 - start).max(0.0) * rate / 1e9).round() as u64;
                self.pad("start", n, start, stop as f64);
                stop as f64
            }
            (None, None) => start,
        };
        if stop_ns.is_some() {
            let unix_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as i64);
            self.line(&TimingLine::Stop {
                frame: self.file_frames(),
                host_ns: end.max(self.last_anchor_ns).round() as u64,
                unix_ms,
            });
        }
        self.checkpoint();
        self.outcome.frames = self.file_frames();
    }

    fn line(&mut self, line: &TimingLine) {
        let Some(files) = self.files.as_mut() else {
            return;
        };
        if self.stopped_writing {
            return;
        }
        let result = serde_json::to_writer(&mut files.timing, line)
            .map_err(io::Error::other)
            .and_then(|()| files.timing.write_all(b"\n"));
        if let Err(err) = result {
            self.fail(error_code::WRITE_FAILED, &err);
        }
    }

    fn flush_timing(&mut self) {
        if self.stopped_writing {
            return;
        }
        if let Some(files) = self.files.as_mut()
            && let Err(err) = files.timing.flush()
        {
            self.fail(error_code::WRITE_FAILED, &err);
        }
    }

    fn write(&mut self, op: impl FnOnce(&mut WavWriter) -> io::Result<()>) {
        if self.stopped_writing {
            return;
        }
        let Some(files) = self.files.as_mut() else {
            return;
        };
        if let Err(err) = op(&mut files.wav) {
            let code = if err.kind() == io::ErrorKind::FileTooLarge {
                error_code::TRACK_SIZE_LIMIT
            } else {
                error_code::WRITE_FAILED
            };
            self.fail(code, &err);
        }
    }

    fn checkpoint(&mut self) {
        self.last_checkpoint = Instant::now();
        let Some(files) = self.files.as_mut() else {
            return;
        };
        // After a failure, still try to make what was written readable.
        let result = files
            .timing
            .flush()
            .and_then(|()| files.timing.get_ref().sync_data())
            .and_then(|()| files.wav.checkpoint());
        if let Err(err) = result
            && !self.stopped_writing
        {
            self.fail(error_code::WRITE_FAILED, &err);
        }
    }

    /// Stops writing this track for good and reports why.
    fn fail(&mut self, code: &'static str, err: &dyn std::fmt::Display) {
        if self.stopped_writing {
            return;
        }
        self.stopped_writing = true;
        self.problem(code, err.to_string());
    }

    fn problem(&mut self, code: &'static str, detail: impl ToString) {
        if !self.outcome.problems.contains(&code) {
            self.outcome.problems.push(code);
        }
        self.cfg.observer.event(CaptureEvent::TrackProblem {
            source: self.cfg.source,
            code,
            detail: detail.to_string(),
        });
    }
}
