//! Progressive 32-bit float WAV files.
//!
//! The header is written first with empty sizes and patched at every
//! [`WavWriter::checkpoint`], followed by a sync. If the process dies between two
//! checkpoints, the file is still a valid WAV up to the last checkpoint, and
//! [`repair_wav`] recovers the rest from the file length.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::{CaptureError, Result, StreamFormat, error_code};

/// Size of the canonical header written by [`WavWriter`].
pub const HEADER_LEN: u64 = 44;
/// Largest data chunk a WAV can describe (its sizes are 32-bit).
const MAX_DATA_BYTES: u64 = u32::MAX as u64 - 36;
/// `WAVE_FORMAT_IEEE_FLOAT`.
const FORMAT_FLOAT: u16 = 3;

/// Writes a WAV progressively. Not buffered: every [`write`](Self::write) reaches the
/// operating system, so a crash of the process loses nothing written.
#[derive(Debug)]
pub struct WavWriter {
    file: File,
    format: StreamFormat,
    data_bytes: u64,
    max_data_bytes: u64,
    scratch: Vec<u8>,
}

fn header(format: StreamFormat, data_bytes: u32) -> [u8; HEADER_LEN as usize] {
    let block_align = format.channels * 4;
    let mut h = [0u8; HEADER_LEN as usize];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&36u32.wrapping_add(data_bytes).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&FORMAT_FLOAT.to_le_bytes());
    h[22..24].copy_from_slice(&format.channels.to_le_bytes());
    h[24..28].copy_from_slice(&format.sample_rate.to_le_bytes());
    h[28..32].copy_from_slice(&(format.sample_rate * u32::from(block_align)).to_le_bytes());
    h[32..34].copy_from_slice(&block_align.to_le_bytes());
    h[34..36].copy_from_slice(&32u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    h
}

/// Writes the RIFF and data chunk sizes in place.
fn patch_sizes(file: &mut File, data_bytes: u32) -> io::Result<()> {
    file.seek(SeekFrom::Start(4))?;
    file.write_all(&36u32.wrapping_add(data_bytes).to_le_bytes())?;
    file.seek(SeekFrom::Start(40))?;
    file.write_all(&data_bytes.to_le_bytes())
}

impl WavWriter {
    /// Creates a new file; fails if it exists.
    pub fn create(path: &Path, format: StreamFormat) -> io::Result<Self> {
        Self::create_with_limit(path, format, MAX_DATA_BYTES)
    }

    pub(crate) fn create_with_limit(
        path: &Path,
        format: StreamFormat,
        max_data_bytes: u64,
    ) -> io::Result<Self> {
        if format.channels == 0 || format.sample_rate == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty format"));
        }
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(&header(format, 0))?;
        file.sync_all()?;
        let block = u64::from(format.channels) * 4;
        Ok(Self {
            file,
            format,
            data_bytes: 0,
            max_data_bytes: max_data_bytes.min(MAX_DATA_BYTES) / block * block,
            scratch: Vec::with_capacity(1 << 16),
        })
    }

    pub fn format(&self) -> StreamFormat {
        self.format
    }

    /// Frames written so far.
    pub fn frames(&self) -> u64 {
        self.data_bytes / (u64::from(self.format.channels) * 4)
    }

    /// Frames that still fit in the file.
    pub fn remaining_frames(&self) -> u64 {
        (self.max_data_bytes - self.data_bytes) / (u64::from(self.format.channels) * 4)
    }

    /// Appends interleaved samples (whole frames). Fails with `FileTooLarge` past the
    /// WAV size limit, without writing anything.
    pub fn write(&mut self, samples: &[f32]) -> io::Result<()> {
        if samples.is_empty() {
            return Ok(());
        }
        let bytes = samples.len() as u64 * 4;
        if self.data_bytes + bytes > self.max_data_bytes {
            return Err(io::ErrorKind::FileTooLarge.into());
        }
        self.scratch.clear();
        for sample in samples {
            self.scratch.extend_from_slice(&sample.to_le_bytes());
        }
        self.file.write_all(&self.scratch)?;
        self.data_bytes += bytes;
        Ok(())
    }

    /// Appends `frames` frames of silence.
    pub fn write_silence(&mut self, frames: u64) -> io::Result<()> {
        const CHUNK: usize = 4096;
        let channels = usize::from(self.format.channels);
        let zeros = vec![0.0f32; CHUNK * channels];
        let mut left = frames;
        while left > 0 {
            let n = left.min(CHUNK as u64) as usize;
            self.write(&zeros[..n * channels])?;
            left -= n as u64;
        }
        Ok(())
    }

    /// Patches the header sizes and syncs, so the file on disk is complete.
    pub fn checkpoint(&mut self) -> io::Result<()> {
        let data = self.data_bytes as u32;
        patch_sizes(&mut self.file, data)?;
        self.file.seek(SeekFrom::End(0))?;
        self.file.sync_data()
    }
}

/// What a WAV written by Trama contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavInfo {
    pub format: StreamFormat,
    /// Frames counted by the header (up to the last checkpoint).
    pub header_frames: u64,
    /// Whole frames present in the file.
    pub file_frames: u64,
}

impl WavInfo {
    /// Duration of the frames present in the file, in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        self.file_frames * 1000 / u64::from(self.format.sample_rate)
    }
}

fn invalid(path: &Path, why: &str) -> CaptureError {
    CaptureError::new(
        error_code::INVALID_WAV,
        format!("{}: {why}", path.display()),
    )
}

/// Reads the header of a WAV written by [`WavWriter`].
pub fn inspect(path: &Path) -> Result<WavInfo> {
    let mut file = File::open(path).map_err(|e| invalid(path, &e.to_string()))?;
    let mut h = [0u8; HEADER_LEN as usize];
    file.read_exact(&mut h)
        .map_err(|_| invalid(path, "shorter than a header"))?;
    let format_tag = u16::from_le_bytes([h[20], h[21]]);
    if &h[0..4] != b"RIFF"
        || &h[8..12] != b"WAVE"
        || &h[36..40] != b"data"
        || format_tag != FORMAT_FLOAT
    {
        return Err(invalid(path, "not a float WAV written by Trama"));
    }
    let channels = u16::from_le_bytes([h[22], h[23]]);
    let sample_rate = u32::from_le_bytes([h[24], h[25], h[26], h[27]]);
    if channels == 0 || sample_rate == 0 {
        return Err(invalid(path, "empty format"));
    }
    let data = u64::from(u32::from_le_bytes([h[40], h[41], h[42], h[43]]));
    let len = file
        .metadata()
        .map_err(|e| invalid(path, &e.to_string()))?
        .len();
    let block = u64::from(channels) * 4;
    Ok(WavInfo {
        format: StreamFormat {
            sample_rate,
            channels,
        },
        header_frames: data / block,
        file_frames: len.saturating_sub(HEADER_LEN).min(MAX_DATA_BYTES) / block,
    })
}

/// Makes the header count every whole frame in the file and drops a partial last frame:
/// what is left after a crash. Safe to run on a complete file.
pub fn repair_wav(path: &Path) -> Result<WavInfo> {
    let info = inspect(path)?;
    let block = u64::from(info.format.channels) * 4;
    let data = info.file_frames * block;
    let fix = || -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).open(path)?;
        file.set_len(HEADER_LEN + data)?;
        file.write_all(&header(info.format, data as u32))?;
        file.sync_all()
    };
    fix().map_err(|e| CaptureError::io(error_code::WRITE_FAILED, e))?;
    inspect(path)
}

/// Reads all the samples of a WAV written by [`WavWriter`], interleaved.
pub fn read(path: &Path) -> Result<(StreamFormat, Vec<f32>)> {
    let info = inspect(path)?;
    let bytes = std::fs::read(path).map_err(|e| invalid(path, &e.to_string()))?;
    let count = (info.file_frames * u64::from(info.format.channels)) as usize;
    let samples = bytes[HEADER_LEN as usize..]
        .chunks_exact(4)
        .take(count)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    Ok((info.format, samples))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONO: StreamFormat = StreamFormat {
        sample_rate: 8000,
        channels: 1,
    };

    #[test]
    fn writes_a_readable_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let mut w = WavWriter::create(&path, MONO).unwrap();
        w.write(&[0.5, -0.5, 0.25]).unwrap();
        w.write_silence(5).unwrap();
        w.checkpoint().unwrap();
        let info = inspect(&path).unwrap();
        assert_eq!(info.header_frames, 8);
        assert_eq!(info.file_frames, 8);
        let (format, samples) = read(&path).unwrap();
        assert_eq!(format, MONO);
        assert_eq!(samples, [0.5, -0.5, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        WavWriter::create(&path, MONO).unwrap();
        assert!(WavWriter::create(&path, MONO).is_err());
    }

    #[test]
    fn repair_recovers_audio_written_after_the_last_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let mut w = WavWriter::create(&path, MONO).unwrap();
        w.write(&[0.1; 800]).unwrap();
        w.checkpoint().unwrap();
        w.write(&[0.2; 80]).unwrap();
        // The process dies here: no checkpoint, and half a sample at the end.
        drop(w);
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(&[1, 2]).unwrap();

        let before = inspect(&path).unwrap();
        assert_eq!(before.header_frames, 800);
        assert_eq!(before.file_frames, 880);
        let after = repair_wav(&path).unwrap();
        assert_eq!(after.header_frames, 880);
        assert_eq!(after.file_frames, 880);
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            HEADER_LEN + 880 * 4
        );
        assert_eq!(read(&path).unwrap().1[879], 0.2);
    }

    #[test]
    fn refuses_to_grow_past_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let mut w = WavWriter::create_with_limit(&path, MONO, 40).unwrap();
        w.write(&[0.0; 8]).unwrap();
        assert_eq!(w.remaining_frames(), 2);
        let err = w.write(&[0.0; 4]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
        assert_eq!(w.frames(), 8);
    }

    #[test]
    fn rejects_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        std::fs::write(
            &path,
            b"not a wav file at all, but long enough for a header",
        )
        .unwrap();
        assert_eq!(inspect(&path).unwrap_err().code, error_code::INVALID_WAV);
    }
}
