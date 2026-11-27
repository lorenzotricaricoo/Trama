//! Minimal progressive WAV writer (32-bit float).
//!
//! The header is written up front with zero sizes and patched on every
//! `checkpoint()` (followed by `fsync`). If the process dies between two
//! checkpoints the file is still a valid WAV holding audio up to the last
//! checkpoint; `repair()` recovers the remaining tail from the file length.

use anyhow::{Context, Result, bail};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::FileExt;
use std::path::Path;

const HEADER_LEN: u64 = 44;

pub struct WavWriter {
    file: File,
    channels: u16,
    data_bytes: u64,
    scratch: Vec<u8>,
}

fn header(channels: u16, rate: u32, data_bytes: u32) -> [u8; 44] {
    let block_align = channels * 4;
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36u32.wrapping_add(data_bytes)).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&3u16.to_le_bytes()); // WAVE_FORMAT_IEEE_FLOAT
    h[22..24].copy_from_slice(&channels.to_le_bytes());
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&(rate * block_align as u32).to_le_bytes());
    h[32..34].copy_from_slice(&block_align.to_le_bytes());
    h[34..36].copy_from_slice(&32u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    h
}

impl WavWriter {
    pub fn create(path: &Path, channels: u16, rate: u32) -> Result<Self> {
        let mut file = File::create(path).with_context(|| format!("create {}", path.display()))?;
        file.write_all(&header(channels, rate, 0))?;
        file.sync_all()?;
        Ok(Self { file, channels, data_bytes: 0, scratch: Vec::with_capacity(1 << 16) })
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        if samples.is_empty() {
            return Ok(());
        }
        self.scratch.clear();
        for s in samples {
            self.scratch.extend_from_slice(&s.to_le_bytes());
        }
        self.file.write_all(&self.scratch)?;
        self.data_bytes += self.scratch.len() as u64;
        Ok(())
    }

    pub fn frames(&self) -> u64 {
        self.data_bytes / (self.channels as u64 * 4)
    }

    /// Patch the RIFF/data sizes and fsync, so the file on disk is complete.
    pub fn checkpoint(&mut self) -> Result<()> {
        let d = self.data_bytes.min(u32::MAX as u64 - 36) as u32;
        self.file.write_all_at(&(36 + d).to_le_bytes(), 4)?;
        self.file.write_all_at(&d.to_le_bytes(), 40)?;
        self.file.sync_data()?;
        Ok(())
    }
}

pub struct WavInfo {
    pub channels: u16,
    pub rate: u32,
    pub header_frames: u64,
    pub file_frames: u64,
}

pub fn inspect(path: &Path) -> Result<WavInfo> {
    let mut f = File::open(path)?;
    let mut h = [0u8; 44];
    f.read_exact(&mut h)?;
    if &h[0..4] != b"RIFF" || &h[8..12] != b"WAVE" || &h[36..40] != b"data" {
        bail!("{} is not a WAV written by trama-rec", path.display());
    }
    let channels = u16::from_le_bytes([h[22], h[23]]);
    let rate = u32::from_le_bytes([h[24], h[25], h[26], h[27]]);
    let data = u32::from_le_bytes([h[40], h[41], h[42], h[43]]) as u64;
    let len = f.metadata()?.len();
    let block = channels as u64 * 4;
    Ok(WavInfo {
        channels,
        rate,
        header_frames: data / block,
        file_frames: (len - HEADER_LEN) / block,
    })
}

/// Rewrite the header sizes from the actual file length (drops a partial last frame).
pub fn repair(path: &Path) -> Result<WavInfo> {
    let info = inspect(path)?;
    let block = info.channels as u64 * 4;
    let data = info.file_frames * block;
    let f = OpenOptions::new().write(true).open(path)?;
    f.set_len(HEADER_LEN + data)?;
    f.write_all_at(&header(info.channels, info.rate, data as u32), 0)?;
    f.sync_all()?;
    inspect(path)
}

/// Whole file, mixed down to mono.
pub fn read_mono(path: &Path) -> Result<(Vec<f32>, u32)> {
    let info = inspect(path)?;
    let buf = std::fs::read(path)?;
    let ch = info.channels as usize;
    let mut out = Vec::with_capacity(info.file_frames as usize);
    for f in 0..info.file_frames as usize {
        let mut acc = 0f32;
        for c in 0..ch {
            let o = 44 + (f * ch + c) * 4;
            acc += f32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
        }
        out.push(acc / ch as f32);
    }
    Ok((out, info.rate))
}

/// Peak and RMS of a WAV written by this tool (used to detect silent = TCC-blocked tracks).
pub fn levels(path: &Path) -> Result<(f32, f32)> {
    let info = inspect(path)?;
    let mut f = File::open(path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let n = (info.file_frames * info.channels as u64) as usize;
    let (mut peak, mut sum) = (0f32, 0f64);
    for i in 0..n {
        let o = 44 + i * 4;
        let s = f32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
        peak = peak.max(s.abs());
        sum += (s as f64) * (s as f64);
    }
    Ok((peak, if n > 0 { (sum / n as f64).sqrt() as f32 } else { 0.0 }))
}
