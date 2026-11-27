//! trama-rec: throwaway spike for issue #11 (two-track meeting capture on macOS).
//!
//!   trama-rec record --out DIR [--seconds N] [--no-mic] [--no-system] [--mic-device NAME]
//!                    [--tap-pid PID]... [--mute] [--tap-only] [--checkpoint-ms MS]
//!                    [--backend tap|sck]   (sck needs `--features sck`)
//!   trama-rec analyze DIR
//!   trama-rec align-check DIR      (click track audible in both tracks, speakers on)
//!   trama-rec repair FILE.wav...
//!   trama-rec gen-tone --out FILE.wav --seconds N
//!   trama-rec devices

mod analyze;
#[cfg(feature = "sck")]
mod sck;
mod sink;
mod tap;
mod wav;

use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, rest) = args.split_first().map(|(c, r)| (c.as_str(), r)).unwrap_or(("help", &[]));
    match cmd {
        "record" => record(rest),
        "analyze" => analyze::run(&PathBuf::from(rest.first().context("analyze DIR")?)),
        "repair" => {
            for f in rest {
                let i = wav::repair(&PathBuf::from(f))?;
                println!("{f}: {} frames = {:.3} s ({} ch @ {} Hz)", i.file_frames, i.file_frames as f64 / i.rate as f64, i.channels, i.rate);
            }
            Ok(())
        }
        "gen-tone" => gen_tone(rest),
        "align-check" => analyze::align_check(&PathBuf::from(rest.first().context("align-check DIR")?)),
        "version" => {
            println!("trama-rec {} build {}", env!("CARGO_PKG_VERSION"), option_env!("SPIKE_BUILD_ID").unwrap_or("dev"));
            Ok(())
        }
        "devices" => devices(),
        _ => {
            eprintln!("{}", include_str!("main.rs").lines().take(11).collect::<Vec<_>>().join("\n"));
            Ok(())
        }
    }
}

fn flag(rest: &[String], name: &str) -> bool {
    rest.iter().any(|a| a == name)
}

fn opt(rest: &[String], name: &str) -> Option<String> {
    rest.iter().position(|a| a == name).and_then(|i| rest.get(i + 1)).cloned()
}

fn opts(rest: &[String], name: &str) -> Vec<String> {
    rest.windows(2).filter(|w| w[0] == name).map(|w| w[1].clone()).collect()
}

fn host_now_ns() -> u64 {
    unsafe { objc2_core_audio::AudioConvertHostTimeToNanos(objc2_core_audio::AudioGetCurrentHostTime()) }
}

fn devices() -> Result<()> {
    let host = cpal::default_host();
    if let Some(d) = host.default_input_device() {
        println!("default input:  {} {:?}", d.description()?.name(), d.default_input_config()?);
    }
    for d in host.input_devices()? {
        println!("input device:   {}", d.description()?.name());
    }
    if let Some(d) = host.default_output_device() {
        println!("default output: {} {:?}", d.description()?.name(), d.default_output_config()?);
    }
    Ok(())
}

fn record(rest: &[String]) -> Result<()> {
    let out = PathBuf::from(opt(rest, "--out").context("--out DIR is required")?);
    let seconds: Option<f64> = opt(rest, "--seconds").map(|s| s.parse()).transpose()?;
    let checkpoint = Duration::from_millis(opt(rest, "--checkpoint-ms").map(|s| s.parse()).transpose()?.unwrap_or(1000));
    let want_mic = !flag(rest, "--no-mic");
    let want_sys = !flag(rest, "--no-system");
    let pids: Vec<i32> = opts(rest, "--tap-pid").iter().map(|p| p.parse()).collect::<Result<_, _>>()?;
    let mute = flag(rest, "--mute");
    std::fs::create_dir_all(&out)?;

    let stop = Arc::new(AtomicBool::new(false));
    let stop_req = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, stop_req.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, stop_req.clone())?;

    let mut tracks = Vec::new();
    let mut session = vec![
        format!(r#""pid":{}"#, std::process::id()),
        format!(r#""start_unix_ms":{}"#, sink::now_unix_ms()),
        format!(r#""start_host_ns":{}"#, host_now_ns()),
        format!(r#""checkpoint_ms":{}"#, checkpoint.as_millis()),
    ];

    // --- Microphone (cpal) ---
    let mut mic_stream = None;
    if want_mic {
        let host = cpal::default_host();
        let dev = match opt(rest, "--mic-device") {
            // Pick an input device by (case-insensitive) name substring, e.g. a USB/Bluetooth headset.
            Some(want) => host
                .input_devices()?
                .find(|d| d.description().map(|x| x.name().to_lowercase().contains(&want.to_lowercase())).unwrap_or(false))
                .with_context(|| format!("no input device matching {want:?}"))?,
            None => host.default_input_device().context("no default input device")?,
        };
        let cfg = dev.default_input_config()?;
        let ch = cfg.channels();
        let rate = cfg.sample_rate();
        let name = dev.description().map(|d| d.name().to_string()).unwrap_or_default();
        let (mut rt, track) = sink::track("mic", &out, ch, rate, stop.clone(), checkpoint)?;
        let stream = dev.build_input_stream::<f32, _, _>(
            cfg.config(),
            move |data: &[f32], info: &cpal::InputCallbackInfo| {
                let host_ns = info.timestamp().callback.as_nanos() as u64;
                rt.push(data, ch as usize, host_ns, f64::NAN);
            },
            |e| eprintln!("mic stream error: {e}"),
            None,
        )?;
        stream.play()?;
        session.push(format!(r#""mic":{{"device":{:?},"rate":{rate},"channels":{ch},"device_format":"{:?}"}}"#, name, cfg.sample_format()));
        eprintln!("mic: {name} {ch} ch @ {rate} Hz");
        mic_stream = Some(stream);
        tracks.push(track);
    }

    // --- System audio (Core Audio process tap) ---
    let mut sys = None;
    #[cfg(feature = "sck")]
    let mut _sck_stream = None;
    #[cfg(feature = "sck")]
    if want_sys && opt(rest, "--backend").as_deref() == Some("sck") {
        let (rt, track) = sink::track("system", &out, 2, 48_000, stop.clone(), checkpoint)?;
        _sck_stream = Some(sck::start(rt, 48_000, 2)?);
        session.push(r#""system":{"backend":"screencapturekit","rate":48000,"channels":2}"#.to_string());
        eprintln!("system: ScreenCaptureKit 2 ch @ 48000 Hz");
        tracks.push(track);
    }
    if want_sys && opt(rest, "--backend").as_deref() == Some("sck") && !cfg!(feature = "sck") {
        bail!("--backend sck needs a build with --features sck");
    }
    if want_sys && opt(rest, "--backend").as_deref() != Some("sck") {
        let target = if pids.is_empty() { tap::Target::AllButSelf } else { tap::Target::Pids(pids.clone()) };
        let prep = tap::prepare(target, mute, flag(rest, "--tap-only"))?;
        let ch = prep.asbd.mChannelsPerFrame as u16;
        let rate = prep.asbd.mSampleRate as u32;
        let (rt, track) = sink::track("system", &out, ch, rate, stop.clone(), checkpoint)?;
        let t = prep.start(rt)?;
        session.push(format!(
            r#""system":{{"backend":"coreaudio-process-tap","output_uid":{:?},"rate":{rate},"channels":{ch},"non_interleaved":{},"target":{:?},"muted":{mute},"tap_only_aggregate":{}}}"#,
            t.output_uid,
            t.non_interleaved,
            if pids.is_empty() { "all-but-self".to_string() } else { format!("pids {pids:?}") },
            flag(rest, "--tap-only")
        ));
        eprintln!("system: process tap on output {} {ch} ch @ {rate} Hz", t.output_uid);
        sys = Some(t);
        tracks.push(track);
    }
    if tracks.is_empty() {
        bail!("nothing to record");
    }
    std::fs::write(out.join("session.json"), format!("{{{}}}\n", session.join(",")))?;
    eprintln!("recording to {} (pid {}) ...", out.display(), std::process::id());

    let t0 = Instant::now();
    let mut last_print = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(100));
        if stop_req.load(Ordering::Relaxed) {
            eprintln!("signal received, stopping");
            break;
        }
        if let Some(s) = seconds {
            if t0.elapsed().as_secs_f64() >= s {
                break;
            }
        }
        if last_print.elapsed() >= Duration::from_secs(10) {
            last_print = Instant::now();
            let parts: Vec<String> = tracks
                .iter()
                .map(|t| {
                    format!(
                        "{} frames={} dropped={}",
                        t.name,
                        t.stats.frames.load(Ordering::Relaxed),
                        t.stats.dropped_frames.load(Ordering::Relaxed)
                    )
                })
                .collect();
            eprintln!("[{:>6.1}s] {}", t0.elapsed().as_secs_f64(), parts.join(" | "));
        }
    }

    // Stop producers first, then let writers drain and finalize.
    drop(mic_stream);
    if let Some(t) = sys {
        t.stop();
    }
    stop.store(true, Ordering::Release);
    for t in tracks {
        let name = t.name;
        let frames = t.join()?;
        eprintln!("{name}: {frames} frames written");
    }
    Ok(())
}

fn gen_tone(rest: &[String]) -> Result<()> {
    let out = PathBuf::from(opt(rest, "--out").context("--out FILE")?);
    let secs: f64 = opt(rest, "--seconds").context("--seconds N")?.parse()?;
    let rate = 48_000u32;
    let mut w = wav::WavWriter::create(&out, 2, rate)?;
    let mut buf = Vec::with_capacity(rate as usize * 2);
    let total = (secs * rate as f64) as u64;
    let mut i = 0u64;
    while i < total {
        buf.clear();
        for _ in 0..rate.min((total - i) as u32) {
            let t = i as f64 / rate as f64;
            // 440 Hz tone + a 10 ms click at every whole second (alignment marker).
            let mut s = 0.1 * (2.0 * std::f64::consts::PI * 440.0 * t).sin();
            if i % rate as u64 <= rate as u64 / 100 {
                s += 0.5 * (2.0 * std::f64::consts::PI * 2000.0 * t).sin();
            }
            buf.push(s as f32);
            buf.push(s as f32);
            i += 1;
        }
        w.write(&buf)?;
    }
    w.checkpoint()?;
    println!("wrote {} ({secs} s)", out.display());
    Ok(())
}
