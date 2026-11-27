//! Real-time side (lock-free ring buffers filled by audio callbacks) and
//! disk side (a writer thread per track: WAV + timing sidecar).

use crate::wav::WavWriter;
use anyhow::Result;
use rtrb::{Consumer, Producer, RingBuffer};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// One audio callback, as seen by the device.
#[derive(Clone, Copy, Default)]
pub struct Meta {
    /// Host time (mach_absolute_time in ns) of the first frame of the buffer.
    pub host_ns: u64,
    pub frames: u32,
    /// Device sample time (NaN if the backend does not expose it).
    pub sample_time: f64,
    /// Frames lost because the ring buffer was full.
    pub dropped: u32,
}

/// Owned by the audio callback. Never blocks, never allocates.
pub struct RtSide {
    samples: Producer<f32>,
    metas: Producer<Meta>,
    pub stats: Arc<Stats>,
}

#[derive(Default)]
pub struct Stats {
    pub callbacks: AtomicU64,
    pub frames: AtomicU64,
    pub dropped_frames: AtomicU64,
    pub lost_metas: AtomicU64,
}

impl RtSide {
    /// Push interleaved samples for one callback.
    pub fn push(&mut self, interleaved: &[f32], channels: usize, host_ns: u64, sample_time: f64) {
        let frames = interleaved.len() / channels;
        let mut dropped = 0;
        match self.samples.write_chunk_uninit(interleaved.len()) {
            Ok(chunk) => {
                chunk.fill_from_iter(interleaved.iter().copied());
            }
            Err(_) => dropped = frames as u32,
        }
        if self.metas.push(Meta { host_ns, frames: frames as u32, sample_time, dropped }).is_err() {
            self.stats.lost_metas.fetch_add(1, Ordering::Relaxed);
        }
        self.stats.callbacks.fetch_add(1, Ordering::Relaxed);
        self.stats.frames.fetch_add(frames as u64, Ordering::Relaxed);
        self.stats.dropped_frames.fetch_add(dropped as u64, Ordering::Relaxed);
    }
}

pub struct Track {
    pub name: &'static str,
    pub stats: Arc<Stats>,
    handle: Option<JoinHandle<Result<u64>>>,
}

impl Track {
    pub fn join(mut self) -> Result<u64> {
        self.handle.take().unwrap().join().expect("writer thread panicked")
    }
}

pub fn now_unix_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
}

/// Create the ring buffers and spawn the writer thread for a track.
pub fn track(
    name: &'static str,
    dir: &Path,
    channels: u16,
    rate: u32,
    stop: Arc<AtomicBool>,
    checkpoint_every: Duration,
) -> Result<(RtSide, Track)> {
    let (samples_p, samples_c) = RingBuffer::<f32>::new(rate as usize * channels as usize * 20);
    let (metas_p, metas_c) = RingBuffer::<Meta>::new(16_384);
    let stats = Arc::new(Stats::default());
    let wav = WavWriter::create(&dir.join(format!("{name}.wav")), channels, rate)?;
    let sidecar = BufWriter::new(File::create(dir.join(format!("{name}.timing.jsonl")))?);
    let handle = std::thread::Builder::new()
        .name(format!("writer-{name}"))
        .spawn(move || writer_loop(name, wav, sidecar, samples_c, metas_c, rate, stop, checkpoint_every))?;
    let rt = RtSide { samples: samples_p, metas: metas_p, stats: stats.clone() };
    Ok((rt, Track { name, stats, handle: Some(handle) }))
}

#[allow(clippy::too_many_arguments)]
fn writer_loop(
    name: &str,
    mut wav: WavWriter,
    mut sidecar: BufWriter<File>,
    mut samples: Consumer<f32>,
    mut metas: Consumer<Meta>,
    rate: u32,
    stop: Arc<AtomicBool>,
    checkpoint_every: Duration,
) -> Result<u64> {
    // Sidecar: one "anchor" line per ~1 s of device frames, plus every discontinuity.
    let anchor_every = rate as u64;
    let mut device_frames: u64 = 0; // frames delivered by the device so far
    let mut next_anchor: u64 = 0;
    let mut prev: Option<Meta> = None;
    let mut last_cp = Instant::now();
    loop {
        let stopping = stop.load(Ordering::Acquire);
        while let Ok(m) = metas.pop() {
            let mut kind = None;
            if let Some(p) = prev {
                let expected_st = p.sample_time + p.frames as f64;
                if !m.sample_time.is_nan() && (m.sample_time - expected_st).abs() > 0.5 {
                    kind = Some("sample_time_jump");
                }
                // Host-time gap larger than 3 buffers: the device stalled or we lost callbacks.
                let expected_ns = p.frames as f64 * 1e9 / rate as f64;
                if (m.host_ns as f64 - p.host_ns as f64) > expected_ns * 3.0 + 5e6 {
                    kind = Some("host_gap");
                }
            }
            if m.dropped > 0 {
                kind = Some("dropped");
            }
            if kind.is_some() || device_frames >= next_anchor {
                writeln!(
                    sidecar,
                    r#"{{"t":"{}","host_ns":{},"frame":{},"sample_time":{},"frames":{},"dropped":{},"written":{},"unix_ms":{}}}"#,
                    kind.unwrap_or("anchor"),
                    m.host_ns,
                    device_frames,
                    if m.sample_time.is_nan() { "null".to_string() } else { format!("{:.1}", m.sample_time) },
                    m.frames,
                    m.dropped,
                    wav.frames(),
                    now_unix_ms()
                )?;
                if device_frames >= next_anchor {
                    next_anchor = device_frames + anchor_every;
                }
            }
            device_frames += m.frames as u64;
            prev = Some(m);
        }
        let n = samples.slots();
        if n > 0 {
            let chunk = samples.read_chunk(n).unwrap();
            let (a, b) = chunk.as_slices();
            wav.write(a)?;
            wav.write(b)?;
            chunk.commit_all();
        }
        if stopping || last_cp.elapsed() >= checkpoint_every {
            wav.checkpoint()?;
            sidecar.flush()?;
            sidecar.get_ref().sync_data()?;
            last_cp = Instant::now();
        }
        if stopping && metas.is_empty() && samples.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = name;
    Ok(wav.frames())
}
