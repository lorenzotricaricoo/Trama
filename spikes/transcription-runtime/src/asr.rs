//! Parakeet TDT through sherpa-onnx (ONNX Runtime underneath), with optional
//! Silero VAD chunking for long audio.

use crate::audio::SR;
use anyhow::{Context, Result, bail};
use clap::Args;
use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig, SileroVadModelConfig,
    VadModelConfig, VoiceActivityDetector,
};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Args, Debug, Clone)]
pub struct EngineOpts {
    /// Folder of a sherpa-onnx Parakeet TDT package (encoder/decoder/joiner + tokens.txt).
    #[arg(long, env = "TRAMA_STT_MODEL")]
    pub model: PathBuf,
    /// ONNX Runtime execution provider: cpu or coreml.
    #[arg(long, default_value = "cpu")]
    pub provider: String,
    /// Intra-op threads for the recognizer.
    #[arg(long, default_value_t = 4)]
    pub threads: i32,
    /// Silero VAD model (silero_vad.onnx). Speech is cut into segments at silences
    /// before decoding. Strongly recommended for anything longer than ~20 s.
    #[arg(long, env = "TRAMA_STT_VAD")]
    pub vad: Option<PathBuf>,
    /// Disable VAD even if --vad/TRAMA_STT_VAD is set (decode the whole file at once).
    #[arg(long)]
    pub no_vad: bool,
    /// Without VAD: cut the audio into fixed windows of this many seconds instead of
    /// decoding it whole (0 = whole file). Used to test the model alone on silence.
    #[arg(long, default_value_t = 0.0)]
    pub fixed_chunk_secs: f32,
    /// VAD speech probability threshold.
    #[arg(long, default_value_t = 0.5)]
    pub vad_threshold: f32,
    /// Minimum silence (s) that closes a speech segment.
    #[arg(long, default_value_t = 0.5)]
    pub vad_min_silence: f32,
    /// Maximum segment length (s); longer speech is split.
    #[arg(long, default_value_t = 20.0)]
    pub vad_max_speech: f32,
    /// Audio (ms) added before/after each VAD segment.
    #[arg(long, default_value_t = 200)]
    pub vad_pad_ms: u32,
    /// Peak-normalize the whole file to -1 dBFS before recognition.
    #[arg(long)]
    pub normalize_gain: bool,
}

pub struct Segment {
    pub start: f32,
    pub end: f32,
    pub text: String,
}

pub struct Transcript {
    pub text: String,
    pub segments: Vec<Segment>,
    pub decode_secs: f64,
}

pub struct Engine {
    rec: OfflineRecognizer,
    vad: Option<VadModelConfig>,
    opts: EngineOpts,
    pub load_secs: f64,
    pub model_desc: String,
}

fn pick(dir: &Path, stem: &str) -> Result<String> {
    for name in [format!("{stem}.int8.onnx"), format!("{stem}.onnx"), format!("{stem}.fp16.onnx")] {
        let p = dir.join(&name);
        if p.exists() {
            return Ok(p.to_string_lossy().into_owned());
        }
    }
    bail!("no {stem}*.onnx in {}", dir.display())
}

impl Engine {
    pub fn new(opts: &EngineOpts) -> Result<Self> {
        let t0 = Instant::now();
        let dir = &opts.model;
        let encoder = pick(dir, "encoder")?;
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.transducer = OfflineTransducerModelConfig {
            encoder: Some(encoder.clone()),
            decoder: Some(pick(dir, "decoder")?),
            joiner: Some(pick(dir, "joiner")?),
        };
        config.model_config.tokens = Some(dir.join("tokens.txt").to_string_lossy().into_owned());
        config.model_config.model_type = Some("nemo_transducer".into());
        config.model_config.num_threads = opts.threads;
        config.model_config.provider = Some(opts.provider.clone());
        config.decoding_method = Some("greedy_search".into());
        let rec = OfflineRecognizer::create(&config).context("cannot create recognizer")?;

        let vad = match (&opts.vad, opts.no_vad) {
            (Some(p), false) => Some(VadModelConfig {
                silero_vad: SileroVadModelConfig {
                    model: Some(p.to_string_lossy().into_owned()),
                    threshold: opts.vad_threshold,
                    min_silence_duration: opts.vad_min_silence,
                    min_speech_duration: 0.25,
                    window_size: 512,
                    max_speech_duration: opts.vad_max_speech,
                },
                sample_rate: SR as i32,
                num_threads: 1,
                provider: Some("cpu".into()),
                ..Default::default()
            }),
            _ => None,
        };
        let model_desc = format!(
            "{} ({}) provider={} threads={} vad={} [sherpa-onnx {}, onnxruntime {}]",
            dir.file_name().unwrap_or_default().to_string_lossy(),
            Path::new(&encoder).file_name().unwrap_or_default().to_string_lossy(),
            opts.provider,
            opts.threads,
            if vad.is_some() {
                format!("silero(pad {} ms, min silence {} s)", opts.vad_pad_ms, opts.vad_min_silence)
            } else if opts.fixed_chunk_secs > 0.0 {
                format!("off, fixed {} s windows", opts.fixed_chunk_secs)
            } else {
                "off".into()
            },
            sherpa_onnx::version(),
            sherpa_onnx::onnxruntime_version()
        );
        Ok(Self { rec, vad, opts: opts.clone(), load_secs: t0.elapsed().as_secs_f64(), model_desc })
    }

    fn decode_one(&self, samples: &[f32]) -> String {
        let stream = self.rec.create_stream();
        stream.accept_waveform(SR as i32, samples);
        self.rec.decode(&stream);
        stream.get_result().map(|r| r.text.trim().to_owned()).unwrap_or_default()
    }

    /// Transcribes 16 kHz mono samples.
    pub fn transcribe(&self, input: &[f32]) -> Result<Transcript> {
        let t0 = Instant::now();
        let normalized;
        let samples: &[f32] = if self.opts.normalize_gain {
            let peak = input.iter().fold(0f32, |m, x| m.max(x.abs()));
            let g = if peak > 1e-6 { 0.891 / peak } else { 1.0 };
            normalized = input.iter().map(|x| x * g).collect::<Vec<_>>();
            &normalized
        } else {
            input
        };

        let segments: Vec<(usize, usize)> = match &self.vad {
            None if self.opts.fixed_chunk_secs > 0.0 => {
                let w = (self.opts.fixed_chunk_secs * SR as f32) as usize;
                (0..samples.len()).step_by(w).map(|s| (s, (s + w).min(samples.len()))).collect()
            }
            None => vec![(0, samples.len())],
            Some(cfg) => {
                let buf = (self.opts.vad_max_speech + 10.0).max(30.0);
                let vad = VoiceActivityDetector::create(cfg, buf).context("cannot create VAD")?;
                let mut spans = Vec::new();
                let drain = |vad: &VoiceActivityDetector, spans: &mut Vec<(usize, usize)>| {
                    while let Some(seg) = vad.front() {
                        let s = seg.start().max(0) as usize;
                        spans.push((s, s + seg.n() as usize));
                        vad.pop();
                    }
                };
                for chunk in samples.chunks(512) {
                    vad.accept_waveform(chunk);
                    drain(&vad, &mut spans);
                }
                vad.flush();
                drain(&vad, &mut spans);
                let pad = (self.opts.vad_pad_ms * SR / 1000) as usize;
                spans
                    .into_iter()
                    .map(|(s, e)| (s.saturating_sub(pad), (e + pad).min(samples.len())))
                    .collect()
            }
        };

        let mut out = Vec::new();
        for (s, e) in segments {
            if e <= s {
                continue;
            }
            let text = self.decode_one(&samples[s..e]);
            out.push(Segment { start: s as f32 / SR as f32, end: e as f32 / SR as f32, text });
        }
        let text = out.iter().map(|s| s.text.as_str()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ");
        Ok(Transcript { text, segments: out, decode_secs: t0.elapsed().as_secs_f64() })
    }
}

/// Peak resident set size of this process so far, in MiB (not measured on Windows).
#[cfg(not(unix))]
pub fn peak_rss_mib() -> f64 {
    f64::NAN
}

/// Peak resident set size of this process so far, in MiB.
#[cfg(unix)]
pub fn peak_rss_mib() -> f64 {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
    let v = ru.ru_maxrss as f64;
    if cfg!(target_os = "macos") { v / 1024.0 / 1024.0 } else { v / 1024.0 }
}
