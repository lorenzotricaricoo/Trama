//! The device side: what a platform backend implements, and the real-time sink its
//! audio callback writes into.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;

use rtrb::{Consumer, Producer, RingBuffer};

use super::{CaptureError, DeviceInfo, PermissionState, Source, StreamFormat};

/// Samples a ring buffer holds: 2^21 `f32`, about 20 s of 48 kHz stereo. The writer
/// drains it every 20 ms, so it only fills up if the disk stalls for seconds.
const SAMPLE_CAPACITY: usize = 1 << 21;
/// Callbacks a ring buffer holds; at 10 ms per callback, almost three minutes.
const META_CAPACITY: usize = 1 << 14;

/// A platform's audio devices. Implemented for macOS by [`super::macos::MacBackend`]
/// and by fakes in tests.
///
/// Streams are opened, used and dropped on the same thread, so they need not be `Send`.
pub trait CaptureBackend: Send + Sync {
    /// Current host time in nanoseconds, on the clock of the buffers' timestamps
    /// (`mach_absolute_time` on macOS). Monotonic.
    fn now_ns(&self) -> u64;

    /// Microphone permission, without asking.
    fn microphone_permission(&self) -> PermissionState;

    /// Asks the user for microphone access if not asked yet; blocks until they answer.
    fn request_microphone_permission(&self) -> PermissionState;

    /// Checks system audio access. macOS has no API for it, so a backend may play an
    /// inaudible probe and check that it is captured; this can take about a second.
    fn probe_system_audio_permission(&self) -> PermissionState {
        PermissionState::Unknown
    }

    /// The device a stream on `source` would use now: the default input for the
    /// microphone, the default output for system audio. `None` if there is none.
    fn default_device(&self, source: Source) -> Result<Option<DeviceInfo>, CaptureError>;

    /// Opens and starts a stream. `device` pins a device by [`DeviceInfo::id`]; `None`
    /// uses the default. Every buffer goes to `sink`; stream errors go to `faults`.
    /// Dropping the returned stream stops it.
    fn open(
        &self,
        source: Source,
        device: Option<&str>,
        sink: FrameSink,
        faults: FaultReporter,
    ) -> Result<Box<dyn ActiveStream>, CaptureError>;
}

/// A running stream. Dropping it stops the stream and releases the device.
pub trait ActiveStream {
    /// Format of the buffers pushed to the sink.
    fn format(&self) -> StreamFormat;
    /// The device being captured.
    fn device(&self) -> DeviceInfo;
    /// Name of the capture method, for the sidecar (for example `"cpal"`).
    fn backend(&self) -> &'static str;
}

/// Reports a stream failure from any thread. The supervisor closes the stream and
/// opens a new one.
#[derive(Debug, Clone)]
pub struct FaultReporter {
    generation: u64,
    tx: Sender<Fault>,
}

#[derive(Debug)]
pub(crate) struct Fault {
    pub generation: u64,
    pub code: &'static str,
    pub detail: String,
}

impl FaultReporter {
    pub(crate) fn new(generation: u64, tx: Sender<Fault>) -> Self {
        Self { generation, tx }
    }

    /// Reports that the stream failed. `code` is one of [`super::error_code`].
    pub fn report(&self, code: &'static str, detail: impl ToString) {
        // The supervisor may already be gone at the end of a recording.
        let _ = self.tx.send(Fault {
            generation: self.generation,
            code,
            detail: detail.to_string(),
        });
    }
}

/// One audio callback, as seen by the writer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Meta {
    /// Host time of the first frame of the buffer, in nanoseconds.
    pub host_ns: u64,
    pub frames: u32,
    /// Whether the samples were lost because the ring buffer was full.
    pub dropped: bool,
}

/// Counters shared by the audio callback and the supervisor.
#[derive(Debug, Default)]
pub(crate) struct SinkStats {
    pub callbacks: AtomicU64,
    pub dropped_frames: AtomicU64,
}

/// Where an audio callback puts its buffers. Real-time safe: never blocks, never
/// allocates.
pub struct FrameSink {
    samples: Producer<f32>,
    metas: Producer<Meta>,
    stats: Arc<SinkStats>,
}

/// The writer's end of a [`FrameSink`].
pub(crate) struct FrameSource {
    pub samples: Consumer<f32>,
    pub metas: Consumer<Meta>,
}

pub(crate) fn frame_channel() -> (FrameSink, FrameSource, Arc<SinkStats>) {
    let (samples, samples_rx) = RingBuffer::new(SAMPLE_CAPACITY);
    let (metas, metas_rx) = RingBuffer::new(META_CAPACITY);
    let stats = Arc::new(SinkStats::default());
    let sink = FrameSink {
        samples,
        metas,
        stats: stats.clone(),
    };
    let source = FrameSource {
        samples: samples_rx,
        metas: metas_rx,
    };
    (sink, source, stats)
}

impl FrameSink {
    /// Pushes one buffer of interleaved samples with `channels` channels, whose first
    /// frame was captured at `host_ns` (see [`CaptureBackend::now_ns`]). `channels`
    /// must match the stream's [`ActiveStream::format`].
    pub fn push(&mut self, interleaved: &[f32], channels: usize, host_ns: u64) {
        let channels = channels.max(1);
        let frames = interleaved.len() / channels;
        if frames == 0 {
            return;
        }
        let samples = &interleaved[..frames * channels];
        // Keep the two rings in step: only write samples if their meta fits too.
        let dropped = if self.metas.slots() == 0 {
            true
        } else {
            match self.samples.write_chunk_uninit(samples.len()) {
                Ok(chunk) => {
                    chunk.fill_from_iter(samples.iter().copied());
                    false
                }
                Err(_) => true,
            }
        };
        let meta = Meta {
            host_ns,
            frames: frames as u32,
            dropped,
        };
        // Fails only when the meta ring is full, in which case `dropped` is already
        // set and nothing was written: the writer sees a host-time gap and pads it.
        let _ = self.metas.push(meta);
        self.stats.callbacks.fetch_add(1, Ordering::Relaxed);
        if dropped {
            self.stats
                .dropped_frames
                .fetch_add(frames as u64, Ordering::Relaxed);
        }
    }
}
