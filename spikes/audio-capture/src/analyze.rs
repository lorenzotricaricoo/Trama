//! Offline analysis of a recording directory: completeness, clock drift, gaps.
//!
//! Each track's sidecar has anchors (host_ns, device frame index). A least-squares
//! fit of frame index vs host time gives the track's real sample rate measured
//! against the host clock (mach_absolute_time). The difference between the two
//! tracks' rates is the inter-track drift if the files were simply played side by
//! side at their nominal rate.

use crate::wav;
use anyhow::Result;
use serde_json::Value;
use std::path::Path;

pub struct TrackFit {
    pub name: String,
    pub nominal: f64,
    pub first_host_ns: u64,
    pub ppm: f64,
    pub duration_s: f64,
    /// Position error at the end of the run: frames/nominal - elapsed host time (ms).
    pub end_error_ms: f64,
}

fn fit(name: &str, dir: &Path) -> Result<Option<TrackFit>> {
    let wavp = dir.join(format!("{name}.wav"));
    let tp = dir.join(format!("{name}.timing.jsonl"));
    if !wavp.exists() {
        return Ok(None);
    }
    let info = wav::inspect(&wavp)?;
    let (peak, rms) = wav::levels(&wavp)?;
    let nominal = info.rate as f64;
    let mut pts = Vec::new();
    let mut events = Vec::new();
    for line in std::fs::read_to_string(&tp)?.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue }; // tolerate a torn last line
        let h = v["host_ns"].as_u64().unwrap_or(0);
        let f = v["frame"].as_u64().unwrap_or(0);
        pts.push((h, f));
        if v["t"] != "anchor" {
            events.push(format!("{}@{:.1}s", v["t"].as_str().unwrap_or("?"), f as f64 / nominal));
        }
    }
    println!("== {name}");
    println!(
        "   wav: {} ch @ {} Hz, header {} frames ({:.3} s), file {} frames ({:.3} s)",
        info.channels,
        info.rate,
        info.header_frames,
        info.header_frames as f64 / nominal,
        info.file_frames,
        info.file_frames as f64 / nominal
    );
    println!(
        "   level: peak {:.4}, rms {:.5} {}",
        peak,
        rms,
        if peak == 0.0 { "<- SILENT (TCC denied, or nothing playing)" } else { "" }
    );
    println!("   sidecar: {} anchors, {} events {:?}", pts.len(), events.len(), events.iter().take(10).collect::<Vec<_>>());
    if pts.len() < 3 {
        return Ok(None);
    }
    let (h0, f0) = pts[0];
    let xs: Vec<f64> = pts.iter().map(|(h, _)| (*h - h0) as f64 / 1e9).collect();
    let ys: Vec<f64> = pts.iter().map(|(_, f)| (*f - f0) as f64).collect();
    let n = xs.len() as f64;
    let mx = xs.iter().sum::<f64>() / n;
    let my = ys.iter().sum::<f64>() / n;
    let sxy: f64 = xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    let sxx: f64 = xs.iter().map(|x| (x - mx) * (x - mx)).sum();
    let rate = sxy / sxx;
    let b = my - rate * mx;
    let max_resid_ms = xs
        .iter()
        .zip(&ys)
        .map(|(x, y)| ((y - (rate * x + b)) / nominal * 1e3).abs())
        .fold(0.0, f64::max);
    let ppm = (rate / nominal - 1.0) * 1e6;
    let dur = *xs.last().unwrap();
    let end_error_ms = (ys.last().unwrap() / nominal - dur) * 1e3;
    println!(
        "   clock: measured {:.3} Hz over {:.1} s -> {:+.1} ppm vs nominal; end error {:+.2} ms; max jitter {:.2} ms",
        rate, dur, ppm, end_error_ms, max_resid_ms
    );
    Ok(Some(TrackFit { name: name.into(), nominal, first_host_ns: h0, ppm, duration_s: dur, end_error_ms }))
}

pub fn run(dir: &Path) -> Result<()> {
    let mic = fit("mic", dir)?;
    let sys = fit("system", dir)?;
    if let (Some(m), Some(s)) = (mic, sys) {
        let rel_ppm = m.ppm - s.ppm;
        let dur = m.duration_s.min(s.duration_s);
        println!("== inter-track");
        println!(
            "   start offset (system first frame - mic first frame): {:+.1} ms",
            (s.first_host_ns as f64 - m.first_host_ns as f64) / 1e6
        );
        println!(
            "   relative clock drift {:+.1} ppm -> {:+.1} ms over this run ({:.0} s), extrapolated {:+.1} ms at 60 min",
            rel_ppm,
            rel_ppm * 1e-6 * dur * 1e3,
            dur,
            rel_ppm * 1e-6 * 3600.0 * 1e3
        );
        println!(
            "   measured end-of-run misalignment if played at nominal rate: {:+.2} ms ({} {:+.2} ms, {} {:+.2} ms)",
            m.end_error_ms - s.end_error_ms,
            m.name,
            m.end_error_ms,
            s.name,
            s.end_error_ms
        );
        let _ = (m.nominal, s.nominal);
    }
    Ok(())
}

fn anchors(path: &Path) -> Result<Vec<(f64, f64)>> {
    let mut v = Vec::new();
    for line in std::fs::read_to_string(path)?.lines() {
        let Ok(j) = serde_json::from_str::<Value>(line) else { continue };
        if let (Some(f), Some(h)) = (j["frame"].as_u64(), j["host_ns"].as_u64()) {
            v.push((f as f64, h as f64 / 1e9));
        }
    }
    Ok(v)
}

/// Map a frame index to host time (s) by piecewise-linear interpolation of the anchors.
/// This is how a consumer realigns the two tracks on one timeline.
fn frame_to_host(a: &[(f64, f64)], frame: f64) -> f64 {
    let i = a.partition_point(|(f, _)| *f <= frame).clamp(1, a.len() - 1);
    let ((f0, h0), (f1, h1)) = (a[i - 1], a[i]);
    h0 + (frame - f0) * (h1 - h0) / (f1 - f0)
}

/// Onsets: first 10 ms block above 30% of the track peak after >= 400 ms below it.
fn onsets(x: &[f32], rate: u32) -> Vec<f64> {
    let peak = x.iter().fold(0f32, |m, v| m.max(v.abs()));
    let thr = peak * 0.3;
    let blk = rate as usize / 100;
    let mut quiet = 0usize;
    let mut out = Vec::new();
    for (b, chunk) in x.chunks(blk).enumerate() {
        let p = chunk.iter().fold(0f32, |m, v| m.max(v.abs()));
        if p >= thr && peak > 0.0 {
            if quiet >= 40 {
                let k = chunk.iter().position(|v| v.abs() >= thr).unwrap_or(0);
                out.push((b * blk + k) as f64);
            }
            quiet = 0;
        } else {
            quiet += 1;
        }
    }
    out
}

/// End-to-end alignment check with an acoustic click track (speakers, no headphones):
/// the same click appears digitally in `system` and acoustically in `mic`.
pub fn align_check(dir: &Path) -> Result<()> {
    let (mic, mr) = wav::read_mono(&dir.join("mic.wav"))?;
    let (sys, sr) = wav::read_mono(&dir.join("system.wav"))?;
    let am = anchors(&dir.join("mic.timing.jsonl"))?;
    let asys = anchors(&dir.join("system.timing.jsonl"))?;
    let om = onsets(&mic, mr);
    let os = onsets(&sys, sr);
    println!("onsets: mic {}, system {}", om.len(), os.len());
    let (m0, s0) = (am[0].1, asys[0].1);
    let mut rows = Vec::new();
    for &fs in &os {
        let hs = frame_to_host(&asys, fs);
        let best = om.iter().map(|&fm| (fm, frame_to_host(&am, fm))).min_by(|a, b| {
            (a.1 - hs).abs().partial_cmp(&(b.1 - hs).abs()).unwrap()
        });
        if let Some((fm, hm)) = best {
            if (hm - hs).abs() < 0.5 {
                // naive: files assumed to start at their first anchor and run at the nominal rate
                let naive = ((m0 + fm / mr as f64) - (s0 + fs / sr as f64)) * 1e3;
                rows.push((hs - s0, (hm - hs) * 1e3, naive));
            }
        }
    }
    if rows.is_empty() {
        println!("no matching clicks (is the click track audible in both tracks?)");
        return Ok(());
    }
    for r in rows.iter().take(3).chain(rows.iter().rev().take(3).rev()) {
        println!("  t={:>7.1}s  mic-system offset via sidecar {:+7.2} ms  (naive {:+7.2} ms)", r.0, r.1, r.2);
    }
    let (lo, hi) = rows.iter().fold((f64::MAX, f64::MIN), |(l, h), r| (l.min(r.1), h.max(r.1)));
    println!(
        "matched {} clicks; sidecar offset {:.2}..{:.2} ms (spread {:.2} ms). The constant part is acoustic path + input latency; the spread is the residual drift/jitter.",
        rows.len(),
        lo,
        hi,
        hi - lo
    );
    Ok(())
}
