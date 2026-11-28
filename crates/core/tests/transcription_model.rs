//! Regression tests with the real model, ignored by default: they need the 2.5 GB
//! transcription model and, the first time, a 658 MB download of public test audio.
//!
//! ```sh
//! TRAMA_MODELS=/path/to/models cargo test --release -p trama-core \
//!     --test transcription_model -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `TRAMA_MODELS` is a models folder with the verified transcription model, as the app
//! downloads it (see `docs/architecture/models.md`). Test audio is cached in
//! `TRAMA_FIXTURES`, by default `target/fixtures`.
//!
//! The audio is FLEURS (Google, CC-BY-4.0): the first 200 utterances of the Italian
//! test split, 47.8 minutes read by several speakers, the same set spike #10 measured
//! (2.15% word error rate). `TRAMA_FLEURS_CLIPS` changes how many are used.

mod common;

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use trama_core::jobs::{JobEvent, Runner, Transcribe, lock};
use trama_core::models::{ModelState, ModelStore, transcription_model};
use trama_core::storage::*;
use trama_core::transcription::*;
use unicode_normalization::UnicodeNormalization;

/// Word error rate the model must stay at or below on the FLEURS check (issue #26).
const MAX_WER_PERCENT: f64 = 2.5;

/// FLEURS on Hugging Face, pinned to a commit.
const FLEURS_BASE: &str = "https://huggingface.co/datasets/google/fleurs/resolve/70bb2e84b976b7e960aa89f1c648e09c59f894dd/data/it_it";
const FLEURS_TSV_SHA256: &str = "af1a6d7295547d283441e2ea2e6f24f41d9b26098b47fe3d588930cd053c7b08";
const FLEURS_AUDIO_SHA256: &str =
    "97dbaedbfa52f4fa4a7b380be90b343066330b8526e1a88f5ab9f82f71f1758e";
const FLEURS_DEFAULT_CLIPS: usize = 200;

// --- Setup ---

fn models_dir() -> PathBuf {
    let Some(dir) = std::env::var_os("TRAMA_MODELS") else {
        panic!("set TRAMA_MODELS to a models folder with the verified transcription model");
    };
    let dir = PathBuf::from(dir);
    let status = ModelStore::new(&dir, transcription_model())
        .status()
        .unwrap();
    assert_eq!(
        status.state,
        ModelState::Ready,
        "the model in {} is not ready: download it first (docs/architecture/models.md)",
        dir.display()
    );
    dir
}

fn fixtures_dir() -> PathBuf {
    std::env::var_os("TRAMA_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures"))
}

/// One FLEURS utterance.
struct Clip {
    wav: PathBuf,
    reference: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The first `count` utterances of the Italian test split, downloaded once (checksums
/// verified) and cached.
fn fleurs(count: usize) -> Vec<Clip> {
    let dir = fixtures_dir().join("fleurs-it-test");
    std::fs::create_dir_all(&dir).unwrap();
    let tsv_path = dir.join("test.tsv");
    if !tsv_path.exists() {
        let mut body = Vec::new();
        ureq::get(&format!("{FLEURS_BASE}/test.tsv"))
            .call()
            .unwrap()
            .into_body()
            .into_reader()
            .read_to_end(&mut body)
            .unwrap();
        assert_eq!(sha256_hex(&body), FLEURS_TSV_SHA256, "test.tsv changed");
        std::fs::write(&tsv_path, body).unwrap();
    }
    let tsv = std::fs::read_to_string(&tsv_path).unwrap();
    assert_eq!(sha256_hex(tsv.as_bytes()), FLEURS_TSV_SHA256);
    // Columns: id, file, raw transcription, normalized transcription, …
    let clips: Vec<Clip> = tsv
        .lines()
        .take(count)
        .map(|line| {
            let cols: Vec<&str> = line.split('\t').collect();
            Clip {
                wav: dir.join(cols[1]),
                reference: cols[2].to_owned(),
            }
        })
        .collect();

    let missing: HashSet<String> = clips
        .iter()
        .filter(|clip| !clip.wav.exists())
        .map(|clip| clip.wav.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    if !missing.is_empty() {
        extract_fleurs_audio(&dir, &missing);
    }
    clips
}

/// Streams the audio archive (658 MB), keeping only the wanted files, and checks the
/// archive's SHA-256 at the end; on a mismatch the extracted files are deleted.
fn extract_fleurs_audio(dir: &Path, wanted: &HashSet<String>) {
    eprintln!(
        "downloading the FLEURS Italian test audio (658 MB) for {} clips…",
        wanted.len()
    );
    struct Hashing<R> {
        inner: R,
        hasher: Sha256,
    }
    impl<R: Read> Read for Hashing<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.hasher.update(&buf[..n]);
            Ok(n)
        }
    }
    let response = ureq::get(&format!("{FLEURS_BASE}/audio/test.tar.gz"))
        .call()
        .unwrap();
    let mut reader = Hashing {
        inner: response
            .into_body()
            .into_with_config()
            .limit(u64::MAX)
            .reader(),
        hasher: Sha256::new(),
    };
    let mut written = Vec::new();
    {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&mut reader));
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().into_owned();
            let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            if wanted.contains(&name) {
                let target = dir.join(&name);
                entry.unpack(&target).unwrap();
                written.push(target);
            }
        }
    }
    // Read whatever follows the end of the tar stream, so the whole file is hashed.
    std::io::copy(&mut reader, &mut std::io::sink()).unwrap();
    let actual: String = reader
        .hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != FLEURS_AUDIO_SHA256 {
        for file in written {
            let _ = std::fs::remove_file(file);
        }
        panic!("the FLEURS audio archive does not match its checksum: {actual}");
    }
}

// --- Word error rate, normalized as in spike #10 ---

/// NFKC, lowercase, every character that is not a letter or a digit becomes a space.
/// Accents are kept; numbers are not spelled out.
fn words(text: &str) -> Vec<String> {
    let lowered = text.nfkc().collect::<String>().to_lowercase();
    lowered
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// Word edits (substitutions, deletions, insertions) from `reference` to `hypothesis`,
/// and the alignment with errors marked `{ref→hyp}`.
fn edits(reference: &[String], hypothesis: &[String]) -> (usize, String) {
    let (n, m) = (reference.len(), hypothesis.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let sub = d[i - 1][j - 1] + usize::from(reference[i - 1] != hypothesis[j - 1]);
            d[i][j] = sub.min(d[i - 1][j] + 1).min(d[i][j - 1] + 1);
        }
    }
    let (mut i, mut j) = (n, m);
    let mut ops = Vec::new();
    while i > 0 || j > 0 {
        if i > 0
            && j > 0
            && d[i][j] == d[i - 1][j - 1] + usize::from(reference[i - 1] != hypothesis[j - 1])
        {
            ops.push(if reference[i - 1] == hypothesis[j - 1] {
                reference[i - 1].clone()
            } else {
                format!("{{{}→{}}}", reference[i - 1], hypothesis[j - 1])
            });
            i -= 1;
            j -= 1;
        } else if i > 0 && d[i][j] == d[i - 1][j] + 1 {
            ops.push(format!("{{{}→∅}}", reference[i - 1]));
            i -= 1;
        } else {
            ops.push(format!("{{∅→{}}}", hypothesis[j - 1]));
            j -= 1;
        }
    }
    ops.reverse();
    (d[n][m], ops.join(" "))
}

#[test]
fn word_error_rate_is_counted_like_the_spike() {
    assert_eq!(
        words("L'azienda, OK? Follow-up è pronto."),
        ["l", "azienda", "ok", "follow", "up", "è", "pronto"]
    );
    let (errors, alignment) = edits(
        &words("il deploy di oggi è pronto"),
        &words("il deployment oggi è pronto davvero"),
    );
    assert_eq!(errors, 3);
    assert!(alignment.starts_with("il {"), "{alignment}");
    assert!(
        alignment.ends_with("oggi è pronto {∅→davvero}"),
        "{alignment}"
    );
}

// --- Helpers with the real engine ---

fn transcribe_samples(engine: &mut dyn Engine, samples: &[f32]) -> Vec<SpokenSegment> {
    let mut detector = engine.detector().unwrap();
    let settings = engine.settings();
    transcribe_track(
        &mut SliceSource::new(samples, 8_000),
        detector.as_mut(),
        engine.recognizer(),
        &settings,
        &mut |_| {},
        &|| false,
    )
    .unwrap()
}

fn read_clip(path: &Path) -> Vec<f32> {
    let mut source = WavSource::open(path).unwrap();
    let mut samples = Vec::new();
    while source.read(&mut samples).unwrap() {}
    samples
}

fn gigabytes(bytes: Option<u64>) -> String {
    bytes.map_or("unknown".into(), |b| format!("{:.2} GB", b as f64 / 1e9))
}

// --- The checks of issue #26 ---

#[test]
#[ignore = "needs the transcription model (TRAMA_MODELS) and downloads 658 MB of test audio"]
fn fleurs_italian_word_error_rate_stays_under_the_threshold() {
    let count = std::env::var("TRAMA_FLEURS_CLIPS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(FLEURS_DEFAULT_CLIPS);
    let clips = fleurs(count);
    let memory = MemoryMonitor::start(Duration::from_millis(100));
    let loaded = Instant::now();
    let mut engine = SherpaLoader::new(models_dir()).load_engine().unwrap();
    let load_time = loaded.elapsed();

    let (mut errors, mut total, mut audio_s) = (0, 0, 0.0);
    let decoding = Instant::now();
    for clip in &clips {
        let samples = read_clip(&clip.wav);
        audio_s += samples.len() as f64 / f64::from(SAMPLE_RATE);
        let hypothesis: Vec<String> = transcribe_samples(&mut engine, &samples)
            .into_iter()
            .map(|s| s.text)
            .collect();
        let reference = words(&clip.reference);
        let (e, alignment) = edits(&reference, &words(&hypothesis.join(" ")));
        if e > 0 {
            eprintln!(
                "{}: {e} errors: {alignment}",
                clip.wav.file_name().unwrap().to_string_lossy()
            );
        }
        errors += e;
        total += reference.len();
    }
    let elapsed = decoding.elapsed().as_secs_f64();
    let wer = 100.0 * errors as f64 / total as f64;
    eprintln!(
        "FLEURS it, {} clips, {:.1} min, {total} words: WER {wer:.2}% ({errors} errors); \
         {:.1}x real time; model load {:.1} s; peak memory {}",
        clips.len(),
        audio_s / 60.0,
        audio_s / elapsed,
        load_time.as_secs_f64(),
        gigabytes(memory.finish()),
    );
    assert!(
        wer <= MAX_WER_PERCENT,
        "WER {wer:.2}% above {MAX_WER_PERCENT}%"
    );
}

/// Deterministic noise (xorshift), scaled to an RMS level in dBFS.
fn noise(seconds: f32, dbfs: f32, seed: u64, low_pass: bool) -> Vec<f32> {
    let n = (seconds * SAMPLE_RATE as f32) as usize;
    let mut state = seed.max(1);
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    };
    let mut filtered = 0.0f32;
    let raw: Vec<f32> = (0..n)
        .map(|i| {
            if low_pass {
                // Room tone: low-passed noise and mains hum.
                filtered = 0.97 * filtered + 0.03 * next();
                let hum = (2.0 * std::f32::consts::PI * 50.0 * i as f32 / 16_000.0).sin();
                filtered * 4.0 + 0.3 * hum
            } else {
                next()
            }
        })
        .collect();
    let rms = (raw.iter().map(|x| x * x).sum::<f32>() / n as f32).sqrt();
    let target = 10f32.powf(dbfs / 20.0);
    raw.into_iter().map(|x| x / rms * target).collect()
}

#[test]
#[ignore = "needs the transcription model (TRAMA_MODELS)"]
fn silence_and_noise_produce_no_words() {
    let mut engine = SherpaLoader::new(models_dir()).load_engine().unwrap();
    let cases: Vec<(&str, Vec<f32>)> = vec![
        ("digital silence", vec![0.0; 60 * SAMPLE_RATE as usize]),
        ("white noise -60 dBFS", noise(60.0, -60.0, 1, false)),
        ("white noise -40 dBFS", noise(60.0, -40.0, 2, false)),
        ("room tone -50 dBFS", noise(60.0, -50.0, 3, true)),
        ("room tone -35 dBFS", noise(60.0, -35.0, 4, true)),
    ];
    for (name, samples) in cases {
        let segments = transcribe_samples(&mut engine, &samples);
        let text: Vec<_> = segments.iter().map(|s| s.text.as_str()).collect();
        eprintln!("{name}: {} words", words(&text.join(" ")).len());
        assert!(segments.is_empty(), "{name}: invented {text:?}");
    }
}

/// Writes 16 kHz samples as a 48 kHz mono 32-bit float WAV, like the recorder.
fn write_track(path: &Path, samples: &[f32]) {
    let resampler = sherpa_onnx::LinearResampler::create(16_000, 48_000).unwrap();
    let upsampled = resampler.resample(samples, true);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for sample in upsampled {
        writer.write_sample(sample).unwrap();
    }
    writer.finalize().unwrap();
}

/// A timing sidecar for a 48 kHz track that starts `offset_ms` after the recording.
fn write_sidecar(path: &Path, offset_ms: u64, frames: u64) {
    let origin = 1_000_000_000u64;
    let host = |frame: u64| origin + offset_ms * 1_000_000 + frame * 1_000_000_000 / 48_000;
    let mut text = format!(
        "{{\"t\":\"start\",\"version\":1,\"source\":\"system\",\"sample_rate\":48000,\"channels\":1,\"recording_start_ns\":{origin},\"unix_ms\":0}}\n"
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
#[ignore = "needs the transcription model (TRAMA_MODELS) and the FLEURS test audio"]
fn a_recorded_meeting_gets_a_two_track_transcript() {
    let models = models_dir();
    let clips = fleurs(12);
    let silence = |s: f32| vec![0.0f32; (s * SAMPLE_RATE as f32) as usize];

    let (_dir, _clock, mut library) = common::library();
    let id = library.create_meeting("it", 0).unwrap().id;
    let relative = Library::meeting_audio_dir(id);
    let audio = library.root().join(&relative);
    std::fs::create_dir_all(&audio).unwrap();

    // "me" speaks first, from 1 s; "others" (a track that started 3 s into the
    // recording) answers 2 s after the end, then "me" again.
    // Three different sentences (FLEURS has several readings of each).
    let first = &clips[0];
    let second = clips
        .iter()
        .find(|c| c.reference != first.reference)
        .unwrap();
    let third = clips
        .iter()
        .find(|c| c.reference != first.reference && c.reference != second.reference)
        .unwrap();
    let (me_clips, others_clip) = ([first, third], second);
    let a = read_clip(&me_clips[0].wav);
    let b = read_clip(&others_clip.wav);
    let c = read_clip(&me_clips[1].wav);
    let rate = SAMPLE_RATE as f32;
    let a_end = 1.0 + a.len() as f32 / rate;
    let b_start = a_end + 2.0;
    let b_end = b_start + b.len() as f32 / rate;
    let c_start = b_end + 2.0;
    let c_len = c.len() as f32 / rate;
    let me = [silence(1.0), a, silence(c_start - a_end), c, silence(2.0)].concat();
    let others = [silence(b_start - 3.0), b, silence(4.0)].concat();
    write_track(&audio.join("me.wav"), &me);
    write_track(&audio.join("others.wav"), &others);
    write_sidecar(
        &audio.join("others.timing.jsonl"),
        3_000,
        others.len() as u64 * 3,
    );
    for (role, timing) in [
        (Role::Me, None),
        (Role::Others, Some("others.timing.jsonl")),
    ] {
        library
            .add_track(
                id,
                &NewTrack {
                    role,
                    audio_path: relative.join(format!("{}.wav", role.as_str())),
                    timing_path: timing.map(|t| relative.join(t)),
                    device: None,
                },
            )
            .unwrap();
    }
    library
        .set_meeting_status(id, MeetingStatus::Processing, None)
        .unwrap();

    let library = Arc::new(Mutex::new(library));
    let (tx, rx) = std::sync::mpsc::channel::<JobEvent>();
    let tx = Mutex::new(tx);
    let runner = Runner::start(
        Arc::clone(&library),
        vec![Arc::new(Transcribe::new(SherpaLoader::new(models)))],
        Arc::new(move |event: JobEvent| {
            let _ = tx.lock().unwrap().send(event);
        }),
    );
    runner.enqueue(JobKind::Transcribe, id).unwrap();
    let mut progress = Vec::new();
    let done = loop {
        let event = rx.recv_timeout(Duration::from_secs(600)).unwrap();
        if let Some(p) = event.progress {
            progress.push(p);
        }
        if !event.job.is_active() && event.progress.is_none() {
            break event.job;
        }
    };
    assert_eq!(done.state, JobState::Done, "{done:?}");
    assert!(progress.len() > 3, "{progress:?}");

    let library = lock(&library);
    assert_eq!(library.meeting(id).unwrap().status, MeetingStatus::Ready);
    let version = library.current_version(id).unwrap().unwrap();
    assert!(version.engine.unwrap().contains("parakeet"));
    let segments = library.segments(version.id).unwrap();
    for s in &segments {
        eprintln!(
            "[{:>6} ms – {:>6} ms] {}: {}",
            s.start_ms,
            s.end_ms,
            s.role.as_str(),
            s.text
        );
    }
    let roles: Vec<Role> = segments.iter().map(|s| s.role).collect();
    assert_eq!(roles.first(), Some(&Role::Me));
    assert!(roles.contains(&Role::Others));
    assert_eq!(roles.last(), Some(&Role::Me));
    // Each turn lies where its clip was placed on the recording's time line (the clips
    // have some silence of their own at both ends). Without the sidecar, the `others`
    // turn would land 3 s early, before its window.
    let within = |s: &Segment, from: f32, to: f32| {
        s.start_ms as f32 >= from * 1000.0 && s.end_ms as f32 <= to * 1000.0
    };
    for s in &segments {
        let ok = match s.role {
            Role::Others => within(s, b_start, b_end),
            Role::Me => within(s, 1.0, a_end) || within(s, c_start, c_start + c_len),
        };
        assert!(ok, "{s:?} is outside its clip");
    }
    // And the text is right.
    let said = |role: Role| {
        segments
            .iter()
            .filter(|s| s.role == role)
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let me_reference = format!("{} {}", me_clips[0].reference, me_clips[1].reference);
    let (me_errors, _) = edits(&words(&me_reference), &words(&said(Role::Me)));
    let (others_errors, _) = edits(&words(&others_clip.reference), &words(&said(Role::Others)));
    assert!(me_errors <= 2, "me: {}", said(Role::Me));
    assert!(others_errors <= 2, "others: {}", said(Role::Others));
}
