//! Transcription tests that need no model: the pipeline, the timeline and the job, with a
//! fake detector and a fake recognizer. Tests with the real model are in
//! `transcription_model.rs` (ignored by default).
//!
//! The fake audio is made of tones: the fake detector hears any tone as speech, and the
//! fake recognizer "reads" each tone as a word chosen by its pitch, so a test can tell
//! which audio reached the recognizer.

mod common;

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{ManualClock, library};
use trama_core::jobs::*;
use trama_core::models::{ModelSpec, transcription_model};
use trama_core::storage::*;
use trama_core::transcription::error_code;
use trama_core::transcription::*;

const RATE: usize = SAMPLE_RATE as usize;
const WINDOW: usize = 512;

/// Pitches the fake recognizer knows, and the word it reads for each.
const WORDS: &[(f32, &str)] = &[(250.0, "uno"), (500.0, "due"), (1000.0, "tre")];

fn tone(seconds: f32, hz: f32) -> Vec<f32> {
    (0..(seconds * RATE as f32) as usize)
        .map(|i| 0.3 * (2.0 * std::f32::consts::PI * hz * i as f32 / RATE as f32).sin())
        .collect()
}

fn silence(seconds: f32) -> Vec<f32> {
    vec![0.0; (seconds * RATE as f32) as usize]
}

fn concat(parts: &[Vec<f32>]) -> Vec<f32> {
    parts.concat()
}

fn rms(window: &[f32]) -> f32 {
    (window.iter().map(|s| s * s).sum::<f32>() / window.len().max(1) as f32).sqrt()
}

/// Hears any window louder than -40 dBFS as speech; closes a stretch after
/// `min_silence_s` of silence and splits it at `max_speech_s`, like Silero in sherpa-onnx.
struct FakeDetector {
    settings: VadSettings,
    position: u64,
    partial: Vec<f32>,
    open: Option<u64>,
    silent_since: Option<u64>,
    found: Vec<SpeechSpan>,
}

impl FakeDetector {
    fn new(settings: VadSettings) -> Self {
        Self {
            settings,
            position: 0,
            partial: Vec::new(),
            open: None,
            silent_since: None,
            found: Vec::new(),
        }
    }

    fn window(&mut self, speech: bool) {
        let at = self.position;
        self.position += WINDOW as u64;
        let min_silence = (self.settings.min_silence_s * RATE as f32) as u64;
        let max_speech = (self.settings.max_speech_s * RATE as f32) as u64;
        match (self.open, speech) {
            (None, true) => self.open = Some(at),
            (Some(start), true) => {
                self.silent_since = None;
                if self.position - start >= max_speech {
                    self.found.push(SpeechSpan {
                        start,
                        end: self.position,
                    });
                    self.open = Some(self.position);
                }
            }
            (Some(start), false) => {
                let since = *self.silent_since.get_or_insert(at);
                if self.position - since >= min_silence {
                    self.found.push(SpeechSpan { start, end: since });
                    self.open = None;
                    self.silent_since = None;
                }
            }
            (None, false) => {}
        }
    }
}

impl VoiceDetector for FakeDetector {
    fn accept(&mut self, samples: &[f32]) {
        self.partial.extend_from_slice(samples);
        while self.partial.len() >= WINDOW {
            let speech = rms(&self.partial[..WINDOW]) > 0.01;
            self.partial.drain(..WINDOW);
            self.window(speech);
        }
    }

    fn flush(&mut self) {
        if let Some(start) = self.open.take() {
            let end = self.silent_since.take().unwrap_or(self.position);
            self.found.push(SpeechSpan { start, end });
        }
    }

    fn pop(&mut self) -> Option<SpeechSpan> {
        (!self.found.is_empty()).then(|| self.found.remove(0))
    }

    fn in_speech(&self) -> bool {
        self.open.is_some() && self.silent_since.is_none()
    }
}

/// Reads one word per stretch of tone in what it receives, by pitch (zero crossings),
/// and counts the samples it decodes.
#[derive(Default)]
struct FakeRecognizer {
    decoded: Arc<AtomicU64>,
    windows: Vec<usize>,
}

impl SpeechRecognizer for FakeRecognizer {
    fn recognize(&mut self, samples: &[f32]) -> trama_core::transcription::Result<String> {
        self.decoded
            .fetch_add(samples.len() as u64, Ordering::SeqCst);
        self.windows.push(samples.len());
        let mut words: Vec<&str> = Vec::new();
        let mut last: Option<&str> = None;
        for block in samples.chunks(RATE / 10) {
            // Pitch from the zero crossings of the loud 10 ms pieces only, so a block
            // where a tone ends is not read as a lower one.
            let loud: Vec<&[f32]> = block
                .chunks_exact(RATE / 100)
                .filter(|piece| rms(piece) > 0.01)
                .collect();
            if loud.len() < 3 {
                last = None;
                continue;
            }
            let crossings: usize = loud
                .iter()
                .map(|piece| {
                    piece
                        .windows(2)
                        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
                        .count()
                })
                .sum();
            let duration = (loud.len() * RATE / 100) as f32 / RATE as f32;
            let hz = crossings as f32 / 2.0 / duration;
            let word = WORDS
                .iter()
                .min_by(|a, b| (a.0 - hz).abs().total_cmp(&(b.0 - hz).abs()))
                .map(|(_, w)| *w)
                .unwrap();
            if last != Some(word) {
                words.push(word);
            }
            last = Some(word);
        }
        Ok(words.join(" "))
    }
}

fn run(samples: &[f32]) -> (Vec<SpokenSegment>, FakeRecognizer) {
    let mut recognizer = FakeRecognizer::default();
    let mut detector = FakeDetector::new(VadSettings::ADR_0007);
    let segments = transcribe_track(
        &mut SliceSource::new(samples, 8_000),
        &mut detector,
        &mut recognizer,
        &VadSettings::ADR_0007,
        &mut |_| {},
        &|| false,
    )
    .unwrap();
    (segments, recognizer)
}

/// Milliseconds rounded to tenths of a second.
fn tenths(ms: i64) -> i64 {
    (ms + 50) / 100
}

fn seconds(samples: u64) -> f32 {
    samples as f32 / RATE as f32
}

// --- The pipeline ---

#[test]
fn each_stretch_of_speech_becomes_a_segment() {
    let audio = concat(&[
        silence(1.0),
        tone(2.0, 250.0),
        silence(3.0),
        tone(1.5, 1000.0),
        silence(2.0),
    ]);
    let (segments, recognizer) = run(&audio);
    let texts: Vec<_> = segments.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, ["uno", "tre"]);
    assert!((seconds(segments[0].start) - 1.0).abs() < 0.05);
    assert!((seconds(segments[0].end) - 3.0).abs() < 0.05);
    assert!((seconds(segments[1].start) - 6.0).abs() < 0.05);
    assert!((seconds(segments[1].end) - 7.5).abs() < 0.05);
    // Each stretch was decoded with 500 ms of padding on both sides.
    assert_eq!(recognizer.windows.len(), 2);
    assert!((seconds(recognizer.windows[0] as u64) - 3.0).abs() < 0.05);
    assert!((seconds(recognizer.windows[1] as u64) - 2.5).abs() < 0.05);
}

#[test]
fn padding_stops_at_the_edges_of_the_track() {
    let audio = concat(&[tone(1.0, 500.0), silence(1.2)]);
    let (segments, recognizer) = run(&audio);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].start, 0);
    assert!((seconds(recognizer.windows[0] as u64) - 1.5).abs() < 0.05);

    let (segments, _) = run(&tone(2.0, 500.0));
    assert_eq!(segments.len(), 1, "open speech is closed at the end");
}

#[test]
fn long_speech_is_split_and_no_audio_is_decoded_twice() {
    // 70 s without a pause: split at 30 s, 60 s.
    let audio = concat(&[silence(0.5), tone(70.0, 250.0), silence(2.0)]);
    let (segments, recognizer) = run(&audio);
    assert_eq!(segments.len(), 3, "{segments:?}");
    for pair in segments.windows(2) {
        assert!(pair[0].end <= pair[1].start);
    }
    let decoded = recognizer.decoded.load(Ordering::SeqCst);
    assert!(
        decoded <= audio.len() as u64,
        "decoded {decoded} of {} samples: windows overlap",
        audio.len()
    );
    // The first window still has its padding in front, the last one behind.
    assert!(seconds(recognizer.windows[0] as u64) >= 30.4);
}

#[test]
fn close_stretches_share_the_pause_without_overlapping() {
    // A 1.2 s pause closes the first stretch; 500 ms of padding on each side would meet
    // in the middle of it.
    let audio = concat(&[
        tone(2.0, 250.0),
        silence(1.2),
        tone(2.0, 1000.0),
        silence(1.5),
    ]);
    let (segments, recognizer) = run(&audio);
    let texts: Vec<_> = segments.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, ["uno", "tre"]);
    assert!(recognizer.decoded.load(Ordering::SeqCst) <= audio.len() as u64);
}

#[test]
fn silence_produces_no_segments_and_no_decoding() {
    let (segments, recognizer) = run(&silence(120.0));
    assert!(segments.is_empty());
    assert!(recognizer.windows.is_empty());
}

#[test]
fn stops_when_asked_and_reports_progress() {
    let audio = concat(&[
        tone(1.0, 250.0),
        silence(2.0),
        tone(1.0, 500.0),
        silence(60.0),
    ]);
    let mut recognizer = FakeRecognizer::default();
    let mut detector = FakeDetector::new(VadSettings::ADR_0007);
    let mut seen = Vec::new();
    let segments = transcribe_track(
        &mut SliceSource::new(&audio, 8_000),
        &mut detector,
        &mut recognizer,
        &VadSettings::ADR_0007,
        &mut |p| seen.push(p),
        &|| false,
    )
    .unwrap();
    assert_eq!(segments.len(), 2);
    assert!(seen.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(seen.last(), Some(&1.0));

    let stop = AtomicBool::new(false);
    let mut recognizer = FakeRecognizer::default();
    let err = transcribe_track(
        &mut SliceSource::new(&audio, 8_000),
        &mut FakeDetector::new(VadSettings::ADR_0007),
        &mut recognizer,
        &VadSettings::ADR_0007,
        &mut |p| stop.store(p > 0.5, Ordering::SeqCst),
        &|| stop.load(Ordering::SeqCst),
    )
    .unwrap_err();
    assert_eq!(err.code, error_code::STOPPED);
}

// --- Two tracks on one timeline ---

/// An engine made of the fakes.
struct FakeEngine {
    decoded: Arc<AtomicU64>,
    recognizer: FakeRecognizer,
}

impl Engine for FakeEngine {
    fn description(&self) -> String {
        "fake engine".into()
    }

    fn settings(&self) -> VadSettings {
        VadSettings::ADR_0007
    }

    fn detector(&mut self) -> trama_core::transcription::Result<Box<dyn VoiceDetector>> {
        Ok(Box::new(FakeDetector::new(VadSettings::ADR_0007)))
    }

    fn recognizer(&mut self) -> &mut dyn SpeechRecognizer {
        &mut self.recognizer
    }
}

fn fake_engine() -> FakeEngine {
    let decoded = Arc::new(AtomicU64::new(0));
    FakeEngine {
        recognizer: FakeRecognizer {
            decoded: Arc::clone(&decoded),
            windows: Vec::new(),
        },
        decoded,
    }
}

/// Writes 16 kHz samples as a 48 kHz 32-bit float WAV, like the recorder (each sample
/// repeated three times; good enough for tones).
fn write_track(path: &Path, samples: &[f32]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for &sample in samples {
        for _ in 0..3 {
            writer.write_sample(sample).unwrap();
        }
    }
    writer.finalize().unwrap();
}

/// A timing sidecar for a 48 kHz track whose first frame was captured `offset_ms` after
/// the recording started, on a device running `ppm` fast.
fn write_sidecar(path: &Path, offset_ms: u64, frames: u64, ppm: f64) {
    let origin = 5_000_000_000u64;
    let first = origin + offset_ms * 1_000_000;
    let host = |frame: u64| first + (frame as f64 / (48_000.0 * (1.0 + ppm / 1e6)) * 1e9) as u64;
    let mut text = format!(
        "{{\"t\":\"start\",\"version\":1,\"source\":\"x\",\"sample_rate\":48000,\"channels\":1,\"recording_start_ns\":{origin},\"unix_ms\":0}}\n"
    );
    for frame in (0..frames).step_by(48_000) {
        text.push_str(&format!(
            "{{\"t\":\"anchor\",\"frame\":{frame},\"host_ns\":{}}}\n",
            host(frame)
        ));
    }
    text.push_str(&format!(
        "{{\"t\":\"stop\",\"frame\":{frames},\"host_ns\":{},\"unix_ms\":0}}\n",
        host(frames)
    ));
    std::fs::write(path, text).unwrap();
}

#[test]
fn both_tracks_are_merged_on_the_recording_timeline() {
    let dir = tempfile::tempdir().unwrap();
    // Me: speech at 1-3 s and 9-10 s of the file, which starts with the recording.
    let me = concat(&[
        silence(1.0),
        tone(2.0, 250.0),
        silence(6.0),
        tone(1.0, 1000.0),
        silence(2.0),
    ]);
    // Others: the system track starts 4 s late; speech at 1-3 s of its file, so 5-7 s
    // of the recording.
    let others = concat(&[silence(1.0), tone(2.0, 500.0), silence(4.0)]);
    write_track(&dir.path().join("me.wav"), &me);
    write_track(&dir.path().join("others.wav"), &others);
    write_sidecar(
        &dir.path().join("others.timing.jsonl"),
        4_000,
        others.len() as u64 * 3,
        0.0,
    );
    let tracks = [
        TrackAudio {
            role: Role::Me,
            audio_path: dir.path().join("me.wav"),
            timing_path: None,
        },
        TrackAudio {
            role: Role::Others,
            audio_path: dir.path().join("others.wav"),
            timing_path: Some(dir.path().join("others.timing.jsonl")),
        },
    ];
    let mut engine = fake_engine();
    let mut seen = Vec::new();
    let segments =
        transcribe_tracks(&mut engine, &tracks, &mut |p| seen.push(p), &|| false).unwrap();
    let summary: Vec<_> = segments
        .iter()
        .map(|s| {
            (
                s.role,
                s.text.as_str(),
                tenths(s.start_ms),
                tenths(s.end_ms),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (Role::Me, "uno", 10, 30),
            (Role::Others, "due", 50, 70),
            (Role::Me, "tre", 90, 100),
        ]
    );
    assert!(seen.windows(2).all(|w| w[0] <= w[1] + 1e-6));
    assert!((seen.last().unwrap() - 1.0).abs() < 1e-6);
    assert!(engine.decoded.load(Ordering::SeqCst) > 0);
}

#[test]
fn a_damaged_sidecar_falls_back_to_the_nominal_rate() {
    let dir = tempfile::tempdir().unwrap();
    write_track(
        &dir.path().join("me.wav"),
        &concat(&[silence(2.0), tone(1.0, 250.0), silence(2.0)]),
    );
    std::fs::write(dir.path().join("me.timing.jsonl"), "garbage\n{}\n").unwrap();
    let tracks = [TrackAudio {
        role: Role::Me,
        audio_path: dir.path().join("me.wav"),
        timing_path: Some(dir.path().join("me.timing.jsonl")),
    }];
    let segments = transcribe_tracks(&mut fake_engine(), &tracks, &mut |_| {}, &|| false).unwrap();
    assert_eq!(segments.len(), 1);
    assert_eq!(tenths(segments[0].start_ms), 20);
}

#[test]
fn missing_audio_and_no_tracks_are_permanent_errors() {
    let dir = tempfile::tempdir().unwrap();
    let tracks = [TrackAudio {
        role: Role::Me,
        audio_path: dir.path().join("me.wav"),
        timing_path: None,
    }];
    let err = transcribe_tracks(&mut fake_engine(), &tracks, &mut |_| {}, &|| false).unwrap_err();
    assert_eq!(
        (err.code, err.retryable),
        (error_code::AUDIO_MISSING, false)
    );
    let err = transcribe_tracks(&mut fake_engine(), &[], &mut |_| {}, &|| false).unwrap_err();
    assert_eq!(err.code, error_code::NO_AUDIO);
}

// --- The real loader without a model ---

#[test]
fn the_real_engine_reports_a_missing_or_corrupt_model() {
    let dir = tempfile::tempdir().unwrap();
    let err = SherpaLoader::new(dir.path()).load().err().unwrap();
    assert_eq!(
        (err.code, err.retryable),
        (error_code::MODEL_MISSING, false)
    );

    let spec = transcription_model();
    let model_dir = dir.path().join(&spec.id);
    std::fs::create_dir_all(&model_dir).unwrap();
    std::fs::write(model_dir.join("CORRUPT"), "encoder.weights\n").unwrap();
    let err = SherpaLoader::new(dir.path()).load().err().unwrap();
    assert_eq!(err.code, error_code::MODEL_CORRUPT);
}

#[test]
fn the_real_engine_rejects_files_that_are_not_a_model() {
    // A "ready" model whose files are junk: sherpa-onnx refuses it, and it says so.
    let dir = tempfile::tempdir().unwrap();
    let mut spec: ModelSpec = transcription_model();
    for file in &mut spec.files {
        file.size = 4;
    }
    let model_dir = dir.path().join(&spec.id);
    std::fs::create_dir_all(&model_dir).unwrap();
    let mut sums = String::new();
    for file in &spec.files {
        std::fs::write(model_dir.join(&file.name), b"junk").unwrap();
        sums.push_str(&format!("{}  {}\n", file.sha256, file.name));
    }
    std::fs::write(model_dir.join("SHA256SUMS"), sums).unwrap();
    let err = SherpaLoader::with_spec(dir.path(), spec)
        .load()
        .err()
        .unwrap();
    assert_eq!(err.code, error_code::MODEL_LOAD_FAILED);
}

// --- The job ---

const PATIENCE: Duration = Duration::from_secs(20);

/// Loads the fake engine, or fails with a given error. Can hold every recognition until
/// a gate opens, so a test can cancel a running transcription.
#[derive(Clone, Default)]
struct FakeLoader {
    error: Option<TranscriptionError>,
    gate: Option<Arc<AtomicBool>>,
}

struct GatedEngine {
    inner: FakeEngine,
    gate: Option<Arc<AtomicBool>>,
}

impl EngineLoader for FakeLoader {
    fn load(&self) -> trama_core::transcription::Result<Box<dyn Engine>> {
        match &self.error {
            Some(err) => Err(err.clone()),
            None => Ok(Box::new(GatedEngine {
                inner: fake_engine(),
                gate: self.gate.clone(),
            })),
        }
    }
}

impl Engine for GatedEngine {
    fn description(&self) -> String {
        self.inner.description()
    }

    fn settings(&self) -> VadSettings {
        self.inner.settings()
    }

    fn detector(&mut self) -> trama_core::transcription::Result<Box<dyn VoiceDetector>> {
        self.inner.detector()
    }

    fn recognizer(&mut self) -> &mut dyn SpeechRecognizer {
        if let Some(gate) = &self.gate {
            while !gate.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        &mut self.inner.recognizer
    }
}

struct Events(Receiver<JobEvent>);

impl Events {
    fn sink() -> (Arc<dyn EventSink>, Self) {
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let sink = move |event: JobEvent| {
            let _ = tx.lock().unwrap().send(event);
        };
        (Arc::new(sink), Self(rx))
    }

    #[track_caller]
    fn wait_for(&self, want: impl Fn(&JobEvent) -> bool) -> Vec<JobEvent> {
        let deadline = Instant::now() + PATIENCE;
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Ok(event) = self.0.recv_timeout(left) else {
                panic!("event not received; seen: {seen:#?}");
            };
            let done = want(&event);
            seen.push(event);
            if done {
                return seen;
            }
        }
    }
}

fn finished(event: &JobEvent) -> bool {
    !event.job.is_active() && event.progress.is_none()
}

/// A recorded meeting with both tracks, ready to transcribe.
fn recorded_meeting(library: &mut Library, language: &str) -> Id {
    let id = library
        .create_meeting(language, ManualClock::START)
        .unwrap()
        .id;
    let audio = library.root().join(Library::meeting_audio_dir(id));
    std::fs::create_dir_all(&audio).unwrap();
    write_track(
        &audio.join("me.wav"),
        &concat(&[silence(0.5), tone(1.0, 250.0), silence(4.0)]),
    );
    write_track(
        &audio.join("others.wav"),
        &concat(&[silence(2.5), tone(1.0, 500.0), silence(2.0)]),
    );
    let relative = Library::meeting_audio_dir(id);
    for role in [Role::Me, Role::Others] {
        library
            .add_track(
                id,
                &NewTrack {
                    role,
                    audio_path: relative.join(format!("{}.wav", role.as_str())),
                    timing_path: None,
                    device: None,
                },
            )
            .unwrap();
    }
    library
        .set_meeting_status(id, MeetingStatus::Processing, Some(5_500))
        .unwrap();
    id
}

fn start(loader: FakeLoader) -> (tempfile::TempDir, SharedLibrary, Runner, Events) {
    let (dir, _clock, library) = library();
    let library = Arc::new(Mutex::new(library));
    let (sink, events) = Events::sink();
    let runner = Runner::start(
        Arc::clone(&library),
        vec![Arc::new(Transcribe::new(loader))],
        sink,
    );
    (dir, library, runner, events)
}

#[test]
fn the_job_stores_a_two_track_transcript_and_marks_the_meeting_ready() {
    let (_dir, library, runner, events) = start(FakeLoader::default());
    let id = recorded_meeting(&mut lock(&library), "it");
    let job = runner.enqueue(JobKind::Transcribe, id).unwrap();
    let seen = events.wait_for(finished);
    let last = seen.last().unwrap();
    assert_eq!(last.job.state, JobState::Done, "{:?}", last.job);
    let progress: Vec<f32> = seen.iter().filter_map(|e| e.progress).collect();
    assert!(progress.windows(2).all(|w| w[0] <= w[1]), "{progress:?}");
    assert_eq!(progress.last(), Some(&1.0));

    let library = lock(&library);
    assert_eq!(library.meeting(id).unwrap().status, MeetingStatus::Ready);
    let version = library.current_version(id).unwrap().unwrap();
    assert_eq!(version.engine.as_deref(), Some("fake engine"));
    assert!(version.is_sealed());
    let segments: Vec<_> = library
        .segments(version.id)
        .unwrap()
        .into_iter()
        .map(|s| (s.role, s.text, tenths(s.start_ms)))
        .collect();
    assert_eq!(
        segments,
        [
            (Role::Me, "uno".to_owned(), 5),
            (Role::Others, "due".to_owned(), 25)
        ]
    );
    assert_eq!(library.job(job.id).unwrap().state, JobState::Done);
}

#[test]
fn a_completed_transcription_queues_the_summary() {
    let (_dir, library, runner, events) = start(FakeLoader::default());
    let id = recorded_meeting(&mut lock(&library), "it");
    let transcribe = runner.enqueue(JobKind::Transcribe, id).unwrap();
    let seen = events.wait_for(|e| e.job.id == transcribe.id && finished(e));
    assert_eq!(seen.last().unwrap().job.state, JobState::Done);

    // Queued once the transcript is stored, and announced to the interface.
    let queued = seen
        .iter()
        .find(|e| e.job.kind == JobKind::Summarize)
        .expect("no event for the summary job");
    assert_eq!(queued.job.target_id, id);
    assert_eq!(queued.job.state, JobState::Queued);
    let active = lock(&library).active_jobs().unwrap();
    assert!(
        active
            .iter()
            .any(|job| job.kind == JobKind::Summarize && job.target_id == id),
        "{active:?}"
    );
}

#[test]
fn a_completed_transcription_queues_the_audio_deletion_only_when_due() {
    // "Never" (the default): the audio goes as soon as the transcript is stored.
    let (_dir, library, runner, events) = start(FakeLoader::default());
    let id = recorded_meeting(&mut lock(&library), "it");
    let transcribe = runner.enqueue(JobKind::Transcribe, id).unwrap();
    let seen = events.wait_for(|e| e.job.id == transcribe.id && finished(e));
    let queued = seen
        .iter()
        .find(|e| e.job.kind == JobKind::DeleteAudio)
        .expect("no event for the audio deletion");
    assert_eq!(queued.job.target_id, id);
    drop(runner);

    // "7 days": nothing is queued yet.
    let (_dir, library, runner, events) = start(FakeLoader::default());
    {
        let mut library = lock(&library);
        let mut settings = library.library_settings().unwrap();
        settings.audio_retention = AudioRetention::Days7;
        library.set_library_settings(&settings).unwrap();
    }
    let id = recorded_meeting(&mut lock(&library), "it");
    let transcribe = runner.enqueue(JobKind::Transcribe, id).unwrap();
    let seen = events.wait_for(|e| e.job.id == transcribe.id && finished(e));
    assert!(seen.iter().all(|e| e.job.kind != JobKind::DeleteAudio));
}

#[test]
fn a_failed_transcription_queues_no_summary() {
    let (_dir, library, runner, events) = start(FakeLoader {
        error: Some(TranscriptionError::permanent(
            error_code::MODEL_MISSING,
            "/models",
        )),
        gate: None,
    });
    let id = recorded_meeting(&mut lock(&library), "it");
    runner.enqueue(JobKind::Transcribe, id).unwrap();
    let seen = events.wait_for(finished);
    assert!(seen.iter().all(|e| e.job.kind == JobKind::Transcribe));
    assert!(lock(&library).active_jobs().unwrap().is_empty());
}

#[test]
fn without_the_model_the_job_fails_at_once_and_says_why() {
    let (_dir, library, runner, events) = start(FakeLoader {
        error: Some(TranscriptionError::permanent(
            error_code::MODEL_MISSING,
            "/models",
        )),
        gate: None,
    });
    let id = recorded_meeting(&mut lock(&library), "it");
    runner.enqueue(JobKind::Transcribe, id).unwrap();
    let last = events.wait_for(finished).pop().unwrap();
    assert_eq!(last.job.state, JobState::Failed);
    assert_eq!(last.job.attempts, 1);
    assert_eq!(
        last.job.error_code.as_deref(),
        Some(error_code::MODEL_MISSING)
    );
    let meeting = lock(&library).meeting(id).unwrap();
    assert_eq!(meeting.status, MeetingStatus::Failed);
    assert_eq!(
        meeting.error_code.as_deref(),
        Some(error_code::MODEL_MISSING)
    );
}

#[test]
fn a_retryable_failure_keeps_the_meeting_processing() {
    let (_dir, library, runner, events) = start(FakeLoader {
        error: Some(TranscriptionError::retryable(
            error_code::MODEL_LOAD_FAILED,
            "out of memory",
        )),
        gate: None,
    });
    let id = recorded_meeting(&mut lock(&library), "it");
    runner.enqueue(JobKind::Transcribe, id).unwrap();
    let last = events
        .wait_for(|e| e.job.state == JobState::Queued && e.job.attempts == 1)
        .pop()
        .unwrap();
    assert_eq!(
        last.job.error_code.as_deref(),
        Some(error_code::MODEL_LOAD_FAILED)
    );
    assert_eq!(
        lock(&library).meeting(id).unwrap().status,
        MeetingStatus::Processing
    );
}

#[test]
fn deleted_audio_and_unknown_languages_fail_the_meeting() {
    let (_dir, library, runner, events) = start(FakeLoader::default());
    let deleted = recorded_meeting(&mut lock(&library), "it");
    lock(&library).mark_audio_deleted(deleted).unwrap();
    runner.enqueue(JobKind::Transcribe, deleted).unwrap();
    let last = events.wait_for(finished).pop().unwrap();
    assert_eq!(
        last.job.error_code.as_deref(),
        Some(error_code::AUDIO_MISSING)
    );
    assert_eq!(
        lock(&library)
            .meeting(deleted)
            .unwrap()
            .error_code
            .as_deref(),
        Some(error_code::AUDIO_MISSING)
    );

    let japanese = recorded_meeting(&mut lock(&library), "ja");
    runner.enqueue(JobKind::Transcribe, japanese).unwrap();
    let last = events.wait_for(finished).pop().unwrap();
    assert_eq!(
        last.job.error_code.as_deref(),
        Some(error_code::LANGUAGE_UNSUPPORTED)
    );
}

#[test]
fn a_meeting_still_recording_waits() {
    let (_dir, library, runner, events) = start(FakeLoader::default());
    let id = lock(&library)
        .create_meeting("it", ManualClock::START)
        .unwrap()
        .id;
    runner.enqueue(JobKind::Transcribe, id).unwrap();
    let last = events
        .wait_for(|e| e.job.state == JobState::Queued && e.job.attempts == 1)
        .pop()
        .unwrap();
    assert_eq!(
        last.job.error_code.as_deref(),
        Some(error_code::STILL_RECORDING)
    );
    assert_eq!(
        lock(&library).meeting(id).unwrap().status,
        MeetingStatus::Recording
    );
}

#[test]
fn cancelling_a_running_transcription_marks_the_meeting() {
    let gate = Arc::new(AtomicBool::new(false));
    let (_dir, library, runner, events) = start(FakeLoader {
        error: None,
        gate: Some(Arc::clone(&gate)),
    });
    let id = recorded_meeting(&mut lock(&library), "it");
    let job = runner.enqueue(JobKind::Transcribe, id).unwrap();
    events.wait_for(|e| e.job.state == JobState::Running);
    runner.cancel(job.id).unwrap();
    gate.store(true, Ordering::SeqCst);

    let deadline = Instant::now() + PATIENCE;
    loop {
        let meeting = lock(&library).meeting(id).unwrap();
        if meeting.status == MeetingStatus::Failed {
            assert_eq!(meeting.error_code.as_deref(), Some(error_code::CANCELLED));
            break;
        }
        assert!(Instant::now() < deadline, "the meeting was not marked");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        lock(&library).job(job.id).unwrap().state,
        JobState::Cancelled
    );
    assert!(lock(&library).current_version(id).unwrap().is_none());
}

#[test]
fn a_meeting_trashed_while_it_is_transcribed_gets_nothing_and_fails_on_restore() {
    let gate = Arc::new(AtomicBool::new(false));
    let (_dir, library, runner, events) = start(FakeLoader {
        error: None,
        gate: Some(Arc::clone(&gate)),
    });
    let id = recorded_meeting(&mut lock(&library), "it");
    runner.enqueue(JobKind::Transcribe, id).unwrap();
    events.wait_for(|e| e.job.state == JobState::Running);
    lock(&library).trash(id).unwrap();
    gate.store(true, Ordering::SeqCst);

    let last = events.wait_for(finished).pop().unwrap();
    assert_eq!(last.job.state, JobState::Failed, "{:?}", last.job);
    assert_eq!(last.job.attempts, 1, "not retried");
    assert_eq!(
        last.job.error_code.as_deref(),
        Some(error_code::MEETING_DELETED)
    );
    {
        let library = lock(&library);
        assert!(library.current_version(id).unwrap().is_none());
        // Nothing written to the meeting in the trash, not even the failure.
        assert_eq!(
            library.meeting(id).unwrap().status,
            MeetingStatus::Processing
        );
    }

    // No job is left to settle it: restoring it marks it failed, to transcribe again.
    lock(&library).restore(id).unwrap();
    let meeting = lock(&library).meeting(id).unwrap();
    assert_eq!(meeting.status, MeetingStatus::Failed);
    assert_eq!(
        meeting.error_code.as_deref(),
        Some(error_code::MEETING_DELETED)
    );
}

#[test]
fn a_meeting_restored_with_its_transcription_queued_stays_processing() {
    let (_dir, _clock, mut library) = library();
    let id = recorded_meeting(&mut library, "it");
    library.enqueue_job(JobKind::Transcribe, id).unwrap();
    library.trash(id).unwrap();
    library.restore(id).unwrap();
    let meeting = library.meeting(id).unwrap();
    assert_eq!(meeting.status, MeetingStatus::Processing);
    assert_eq!(meeting.error_code, None);
}

#[test]
fn a_track_cut_short_by_a_crash_is_transcribed_up_to_where_it_ends() {
    let (_dir, library, runner, events) = start(FakeLoader::default());
    let id = recorded_meeting(&mut lock(&library), "it");
    // The "others" track loses its last two seconds (silence) and ends inside a
    // sample; its header still declares all of it.
    let path = lock(&library)
        .root()
        .join(Library::meeting_audio_dir(id))
        .join("others.wav");
    let len = std::fs::metadata(&path).unwrap().len();
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(len - 2 * 48_000 * 4 - 3).unwrap();
    drop(file);

    runner.enqueue(JobKind::Transcribe, id).unwrap();
    let last = events.wait_for(finished).pop().unwrap();
    assert_eq!(last.job.state, JobState::Done, "{:?}", last.job);
    let library = lock(&library);
    assert_eq!(library.meeting(id).unwrap().status, MeetingStatus::Ready);
    let version = library.current_version(id).unwrap().unwrap();
    let texts: Vec<_> = library
        .segments(version.id)
        .unwrap()
        .into_iter()
        .map(|s| (s.role, s.text))
        .collect();
    assert_eq!(
        texts,
        [
            (Role::Me, "uno".to_owned()),
            (Role::Others, "due".to_owned())
        ]
    );
}
