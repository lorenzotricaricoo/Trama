//! Audio helpers: load any WAV as 16 kHz mono f32, convert other formats with
//! macOS `afconvert`, write WAV, concatenate, synthesize silence/noise.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const SR: u32 = 16_000;

/// Loads an audio file as 16 kHz mono f32 samples in [-1, 1].
/// Non-WAV files (m4a, mp3, caf, aiff, flac, ...) are converted first with
/// macOS `afconvert` into `scratch`.
pub fn load_16k_mono(path: &Path, scratch: &Path) -> Result<Vec<f32>> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let wav_path: PathBuf = if ext == "wav" {
        path.to_path_buf()
    } else {
        convert_with_afconvert(path, scratch)?
    };
    let (samples, sr) = read_wav_mono(&wav_path)?;
    if sr == SR {
        return Ok(samples);
    }
    let r = sherpa_onnx::LinearResampler::create(sr as i32, SR as i32)
        .context("cannot create resampler")?;
    Ok(r.resample(&samples, true))
}

fn convert_with_afconvert(path: &Path, scratch: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(scratch)?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("audio");
    let out = scratch.join(format!("{stem}.16k.wav"));
    if out.exists() {
        return Ok(out);
    }
    let status = Command::new("afconvert")
        .args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"])
        .arg(path)
        .arg(&out)
        .status()
        .context("afconvert not available (macOS only); convert to 16 kHz mono WAV first")?;
    if !status.success() {
        bail!("afconvert failed on {}", path.display());
    }
    Ok(out)
}

pub fn read_wav_mono(path: &Path) -> Result<(Vec<f32>, u32)> {
    let mut reader =
        hound::WavReader::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let spec = reader.spec();
    let ch = spec.channels as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()?
        }
    };
    let mono = if ch == 1 {
        interleaved
    } else {
        interleaved.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect()
    };
    Ok((mono, spec.sample_rate))
}

pub fn write_wav_16k(path: &Path, samples: &[f32]) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SR,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * 32767.0).round() as i16)?;
    }
    w.finalize()?;
    Ok(())
}

/// Deterministic xorshift noise source (no external RNG crate needed).
struct Rng(u64);
impl Rng {
    fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    }
}

/// kind: "silence" (digital zero), "white" (white noise), "room" (low-passed
/// noise + 50 Hz hum, a crude room-tone/HVAC proxy). `dbfs` is the RMS level.
pub fn synth(kind: &str, seconds: f32, dbfs: f32, seed: u64) -> Result<Vec<f32>> {
    let n = (seconds * SR as f32) as usize;
    let mut rng = Rng(seed.max(1));
    let raw: Vec<f32> = match kind {
        "silence" => return Ok(vec![0.0; n]),
        "white" => (0..n).map(|_| rng.next_f32()).collect(),
        "room" => {
            let mut lp = 0.0f32;
            (0..n)
                .map(|i| {
                    lp = 0.97 * lp + 0.03 * rng.next_f32();
                    let hum = (2.0 * std::f32::consts::PI * 50.0 * i as f32 / SR as f32).sin();
                    lp * 4.0 + 0.3 * hum
                })
                .collect()
        }
        "clicks" => {
            // Keyboard-like: short decaying noise bursts at random times over faint noise.
            let mut env = 0.0f32;
            (0..n)
                .map(|_| {
                    if rng.next_f32() > 0.99985 {
                        env = 1.0;
                    }
                    env *= 0.995;
                    rng.next_f32() * (env + 0.02)
                })
                .collect()
        }
        other => bail!("unknown noise kind {other}"),
    };
    let rms = (raw.iter().map(|x| x * x).sum::<f32>() / n.max(1) as f32).sqrt().max(1e-9);
    let target = 10f32.powf(dbfs / 20.0);
    Ok(raw.into_iter().map(|x| x / rms * target).collect())
}
