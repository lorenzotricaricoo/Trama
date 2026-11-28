//! Capture tests, headless: fake devices on a virtual clock, the real recorder, writer
//! threads and files on disk.
//!
//! The fake devices put a click on the host clock every whole second, so a test can
//! check where the same instant lands in each track after realignment.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use trama_core::capture::*;
use trama_core::storage::{Library, Role};

/// Virtual time runs this many times faster than the wall clock.
const SPEED: f64 = 10.0;
const PATIENCE: Duration = Duration::from_secs(10);
const CLICK_PERIOD_NS: u64 = 1_000_000_000;

/// The virtual host clock shared by the fake devices and the recorder.
#[derive(Clone)]
struct Clock {
    base: Instant,
    speed: f64,
}

impl Clock {
    /// Starts at 100 s, like a host clock that has been running for a while.
    fn now_ns(&self) -> u64 {
        100_000_000_000 + (self.base.elapsed().as_secs_f64() * self.speed * 1e9) as u64
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Signal {
    /// Low noise with a click every whole second of host time.
    Clicks,
    /// Digital silence, as delivered when access is denied.
    Zeros,
}

#[derive(Clone)]
struct FakeDevice {
    info: DeviceInfo,
    format: StreamFormat,
    /// How far the device's clock is off the host clock.
    ppm: f64,
    signal: Signal,
    /// The first stream delivers its first buffer this long after opening (system audio
    /// starts at the first sound).
    first_sound_after: Duration,
    /// The first stream stops delivering after this many buffers.
    stall_after: Option<u64>,
}

impl FakeDevice {
    fn new(id: &str, sample_rate: u32, channels: u16) -> Self {
        Self {
            info: DeviceInfo {
                id: id.into(),
                name: format!("Fake {id}"),
            },
            format: StreamFormat {
                sample_rate,
                channels,
            },
            ppm: 0.0,
            signal: Signal::Clicks,
            first_sound_after: Duration::ZERO,
            stall_after: None,
        }
    }
}

struct FakeState {
    devices: HashMap<String, FakeDevice>,
    defaults: HashMap<Source, String>,
    microphone: PermissionState,
    answer: PermissionState,
    fail_open: HashMap<Source, &'static str>,
    faults: HashMap<Source, FaultReporter>,
    opened: Vec<String>,
}

struct FakeBackend {
    clock: Clock,
    state: Mutex<FakeState>,
    requests: AtomicU32,
}

impl FakeBackend {
    fn new() -> Arc<Self> {
        Self::with_speed(SPEED)
    }

    fn with_speed(speed: f64) -> Arc<Self> {
        let mut devices = HashMap::new();
        let mut defaults = HashMap::new();
        for (source, device) in [
            (
                Source::Microphone,
                FakeDevice::new("built-in-mic", 48_000, 1),
            ),
            (Source::System, FakeDevice::new("speakers", 48_000, 2)),
        ] {
            defaults.insert(source, device.info.id.clone());
            devices.insert(device.info.id.clone(), device);
        }
        Arc::new(Self {
            clock: Clock {
                base: Instant::now(),
                speed,
            },
            state: Mutex::new(FakeState {
                devices,
                defaults,
                microphone: PermissionState::Granted,
                answer: PermissionState::Granted,
                fail_open: HashMap::new(),
                faults: HashMap::new(),
                opened: Vec::new(),
            }),
            requests: AtomicU32::new(0),
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().unwrap()
    }

    /// Changes the default device of `source`, adding it if new.
    fn set_default(&self, source: Source, device: FakeDevice) {
        let mut state = self.state();
        state.defaults.insert(source, device.info.id.clone());
        state.devices.insert(device.info.id.clone(), device);
    }

    fn edit(&self, source: Source, f: impl FnOnce(&mut FakeDevice)) {
        let mut state = self.state();
        let id = state.defaults[&source].clone();
        f(state.devices.get_mut(&id).unwrap());
    }

    fn fault(&self, source: Source, code: &'static str) {
        self.state().faults[&source].report(code, "injected");
    }
}

struct FakeStream {
    device: FakeDevice,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ActiveStream for FakeStream {
    fn format(&self) -> StreamFormat {
        self.device.format
    }
    fn device(&self) -> DeviceInfo {
        self.device.info.clone()
    }
    fn backend(&self) -> &'static str {
        "fake"
    }
}

impl Drop for FakeStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

/// Delivers the device's audio in 10 ms buffers, paced by the virtual clock.
fn generate(clock: Clock, device: FakeDevice, mut sink: FrameSink, stop: Arc<AtomicBool>) {
    let format = device.format;
    let channels = usize::from(format.channels);
    let frames = (format.sample_rate / 100) as u64;
    let rate = f64::from(format.sample_rate) * (1.0 + device.ppm / 1e6);
    let first_ns = clock.now_ns() as f64 + device.first_sound_after.as_secs_f64() * 1e9;
    let host = |n: u64| first_ns + n as f64 * 1e9 / rate;
    let mut buffer = Vec::with_capacity(frames as usize * channels);
    let mut k = 0u64;
    while !stop.load(Ordering::Acquire) {
        let start = k * frames;
        let end_ns = host(start + frames);
        loop {
            if stop.load(Ordering::Acquire) {
                return;
            }
            let ahead_ns = end_ns - clock.now_ns() as f64;
            if ahead_ns <= 0.0 {
                break;
            }
            // Sleep until the buffer is due, in wall time, at most 2 ms at a time.
            let wall = (ahead_ns / clock.speed / 1e9).min(0.002);
            std::thread::sleep(Duration::from_secs_f64(wall));
        }
        if device.stall_after.is_some_and(|n| k >= n) {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        buffer.clear();
        for n in start..start + frames {
            let value = match device.signal {
                Signal::Zeros => 0.0,
                Signal::Clicks => {
                    let before = if n == 0 { 0.0 } else { host(n - 1) };
                    let period = CLICK_PERIOD_NS as f64;
                    if n > 0 && (host(n) / period).floor() > (before / period).floor() {
                        0.9
                    } else {
                        0.01 * ((n % 100) as f32 / 100.0 - 0.5)
                    }
                }
            };
            buffer.extend(std::iter::repeat_n(value, channels));
        }
        sink.push(&buffer, channels, host(start) as u64);
        k += 1;
    }
}

impl CaptureBackend for FakeBackend {
    fn now_ns(&self) -> u64 {
        self.clock.now_ns()
    }

    fn microphone_permission(&self) -> PermissionState {
        self.state().microphone
    }

    fn request_microphone_permission(&self) -> PermissionState {
        self.requests.fetch_add(1, Ordering::Relaxed);
        let mut state = self.state();
        state.microphone = state.answer;
        state.answer
    }

    fn default_device(&self, source: Source) -> Result<Option<DeviceInfo>> {
        let state = self.state();
        Ok(state
            .defaults
            .get(&source)
            .map(|id| state.devices[id].info.clone()))
    }

    fn open(
        &self,
        source: Source,
        device: Option<&str>,
        sink: FrameSink,
        faults: FaultReporter,
    ) -> Result<Box<dyn ActiveStream>> {
        let mut state = self.state();
        if let Some(code) = state.fail_open.get(&source) {
            return Err(CaptureError::new(code, "fake failure"));
        }
        let id = device.map_or_else(|| state.defaults[&source].clone(), str::to_owned);
        let Some(fake) = state.devices.get_mut(&id) else {
            return Err(CaptureError::new(error_code::DEVICE_UNAVAILABLE, id));
        };
        let device = fake.clone();
        // Only the first stream of a device starts late or stalls.
        fake.first_sound_after = Duration::ZERO;
        fake.stall_after = None;
        state.faults.insert(source, faults);
        state.opened.push(id);
        drop(state);
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (clock, device, stop) = (self.clock.clone(), device.clone(), stop.clone());
            std::thread::spawn(move || generate(clock, device, sink, stop))
        };
        Ok(Box::new(FakeStream {
            device,
            stop,
            thread: Some(thread),
        }))
    }
}

#[derive(Default)]
struct Collector {
    events: Mutex<Vec<CaptureEvent>>,
    levels: Mutex<Vec<Level>>,
}

impl CaptureObserver for Collector {
    fn level(&self, level: Level) {
        self.levels.lock().unwrap().push(level);
    }
    fn event(&self, event: CaptureEvent) {
        self.events.lock().unwrap().push(event);
    }
}

impl Collector {
    fn events(&self) -> Vec<CaptureEvent> {
        self.events.lock().unwrap().clone()
    }

    fn wait_for(&self, what: &str, test: impl Fn(&CaptureEvent) -> bool) -> CaptureEvent {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(event) = self.events().into_iter().find(&test) {
                return event;
            }
            assert!(
                Instant::now() < deadline,
                "no {what} event: {:?}",
                self.events()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn problems(&self, source: Source) -> Vec<&'static str> {
        self.events()
            .into_iter()
            .filter_map(|e| match e {
                CaptureEvent::TrackProblem {
                    source: s, code, ..
                } if s == source => Some(code),
                _ => None,
            })
            .collect()
    }
}

/// Problems of a track, without overflows: the virtual clock runs ten times faster than
/// real time, so a heavily loaded machine can starve the writer long enough to overflow
/// the ring buffer. Overflows are padded on the time line, so the other checks still hold.
fn problems(track: &TrackSummary) -> Vec<&'static str> {
    let mut problems = track.problems.clone();
    problems.retain(|&p| p != error_code::FRAMES_DROPPED);
    problems
}

fn tuning() -> Tuning {
    Tuning {
        device_poll: Duration::from_millis(10),
        stall_timeout: Duration::from_millis(150),
        retry_every: Duration::from_millis(20),
        writer_pass: Duration::from_millis(5),
        checkpoint_every: Duration::from_millis(50),
        silent_after: Duration::from_secs(1),
    }
}

struct Setup {
    dir: TempDir,
    backend: Arc<FakeBackend>,
    observer: Arc<Collector>,
    options: RecordingOptions,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let meeting = uuid::Uuid::now_v7();
    let mut options = RecordingOptions::new(dir.path(), meeting);
    options.tuning = tuning();
    Setup {
        dir,
        backend: FakeBackend::new(),
        observer: Arc::new(Collector::default()),
        options,
    }
}

impl Setup {
    fn start(&self) -> Result<Recording> {
        Recording::start(
            self.backend.clone(),
            self.options.clone(),
            self.observer.clone(),
        )
    }

    /// Lets `virtual_ms` of virtual time pass.
    fn run_for(&self, virtual_ms: u64) {
        std::thread::sleep(Duration::from_secs_f64(virtual_ms as f64 / 1e3 / SPEED));
    }

    fn path(&self, relative: &Path) -> std::path::PathBuf {
        self.dir.path().join(relative)
    }

    fn audio_dir(&self) -> std::path::PathBuf {
        self.dir
            .path()
            .join(Library::meeting_audio_dir(self.options.meeting_id))
    }
}

/// Recording times (ms) of the clicks in a track, through its time line.
fn clicks_ms(setup: &Setup, track: &TrackSummary) -> Vec<f64> {
    let (format, samples) = wav::read(&setup.path(&track.audio_path)).unwrap();
    let timeline = Timeline::load(&setup.path(&track.timing_path)).unwrap();
    let channels = usize::from(format.channels);
    let mut clicks = Vec::new();
    let mut last: Option<usize> = None;
    for (i, frame) in samples.chunks_exact(channels).enumerate() {
        if frame[0] > 0.3 && last.is_none_or(|l| i - l > format.sample_rate as usize / 2) {
            clicks.push(timeline.recording_ms(i as f64));
            last = Some(i);
        }
    }
    clicks
}

/// Every expected click is found in `clicks` within `tolerance_ms`.
fn assert_clicks(name: &str, clicks: &[f64], expected: &[f64], tolerance_ms: f64) {
    assert!(!expected.is_empty());
    for t in expected {
        let nearest = clicks
            .iter()
            .map(|c| (c - t).abs())
            .fold(f64::INFINITY, f64::min);
        assert!(
            nearest < tolerance_ms,
            "{name}: click at {t:.3} ms found {nearest:.3} ms away; clicks {clicks:?}"
        );
    }
}

#[test]
fn records_two_tracks_on_separate_clocks_and_realigns_them() {
    let s = setup();
    // A headset mic 300 ppm fast, system audio 200 ppm slow at 44.1 kHz stereo, which
    // starts playing 1.7 s after the recording starts.
    s.backend.edit(Source::Microphone, |d| d.ppm = 300.0);
    s.set_system(|d| {
        d.format = StreamFormat {
            sample_rate: 44_100,
            channels: 2,
        };
        d.ppm = -200.0;
        d.first_sound_after = Duration::from_millis(1_700);
    });
    let recording = s.start().unwrap();
    s.run_for(5_000);
    let summary = recording_stop(&s, recording);

    let [mic, system] = &summary.1.tracks[..] else {
        panic!("two tracks expected: {:?}", summary.1);
    };
    assert_eq!(mic.source, Source::Microphone);
    assert_eq!(
        mic.audio_path,
        Library::meeting_audio_dir(s.options.meeting_id).join("me.wav")
    );
    assert_eq!(
        system.timing_path.file_name().unwrap(),
        "others.timing.jsonl"
    );
    assert_eq!(system.format.unwrap().sample_rate, 44_100);
    assert_eq!(system.format.unwrap().channels, 1, "mixed down to mono");
    assert!(mic.first_audio_ms.unwrap() < 100);
    let offset = system.first_audio_ms.unwrap();
    assert!(
        (1_650..2_000).contains(&offset),
        "system started at {offset} ms"
    );
    assert!(system.padded_frames >= 44_100 * 165 / 100);
    assert!(
        problems(mic).is_empty() && problems(system).is_empty(),
        "{summary:?}"
    );

    // Both files cover the recording, so they line up even when played naively.
    for track in [mic, system] {
        let diff = track.duration_ms as i64 - summary.1.duration_ms as i64;
        assert!(diff.abs() < 80, "{track:?} vs {} ms", summary.1.duration_ms);
    }

    // Through their time lines, the same instants line up within a millisecond.
    let mic_clicks = clicks_ms(&s, mic);
    let system_clicks = clicks_ms(&s, system);
    let end = summary.1.duration_ms as f64 - 100.0;
    assert_clicks(
        "mic",
        &mic_clicks,
        &expected_clicks_from(&summary.0, 0.0, end),
        1.0,
    );
    assert_clicks(
        "system",
        &system_clicks,
        &expected_clicks_from(&summary.0, 1_800.0, end),
        1.0,
    );
    // Before the first sound, the system track is silence.
    assert!(
        system_clicks.iter().all(|&c| c > 1_600.0),
        "{system_clicks:?}"
    );

    let mic_timeline = Timeline::load(&s.path(&mic.timing_path)).unwrap();
    let drift = mic_timeline.drift_ppm().unwrap();
    assert!((drift - 300.0).abs() < 20.0, "mic drift {drift} ppm");
    let system_timeline = Timeline::load(&s.path(&system.timing_path)).unwrap();
    let first = system_timeline.first_audio_ms().unwrap();
    assert!((first - offset as f64).abs() < 2.0, "{first} vs {offset}");

    let started: Vec<_> = s
        .observer
        .events()
        .into_iter()
        .filter(|e| matches!(e, CaptureEvent::TrackStarted { .. }))
        .collect();
    assert_eq!(started.len(), 2, "{started:?}");
}

impl Setup {
    fn set_system(&self, f: impl FnOnce(&mut FakeDevice)) {
        self.backend.edit(Source::System, f);
    }
}

/// Stops a recording, keeping its start time for the checks.
fn recording_stop(s: &Setup, recording: Recording) -> (StartTime, RecordingSummary) {
    let start = StartTime(recording.start_ns());
    let summary = recording.stop().unwrap();
    for track in &summary.tracks {
        assert!(s.path(&track.audio_path).exists());
        // Stopped normally: the header already counts every frame.
        let info = wav::inspect(&s.path(&track.audio_path)).unwrap();
        assert_eq!(info.header_frames, info.file_frames);
        assert_eq!(info.file_frames, track.frames);
    }
    (start, summary)
}

/// The start time of a stopped recording.
#[derive(Debug)]
struct StartTime(u64);

impl StartTime {
    fn start_ns(&self) -> u64 {
        self.0
    }
}

fn expected_clicks_from(start: &StartTime, from_ms: f64, to_ms: f64) -> Vec<f64> {
    let start = start.start_ns();
    let first = start.div_ceil(CLICK_PERIOD_NS) * CLICK_PERIOD_NS;
    (0..)
        .map(|k| (first + k * CLICK_PERIOD_NS - start) as f64 / 1e6)
        .skip_while(|&t| t < from_ms)
        .take_while(|&t| t <= to_ms)
        .collect()
}

#[test]
fn follows_a_device_change_to_a_different_format() {
    let s = setup();
    let recording = s.start().unwrap();
    s.run_for(1_500);
    // A Bluetooth headset connects: 16 kHz stereo on its own clock.
    let mut headset = FakeDevice::new("headset", 16_000, 2);
    headset.ppm = 500.0;
    s.backend.set_default(Source::Microphone, headset);
    s.observer.wait_for("device changed", |e| {
        matches!(e, CaptureEvent::DeviceChanged { source: Source::Microphone, device } if device.id == "headset")
    });
    s.run_for(3_000);
    let (start, summary) = recording_stop(&s, recording);

    let mic = &summary.tracks[0];
    assert_eq!(mic.segments, 2);
    assert_eq!(mic.interruptions, 0);
    assert_eq!(mic.device.as_deref(), Some("Fake built-in-mic"));
    assert_eq!(
        mic.format,
        Some(StreamFormat {
            sample_rate: 48_000,
            channels: 1
        }),
        "the track keeps its format"
    );
    let timeline = Timeline::load(&s.path(&mic.timing_path)).unwrap();
    let segments: Vec<_> = timeline
        .lines()
        .iter()
        .filter_map(|l| match l {
            TimingLine::Segment {
                device_id,
                sample_rate,
                ..
            } => Some((device_id.clone(), *sample_rate)),
            _ => None,
        })
        .collect();
    assert_eq!(
        segments,
        [("built-in-mic".into(), 48_000), ("headset".into(), 16_000)]
    );
    // Clicks stay on time before and after the change, resampled from 16 kHz.
    let clicks = clicks_ms(&s, mic);
    let end = summary.duration_ms as f64 - 100.0;
    assert_clicks("mic", &clicks, &expected_clicks_from(&start, 0.0, end), 1.0);
    assert!(s.backend.state().opened.contains(&"headset".to_string()));
    // System audio was not disturbed.
    assert_eq!(summary.tracks[1].segments, 1);
}

#[test]
fn reopens_a_failed_stream_and_fills_the_gap() {
    let s = setup();
    let recording = s.start().unwrap();
    s.run_for(1_200);
    s.backend.fault(Source::System, error_code::STREAM_FAILED);
    s.observer.wait_for("interruption", |e| {
        matches!(
            e,
            CaptureEvent::TrackInterrupted {
                source: Source::System,
                code: error_code::STREAM_FAILED,
                ..
            }
        )
    });
    s.run_for(2_500);
    let (start, summary) = recording_stop(&s, recording);
    let system = &summary.tracks[1];
    assert_eq!(system.interruptions, 1);
    assert_eq!(system.segments, 2);
    let timeline = Timeline::load(&s.path(&system.timing_path)).unwrap();
    assert!(timeline.lines().iter().any(
        |l| matches!(l, TimingLine::Interrupted { code, .. } if code == error_code::STREAM_FAILED)
    ));
    let clicks = clicks_ms(&s, system);
    let end = summary.duration_ms as f64 - 100.0;
    assert_clicks(
        "system",
        &clicks,
        &expected_clicks_from(&start, 0.0, end),
        1.0,
    );
}

#[test]
fn reopens_a_stalled_stream() {
    let s = setup();
    s.set_system(|d| d.stall_after = Some(100));
    let recording = s.start().unwrap();
    s.observer.wait_for("stall", |e| {
        matches!(
            e,
            CaptureEvent::TrackInterrupted {
                source: Source::System,
                code: error_code::STREAM_STALLED,
                ..
            }
        )
    });
    s.run_for(2_000);
    let (_, summary) = recording_stop(&s, recording);
    let system = &summary.tracks[1];
    assert_eq!(system.segments, 2);
    assert!(
        system.padded_frames > 48_000,
        "the stall is filled with silence"
    );
    let diff = system.duration_ms as i64 - summary.duration_ms as i64;
    assert!(diff.abs() < 80, "{system:?}");
}

#[test]
fn keeps_retrying_a_device_that_cannot_be_reopened() {
    let s = setup();
    let recording = s.start().unwrap();
    s.run_for(500);
    s.backend
        .state()
        .fail_open
        .insert(Source::Microphone, error_code::DEVICE_UNAVAILABLE);
    s.backend
        .fault(Source::Microphone, error_code::DEVICE_UNAVAILABLE);
    s.observer.wait_for("reopen failure", |e| {
        matches!(
            e,
            CaptureEvent::TrackInterrupted {
                source: Source::Microphone,
                code: error_code::DEVICE_UNAVAILABLE,
                detail
            } if detail == "fake failure"
        )
    });
    s.run_for(500);
    s.backend.state().fail_open.clear();
    s.observer.wait_for("reopened", |e| {
        matches!(
            e,
            CaptureEvent::DeviceOpened {
                source: Source::Microphone,
                ..
            }
        ) && s.backend.state().opened.len() > 2
    });
    s.run_for(1_000);
    let (_, summary) = recording_stop(&s, recording);
    assert_eq!(summary.tracks[0].segments, 2);
}

#[test]
fn system_audio_that_never_plays_is_silence_for_the_whole_recording() {
    let s = setup();
    s.set_system(|d| d.first_sound_after = Duration::from_secs(3_600));
    let recording = s.start().unwrap();
    s.run_for(2_000);
    let (_, summary) = recording_stop(&s, recording);
    let system = &summary.tracks[1];
    assert_eq!(system.first_audio_ms, None);
    let diff = system.duration_ms as i64 - summary.duration_ms as i64;
    assert!(diff.abs() < 5, "{system:?}");
    let (_, samples) = wav::read(&s.path(&system.audio_path)).unwrap();
    assert!(samples.iter().all(|&x| x == 0.0));
    assert!(
        Timeline::load(&s.path(&system.timing_path))
            .unwrap()
            .first_audio_ms()
            .is_none()
    );
}

#[test]
fn warns_when_a_track_is_only_digital_silence() {
    let s = setup();
    s.set_system(|d| d.signal = Signal::Zeros);
    let recording = s.start().unwrap();
    s.observer.wait_for("silence warning", |e| {
        matches!(
            e,
            CaptureEvent::TrackProblem {
                source: Source::System,
                code: error_code::SYSTEM_AUDIO_SILENT,
                ..
            }
        )
    });
    let (_, summary) = recording_stop(&s, recording);
    assert_eq!(
        problems(&summary.tracks[1]),
        [error_code::SYSTEM_AUDIO_SILENT]
    );
    assert!(
        s.observer
            .problems(Source::Microphone)
            .iter()
            .all(|&p| p == error_code::FRAMES_DROPPED)
    );
}

#[test]
fn sends_live_levels_for_both_tracks() {
    let s = setup();
    let recording = s.start().unwrap();
    s.run_for(2_000);
    drop(recording);
    let levels = s.observer.levels.lock().unwrap().clone();
    for source in Source::ALL {
        let ours: Vec<_> = levels.iter().filter(|l| l.source == source).collect();
        // 20 a second: about 40 in 2 s.
        assert!(ours.len() >= 25, "{source:?}: {} levels", ours.len());
        assert!(ours.iter().all(|l| l.rms > 0.0 && l.rms < 0.1));
        assert!(
            ours.iter().any(|l| l.peak > 0.8),
            "a click shows in the peak"
        );
        assert!(
            ours.iter()
                .all(|l| l.peak_dbfs() <= 0.0 && l.rms_dbfs() > -120.0)
        );
    }
}

#[test]
fn asks_for_microphone_access_once_and_respects_a_no() {
    // Not asked yet: ask, and record when the user agrees.
    let s = setup();
    s.backend.state().microphone = PermissionState::NotDetermined;
    let recording = s.start().unwrap();
    assert_eq!(s.backend.requests.load(Ordering::Relaxed), 1);
    drop(recording);

    // Not asked yet, and the user says no.
    let s = setup();
    {
        let mut state = s.backend.state();
        state.microphone = PermissionState::NotDetermined;
        state.answer = PermissionState::Denied;
    }
    assert_eq!(s.start().unwrap_err().code, error_code::MICROPHONE_DENIED);

    // Denied or restricted earlier: don't ask again, and leave nothing behind.
    for (state, code) in [
        (PermissionState::Denied, error_code::MICROPHONE_DENIED),
        (
            PermissionState::Restricted,
            error_code::MICROPHONE_RESTRICTED,
        ),
    ] {
        let s = setup();
        s.backend.state().microphone = state;
        assert_eq!(s.start().unwrap_err().code, code);
        assert_eq!(s.backend.requests.load(Ordering::Relaxed), 0);
        assert!(s.backend.state().opened.is_empty());
        assert!(!s.audio_dir().join("me.wav").exists());
    }

    // System audio alone does not need the microphone.
    let mut s = setup();
    s.backend.state().microphone = PermissionState::Denied;
    s.options.microphone = false;
    let (_, summary) = recording_stop(&s, s.start().unwrap());
    assert_eq!(summary.tracks.len(), 1);
    assert_eq!(summary.tracks[0].source, Source::System);
}

#[test]
fn a_failure_to_start_leaves_nothing_behind() {
    let s = setup();
    s.backend
        .state()
        .fail_open
        .insert(Source::System, error_code::SYSTEM_AUDIO_UNAVAILABLE);
    let err = s.start().unwrap_err();
    assert_eq!(err.code, error_code::SYSTEM_AUDIO_UNAVAILABLE);
    // The microphone had started: its files are removed again.
    assert_eq!(s.backend.state().opened, ["built-in-mic"]);
    assert!(!s.audio_dir().exists(), "audio folder left behind");

    let mut s = setup();
    s.options.microphone = false;
    s.options.system_audio = false;
    assert_eq!(s.start().unwrap_err().code, error_code::NOTHING_TO_RECORD);
}

#[test]
fn never_overwrites_a_recording() {
    let s = setup();
    drop(s.start().unwrap());
    let before = std::fs::read(s.audio_dir().join("me.wav")).unwrap();
    assert_eq!(s.start().unwrap_err().code, error_code::ALREADY_RECORDED);
    assert_eq!(std::fs::read(s.audio_dir().join("me.wav")).unwrap(), before);
}

#[test]
fn reports_the_wav_size_limit_and_keeps_the_other_track() {
    let mut s = setup();
    // About half a second of 48 kHz mono.
    s.options.max_track_bytes = 100_000;
    let recording = s.start().unwrap();
    s.observer.wait_for("size limit", |e| {
        matches!(
            e,
            CaptureEvent::TrackProblem {
                code: error_code::TRACK_SIZE_LIMIT,
                ..
            }
        )
    });
    s.run_for(1_000);
    let summary = recording.stop().unwrap();
    for track in &summary.tracks {
        assert_eq!(problems(track), [error_code::TRACK_SIZE_LIMIT]);
        let info = wav::inspect(&s.path(&track.audio_path)).unwrap();
        assert_eq!(info.header_frames, info.file_frames, "still a valid file");
        assert!(info.file_frames <= 25_000);
    }
}

#[test]
fn registers_the_tracks_in_the_library() {
    let s = setup();
    let mut library = Library::open(s.dir.path()).unwrap();
    let meeting = library.create_meeting("it", 0).unwrap().id;
    let mut options = s.options.clone();
    options.meeting_id = meeting;
    let recording = Recording::start(s.backend.clone(), options, Arc::new(NoObserver)).unwrap();
    let tracks = recording.register_tracks(&mut library).unwrap();
    recording.stop().unwrap();

    assert_eq!(tracks.len(), 2);
    assert_eq!(library.tracks(meeting).unwrap(), tracks);
    let dir = Library::meeting_audio_dir(meeting);
    assert_eq!(tracks[0].role, Role::Me);
    assert_eq!(tracks[0].audio_path, dir.join("me.wav"));
    assert_eq!(tracks[0].timing_path, Some(dir.join("me.timing.jsonl")));
    assert_eq!(tracks[0].device.as_deref(), Some("Fake built-in-mic"));
    assert_eq!(tracks[1].role, Role::Others);
    assert_eq!(tracks[1].audio_path, dir.join("others.wav"));
    for track in &tracks {
        assert!(s.dir.path().join(&track.audio_path).exists());
    }
}

/// What a crash leaves on disk: the files as they are mid-recording, without the
/// final header patch.
#[test]
fn a_crash_loses_at_most_a_tenth_of_a_second() {
    let mut s = setup();
    // Real time, real writer timings.
    let backend = FakeBackend::with_speed(1.0);
    s.options.tuning = Tuning::default();
    let recording =
        Recording::start(backend.clone(), s.options.clone(), s.observer.clone()).unwrap();
    std::thread::sleep(Duration::from_millis(1_600));
    // The process dies now: copy the files as they are.
    let crashed = s.dir.path().join("crashed.wav");
    let captured_ms = (backend.now_ns() - recording.start_ns()) as f64 / 1e6;
    std::fs::copy(s.audio_dir().join("me.wav"), &crashed).unwrap();
    std::fs::copy(
        s.audio_dir().join("me.timing.jsonl"),
        s.dir.path().join("crashed.timing.jsonl"),
    )
    .unwrap();
    drop(recording);

    // Readable as is, up to the last checkpoint (at most a second old)...
    let before = wav::inspect(&crashed).unwrap();
    assert!(before.header_frames <= before.file_frames);
    // ...and after a repair, up to the crash.
    let after = repair_wav(&crashed).unwrap();
    assert_eq!(after.header_frames, after.file_frames);
    let lost = captured_ms - after.duration_ms() as f64;
    assert!(lost < 100.0, "lost {lost:.1} ms");
    // The sidecar survives too, a torn last line at worst.
    let timeline = Timeline::load(&s.dir.path().join("crashed.timing.jsonl")).unwrap();
    assert!(timeline.drift_ppm().is_some());
}

#[test]
fn repairs_the_audio_of_an_interrupted_meeting() {
    let s = setup();
    drop(s.start().unwrap());
    // Append half a second of audio the header does not count, as after a crash.
    let me = s.audio_dir().join("me.wav");
    let mut bytes = std::fs::read(&me).unwrap();
    bytes.extend(std::iter::repeat_n(0u8, 48_000 * 2));
    std::fs::write(&me, bytes).unwrap();
    let before = wav::inspect(&me).unwrap();
    assert_eq!(before.file_frames - before.header_frames, 24_000);

    let repaired = repair_meeting_audio(s.dir.path(), s.options.meeting_id).unwrap();
    assert_eq!(repaired.len(), 2);
    assert_eq!(repaired[0].0.file_name().unwrap(), "me.wav");
    assert_eq!(repaired[0].1.header_frames, before.file_frames);
}
