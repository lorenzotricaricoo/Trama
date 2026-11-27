//! trama-stt: throwaway spike CLI for issue #10 (Parakeet via sherpa-onnx from Rust).

mod asr;
mod audio;
mod wer;

use anyhow::{Context, Result, bail};
use asr::{Engine, EngineOpts, peak_rss_mib};
use clap::{Parser, Subcommand};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(about = "Parakeet TDT via sherpa-onnx: transcribe, evaluate WER, make test audio")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Transcribe audio files and print text plus speed/memory stats.
    Transcribe {
        #[command(flatten)]
        engine: EngineOpts,
        /// Print VAD segments with timestamps.
        #[arg(long)]
        segments: bool,
        files: Vec<PathBuf>,
    },
    /// Evaluate a folder: every audio file `X.<ext>` with a reference `X.txt`
    /// (or `X.ref.txt`). An empty reference marks a silence/noise-only file:
    /// its words are counted as invented words, not in the WER.
    Eval {
        #[command(flatten)]
        engine: EngineOpts,
        dir: PathBuf,
        /// Output folder for results.tsv and alignments.txt
        /// (default: ~/Library/Caches/trama-spikes/eval-out/<dir name>).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Score existing transcripts (no model): every `X<suffix>` hypothesis against `X.txt`,
    /// with the same normalization as `eval`. Use it to re-score the original
    /// benchmark outputs so both numbers are computed the same way.
    Score {
        dir: PathBuf,
        #[arg(long, default_value = ".hyp.txt")]
        suffix: String,
    },
    /// Concatenate WAV files (resampled to 16 kHz mono) with a gap of silence.
    Concat {
        #[arg(long, default_value_t = 800)]
        gap_ms: u32,
        #[arg(short, long)]
        output: PathBuf,
        inputs: Vec<PathBuf>,
    },
    /// Write a 16 kHz mono WAV of silence or noise.
    Synth {
        /// silence | white | room
        #[arg(long)]
        kind: String,
        #[arg(long, default_value_t = 300.0)]
        seconds: f32,
        /// RMS level in dBFS (ignored for silence).
        #[arg(long, default_value_t = -60.0, allow_hyphen_values = true)]
        dbfs: f32,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        #[arg(short, long)]
        output: PathBuf,
    },
}

const AUDIO_EXT: &[&str] = &["wav", "m4a", "mp3", "aac", "caf", "aif", "aiff", "flac", "mp4"];

fn cache_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
        .join("Library/Caches/trama-spikes")
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Transcribe { engine, segments, files } => transcribe(&engine, segments, &files),
        Cmd::Eval { engine, dir, out } => eval(&engine, &dir, out),
        Cmd::Score { dir, suffix } => score(&dir, &suffix),
        Cmd::Concat { gap_ms, output, inputs } => {
            let scratch = cache_dir().join("convert");
            let gap = vec![0.0f32; (gap_ms * audio::SR / 1000) as usize];
            let mut all = Vec::new();
            for (i, p) in inputs.iter().enumerate() {
                if i > 0 {
                    all.extend_from_slice(&gap);
                }
                all.extend(audio::load_16k_mono(p, &scratch)?);
            }
            audio::write_wav_16k(&output, &all)?;
            eprintln!("wrote {} ({:.1} s)", output.display(), all.len() as f32 / audio::SR as f32);
            Ok(())
        }
        Cmd::Synth { kind, seconds, dbfs, seed, output } => {
            audio::write_wav_16k(&output, &audio::synth(&kind, seconds, dbfs, seed)?)
        }
    }
}

fn transcribe(opts: &EngineOpts, show_segments: bool, files: &[PathBuf]) -> Result<()> {
    let engine = Engine::new(opts)?;
    eprintln!("model: {}  (load {:.2} s)", engine.model_desc, engine.load_secs);
    let scratch = cache_dir().join("convert");
    for f in files {
        let samples = audio::load_16k_mono(f, &scratch)?;
        let dur = samples.len() as f64 / audio::SR as f64;
        let t = engine.transcribe(&samples)?;
        if show_segments {
            for s in &t.segments {
                println!("[{:8.2} - {:8.2}] {}", s.start, s.end, s.text);
            }
        }
        println!("{}", t.text);
        eprintln!(
            "{}: audio {:.1} s, decode {:.2} s, RTF {:.4} ({:.1}x real time), segments {}, words {}, peak RSS {:.0} MiB",
            f.display(),
            dur,
            t.decode_secs,
            t.decode_secs / dur,
            dur / t.decode_secs,
            t.segments.len(),
            wer::normalize(&t.text).len(),
            peak_rss_mib()
        );
    }
    Ok(())
}

fn score(dir: &Path, suffix: &str) -> Result<()> {
    let mut hyps: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(suffix))
        .collect();
    hyps.sort();
    let mut total = wer::Counts::default();
    for h in &hyps {
        let name = h.file_name().unwrap().to_string_lossy();
        let stem = name.trim_end_matches(suffix);
        let r = dir.join(format!("{stem}.txt"));
        let Ok(reference) = std::fs::read_to_string(&r) else {
            eprintln!("skip {name} (no {stem}.txt)");
            continue;
        };
        let (c, _) = wer::align(&wer::normalize(&reference), &wer::normalize(&std::fs::read_to_string(h)?));
        total.add(&c);
        let w = c.wer().map(|w| format!("{w:.2}")).unwrap_or_else(|| "-".into());
        println!("{stem:<40} N={:<6} S={:<4} D={:<4} I={:<4} WER {w}%", c.n, c.sub, c.del, c.ins);
    }
    let w = total.wer().map(|w| format!("{w:.2}%")).unwrap_or_else(|| "-".into());
    println!("OVERALL {w}  [N={} S={} D={} I={}]", total.n, total.sub, total.del, total.ins);
    Ok(())
}

fn find_reference(audio: &Path) -> Option<PathBuf> {
    let stem = audio.file_stem()?.to_str()?;
    let dir = audio.parent()?;
    [format!("{stem}.txt"), format!("{stem}.ref.txt")]
        .into_iter()
        .map(|n| dir.join(n))
        .find(|p| p.exists())
}

fn eval(opts: &EngineOpts, dir: &Path, out: Option<PathBuf>) -> Result<()> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| AUDIO_EXT.contains(&e.to_lowercase().as_str()))
        })
        .collect();
    files.sort();
    if files.is_empty() {
        bail!("no audio files in {}", dir.display());
    }
    let dir_name = dir.canonicalize()?.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let out = out.unwrap_or_else(|| cache_dir().join("eval-out").join(&dir_name));
    std::fs::create_dir_all(&out)?;
    let scratch = cache_dir().join("convert").join(&dir_name);

    let engine = Engine::new(opts)?;
    eprintln!("model: {}  (load {:.2} s)", engine.model_desc, engine.load_secs);

    let mut tsv = String::from("file\taudio_s\tdecode_s\tref_words\tsub\tdel\tins\twer_pct\n");
    let mut aligned = String::new();
    let mut total = wer::Counts::default();
    let (mut audio_s, mut decode_s) = (0.0f64, 0.0f64);
    let (mut silence_files, mut silence_s, mut invented) = (0usize, 0.0f64, 0usize);

    println!("{:<40} {:>8} {:>6} {:>5} {:>4} {:>4} {:>4} {:>7}", "file", "audio_s", "RTF", "N", "S", "D", "I", "WER%");
    for f in &files {
        let Some(ref_path) = find_reference(f) else {
            eprintln!("skip {} (no reference .txt)", f.display());
            continue;
        };
        let reference = std::fs::read_to_string(&ref_path)?;
        let samples = audio::load_16k_mono(f, &scratch)?;
        let dur = samples.len() as f64 / audio::SR as f64;
        let t = engine.transcribe(&samples)?;
        let r = wer::normalize(&reference);
        let h = wer::normalize(&t.text);
        let (c, ops) = wer::align(&r, &h);
        let name = f.file_name().unwrap().to_string_lossy();
        let stem = f.file_stem().unwrap().to_string_lossy();
        std::fs::write(out.join(format!("{stem}.hyp.txt")), &t.text)?;
        audio_s += dur;
        decode_s += t.decode_secs;
        if r.is_empty() {
            silence_files += 1;
            silence_s += dur;
            invented += h.len();
        } else {
            total.add(&c);
        }
        let wer_s = c.wer().map(|w| format!("{w:.2}")).unwrap_or_else(|| "-".into());
        println!(
            "{:<40} {:>8.1} {:>6.3} {:>5} {:>4} {:>4} {:>4} {:>7}",
            name, dur, t.decode_secs / dur, c.n, c.sub, c.del, c.ins, wer_s
        );
        writeln!(tsv, "{name}\t{dur:.2}\t{:.3}\t{}\t{}\t{}\t{}\t{wer_s}", t.decode_secs, c.n, c.sub, c.del, c.ins)?;
        writeln!(aligned, "### {name}  WER {wer_s}%\nREF: {reference}\nHYP: {}\nALN: {}\n", t.text.trim(), wer::render(&ops))?;
    }
    let overall = total.wer().map(|w| format!("{w:.2}%")).unwrap_or_else(|| "-".into());
    let summary = format!(
        "\nOVERALL (corpus WER over files with non-empty reference): {overall}  \
         [N={} S={} D={} I={}]\n\
         Silence/noise-only files: {silence_files} ({silence_s:.0} s) -> invented words: {invented}\n\
         Speed: {audio_s:.1} s audio in {decode_s:.1} s = {:.1}x real time (RTF {:.4}); load {:.1} s; peak RSS {:.0} MiB\n\
         Model: {}\n",
        total.n, total.sub, total.del, total.ins,
        audio_s / decode_s.max(1e-9), decode_s / audio_s.max(1e-9),
        engine.load_secs, peak_rss_mib(), engine.model_desc
    );
    print!("{summary}");
    tsv.push_str(&format!("# {}", summary.trim().replace('\n', "\n# ")));
    std::fs::write(out.join("results.tsv"), tsv)?;
    std::fs::write(out.join("alignments.txt"), aligned)?;
    eprintln!("wrote {}/results.tsv and alignments.txt", out.display());
    Ok(())
}
