//! Manual checks of audio capture on real devices (see `docs/development/audio-capture.md`).
//!
//! ```text
//! cargo run -p trama-core --example capture -- devices
//! cargo run -p trama-core --example capture -- permissions
//! cargo run -p trama-core --example capture -- record LIBRARY [--seconds N] [--no-mic]
//!                                              [--no-system] [--mic-device ID]
//! cargo run -p trama-core --example capture -- analyze LIBRARY/audio/<meeting id> [--clicks]
//! cargo run -p trama-core --example capture -- repair LIBRARY <meeting id>
//! cargo run -p trama-core --example capture -- clicks FILE.wav --seconds N
//! ```
//!
//! Permissions belong to the process that runs this: from a terminal, the terminal's.
//! Recordings stay where you put them; never in the repository.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("the capture example needs macOS");
}

#[cfg(target_os = "macos")]
fn main() {
    if let Err(err) = mac::run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use trama_core::capture::macos::MacBackend;
    use trama_core::capture::wav::{self, WavWriter};
    use trama_core::capture::*;
    use trama_core::storage::Library;

    type Error = Box<dyn std::error::Error>;

    fn arg(args: &[String], name: &str) -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    }

    fn flag(args: &[String], name: &str) -> bool {
        args.iter().any(|a| a == name)
    }

    pub fn run() -> Result<(), Error> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let positional = |i: usize| -> Result<String, Error> {
            args.get(i)
                .cloned()
                .ok_or_else(|| "missing argument".into())
        };
        match args.first().map(String::as_str) {
            Some("devices") => devices(),
            Some("permissions") => permissions(),
            Some("record") => record(Path::new(&positional(1)?), &args),
            Some("analyze") => analyze(Path::new(&positional(1)?), flag(&args, "--clicks")),
            Some("repair") => {
                let id = positional(2)?.parse()?;
                for (path, info) in repair_meeting_audio(Path::new(&positional(1)?), id)? {
                    println!("{}: {} ms", path.display(), info.duration_ms());
                }
                Ok(())
            }
            Some("clicks") => {
                let seconds: u32 = arg(&args, "--seconds").unwrap_or("60".into()).parse()?;
                clicks(Path::new(&positional(1)?), seconds)
            }
            _ => {
                eprintln!(
                    "{}",
                    include_str!("capture.rs")
                        .lines()
                        .skip(2)
                        .take(9)
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                Ok(())
            }
        }
    }

    fn devices() -> Result<(), Error> {
        let backend = MacBackend::new();
        for source in Source::ALL {
            println!(
                "default {:?}: {:?}",
                source,
                backend.default_device(source)?
            );
        }
        for device in backend.input_devices()? {
            println!("input: {:?}  (--mic-device {:?})", device.name, device.id);
        }
        Ok(())
    }

    fn permissions() -> Result<(), Error> {
        let backend = MacBackend::new();
        println!("microphone: {:?}", backend.microphone_permission());
        println!(
            "system audio (probe): {:?}",
            backend.probe_system_audio_permission()
        );
        Ok(())
    }

    /// Prints events as they happen and the levels once a second.
    struct Printer {
        start: Instant,
        levels: Mutex<[(f32, f32); 2]>,
    }

    impl CaptureObserver for Printer {
        fn level(&self, level: Level) {
            let mut levels = self.levels.lock().unwrap();
            let i = level.source as usize;
            levels[i].0 = levels[i].0.max(level.peak_dbfs());
            levels[i].1 = level.rms_dbfs();
        }
        fn event(&self, event: CaptureEvent) {
            eprintln!("[{:>7.1} s] {event:?}", self.start.elapsed().as_secs_f64());
        }
    }

    fn record(library_root: &Path, args: &[String]) -> Result<(), Error> {
        let seconds: f64 = arg(args, "--seconds").unwrap_or("30".into()).parse()?;
        let mut library = Library::open(library_root)?;
        let started_at = library.now();
        let meeting = library.create_meeting("en", started_at)?.id;
        let mut options = RecordingOptions::new(library_root, meeting);
        options.microphone = !flag(args, "--no-mic");
        options.system_audio = !flag(args, "--no-system");
        options.microphone_device = arg(args, "--mic-device");
        let printer = Arc::new(Printer {
            start: Instant::now(),
            levels: Mutex::new([(-120.0, -120.0); 2]),
        });
        let recording = Recording::start(Arc::new(MacBackend::new()), options, printer.clone())?;
        recording.register_tracks(&mut library)?;
        let dir = library_root.join(Library::meeting_audio_dir(meeting));
        eprintln!(
            "recording {seconds} s into {} (Ctrl-C simulates a crash)",
            dir.display()
        );
        let end = Instant::now() + Duration::from_secs_f64(seconds);
        while Instant::now() < end {
            std::thread::sleep(Duration::from_secs(1));
            let mut levels = printer.levels.lock().unwrap();
            eprintln!(
                "[{:>7.1} s] me peak {:>6.1} dBFS rms {:>6.1} | others peak {:>6.1} dBFS rms {:>6.1}",
                printer.start.elapsed().as_secs_f64(),
                levels[0].0,
                levels[0].1,
                levels[1].0,
                levels[1].1
            );
            *levels = [(-120.0, -120.0); 2];
        }
        let summary = recording.stop()?;
        println!("{summary:#?}");
        println!(
            "\nanalyze with: cargo run -p trama-core --example capture -- analyze {}",
            dir.display()
        );
        Ok(())
    }

    fn analyze(dir: &Path, with_clicks: bool) -> Result<(), Error> {
        let mut tracks = Vec::new();
        for stem in ["me", "others"] {
            let wav_path = dir.join(format!("{stem}.wav"));
            if !wav_path.exists() {
                continue;
            }
            let info = wav::inspect(&wav_path)?;
            let timeline = Timeline::load(&dir.join(format!("{stem}.timing.jsonl")))?;
            let (format, samples) = wav::read(&wav_path)?;
            let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
            let rms = (samples
                .iter()
                .map(|s| f64::from(*s) * f64::from(*s))
                .sum::<f64>()
                / samples.len().max(1) as f64)
                .sqrt();
            println!(
                "== {stem}: {} Hz, {} ch",
                format.sample_rate, format.channels
            );
            println!(
                "   {} ms in the file, {} ms counted by the header{}",
                info.duration_ms(),
                info.header_frames * 1000 / u64::from(format.sample_rate),
                if info.header_frames < info.file_frames {
                    "  <- run `repair`"
                } else {
                    ""
                }
            );
            println!(
                "   peak {peak:.4}, rms {rms:.5}{}",
                if peak == 0.0 { "  <- SILENT" } else { "" }
            );
            println!("   first audio at {:?} ms", timeline.first_audio_ms());
            println!("   clock drift vs host: {:?} ppm", timeline.drift_ppm());
            for line in timeline.lines() {
                match line {
                    TimingLine::Segment {
                        device_name,
                        sample_rate,
                        frame,
                        ..
                    } => println!(
                        "   stream: {device_name} @ {sample_rate} Hz from {:.1} s",
                        timeline.recording_ms(*frame as f64) / 1e3
                    ),
                    TimingLine::Pad {
                        frames,
                        reason,
                        frame,
                    } => println!(
                        "   silence ({reason}): {:.1} ms at {:.1} s",
                        *frames as f64 * 1e3 / f64::from(format.sample_rate),
                        timeline.recording_ms(*frame as f64) / 1e3
                    ),
                    TimingLine::Interrupted { code, .. } => println!("   interrupted: {code}"),
                    _ => {}
                }
            }
            tracks.push((stem, timeline, format, samples));
        }
        if let [
            (_, me, me_format, me_samples),
            (_, others, others_format, others_samples),
        ] = &tracks[..]
        {
            if let (Some(a), Some(b)) = (me.drift_ppm(), others.drift_ppm()) {
                println!(
                    "== relative drift {:+.1} ppm: {:+.1} ms per hour if played naively, removed by the time lines",
                    a - b,
                    (a - b) * 3.6
                );
            }
            if with_clicks {
                let a = onsets(me_samples, *me_format);
                let b = onsets(others_samples, *others_format);
                let mut offsets = Vec::new();
                for &fb in &b {
                    let tb = others.recording_ms(fb);
                    if let Some(ta) = a
                        .iter()
                        .map(|&fa| me.recording_ms(fa))
                        .min_by(|x, y| (x - tb).abs().total_cmp(&(y - tb).abs()))
                        && (ta - tb).abs() < 500.0
                    {
                        offsets.push(ta - tb);
                    }
                }
                if offsets.is_empty() {
                    println!("== clicks: none matched (are they audible in both tracks?)");
                } else {
                    let lo = offsets.iter().copied().fold(f64::MAX, f64::min);
                    let hi = offsets.iter().copied().fold(f64::MIN, f64::max);
                    println!(
                        "== clicks: {} matched, me - others {lo:+.2} .. {hi:+.2} ms (spread {:.2} ms; the constant part is the acoustic path and input latency)",
                        offsets.len(),
                        hi - lo
                    );
                }
            }
        }
        Ok(())
    }

    /// Frames where a click starts: above 30% of the peak after at least 400 ms below.
    fn onsets(samples: &[f32], format: StreamFormat) -> Vec<f64> {
        let channels = usize::from(format.channels);
        let mono: Vec<f32> = samples.chunks_exact(channels).map(|f| f[0]).collect();
        let peak = mono.iter().fold(0f32, |m, s| m.max(s.abs()));
        let threshold = peak * 0.3;
        let quiet_needed = format.sample_rate as usize * 4 / 10;
        let mut quiet = quiet_needed;
        let mut out = Vec::new();
        for (i, s) in mono.iter().enumerate() {
            if s.abs() >= threshold && peak > 0.0 {
                if quiet >= quiet_needed {
                    out.push(i as f64);
                }
                quiet = 0;
            } else {
                quiet += 1;
            }
        }
        out
    }

    /// A click track: a short 2 kHz burst every second over a quiet 440 Hz tone.
    fn clicks(path: &Path, seconds: u32) -> Result<(), Error> {
        let rate = 48_000u32;
        let mut writer = WavWriter::create(
            &PathBuf::from(path),
            StreamFormat {
                sample_rate: rate,
                channels: 1,
            },
        )?;
        let samples: Vec<f32> = (0..rate * seconds)
            .map(|i| {
                let t = f64::from(i) / f64::from(rate);
                let mut s = 0.05 * (std::f64::consts::TAU * 440.0 * t).sin();
                if i % rate < rate / 100 {
                    s += 0.6 * (std::f64::consts::TAU * 2_000.0 * t).sin();
                }
                s as f32
            })
            .collect();
        writer.write(&samples)?;
        writer.checkpoint()?;
        println!(
            "wrote {} ({seconds} s); play it with afplay while recording",
            path.display()
        );
        Ok(())
    }
}
