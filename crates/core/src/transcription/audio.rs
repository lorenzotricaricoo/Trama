//! Reading a track as 16 kHz mono samples, a chunk at a time, so an hour of audio never
//! sits in memory at once.

use std::fs::File;
use std::io::{BufReader, ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use hound::{SampleFormat, WavReader};
use sherpa_onnx::LinearResampler;

use super::{Result, SAMPLE_RATE, TranscriptionError, error_code};

/// Audio read a chunk at a time, as 16 kHz mono samples.
pub trait AudioSource {
    /// Appends the next chunk to `out`. Returns false once the source is exhausted
    /// (`out` may still have received the last samples).
    fn read(&mut self, out: &mut Vec<f32>) -> Result<bool>;
    /// Fraction of the source read so far, from 0 to 1.
    fn progress(&self) -> f32;
}

/// Frames read from the file at a time: half a second at 48 kHz.
const CHUNK_FRAMES: usize = 24_000;

/// A WAV file: 32-bit float (what the recorder writes) or integer PCM, any sample rate,
/// any number of channels. Channels are averaged and the result is resampled to
/// [`SAMPLE_RATE`].
///
/// A file cut short (the recorder crashed before writing all of it, so its header
/// declares more data than there is) is read up to where it ends: what is there gets
/// transcribed.
pub struct WavSource {
    path: PathBuf,
    reader: WavReader<EofReader>,
    /// Set once the file has no more bytes.
    at_eof: Arc<AtomicBool>,
    channels: usize,
    sample_rate: u32,
    format: SampleFormat,
    bits: u16,
    frames: u64,
    frames_read: u64,
    resampler: Option<LinearResampler>,
    interleaved: Vec<f32>,
}

impl std::fmt::Debug for WavSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WavSource")
            .field("path", &self.path)
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("frames", &self.frames)
            .finish_non_exhaustive()
    }
}

impl WavSource {
    pub fn open(path: &Path) -> Result<Self> {
        let at_eof = Arc::new(AtomicBool::new(false));
        let file = File::open(path).map_err(hound::Error::IoError);
        let reader = file
            .and_then(|file| {
                WavReader::new(EofReader {
                    inner: BufReader::new(file),
                    at_eof: Arc::clone(&at_eof),
                })
            })
            .map_err(|err| match err {
                hound::Error::IoError(io) if io.kind() == ErrorKind::NotFound => {
                    TranscriptionError::permanent(error_code::AUDIO_MISSING, path.display())
                }
                hound::Error::IoError(io) => TranscriptionError::retryable(
                    error_code::AUDIO_READ_FAILED,
                    format!("{}: {io}", path.display()),
                ),
                other => TranscriptionError::permanent(
                    error_code::AUDIO_INVALID,
                    format!("{}: {other}", path.display()),
                ),
            })?;
        let spec = reader.spec();
        let supported = match spec.sample_format {
            SampleFormat::Float => spec.bits_per_sample == 32,
            SampleFormat::Int => (8..=32).contains(&spec.bits_per_sample),
        };
        if !supported || spec.channels == 0 || spec.sample_rate == 0 {
            return Err(TranscriptionError::permanent(
                error_code::AUDIO_INVALID,
                format!("{}: unsupported format {spec:?}", path.display()),
            ));
        }
        let resampler = if spec.sample_rate == SAMPLE_RATE {
            None
        } else {
            Some(
                LinearResampler::create(spec.sample_rate as i32, SAMPLE_RATE as i32).ok_or_else(
                    || {
                        TranscriptionError::permanent(
                            error_code::AUDIO_INVALID,
                            format!("cannot resample from {} Hz", spec.sample_rate),
                        )
                    },
                )?,
            )
        };
        Ok(Self {
            path: path.to_owned(),
            frames: u64::from(reader.duration()),
            reader,
            at_eof,
            channels: usize::from(spec.channels),
            sample_rate: spec.sample_rate,
            format: spec.sample_format,
            bits: spec.bits_per_sample,
            frames_read: 0,
            resampler,
            interleaved: Vec::with_capacity(CHUNK_FRAMES * usize::from(spec.channels)),
        })
    }

    /// Sample rate of the file, which the timing sidecar's frames refer to.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Frames in the file, as its header declares them (fewer if it turned out to be
    /// cut short once read to the end).
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Length of the file in seconds.
    pub fn duration_s(&self) -> f64 {
        self.frames as f64 / f64::from(self.sample_rate)
    }
}

/// The file under the WAV reader, noting when it runs out of bytes: a read error there
/// means the file was cut short, not that the disk failed.
struct EofReader {
    inner: BufReader<File>,
    at_eof: Arc<AtomicBool>,
}

impl Read for EofReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n == 0 && !buf.is_empty() {
            self.at_eof.store(true, Ordering::Relaxed);
        }
        Ok(n)
    }
}

fn read_error(path: &Path, err: hound::Error) -> TranscriptionError {
    let detail = format!("{}: {err}", path.display());
    match err {
        hound::Error::IoError(_) => {
            TranscriptionError::retryable(error_code::AUDIO_READ_FAILED, detail)
        }
        _ => TranscriptionError::permanent(error_code::AUDIO_INVALID, detail),
    }
}

impl AudioSource for WavSource {
    fn read(&mut self, out: &mut Vec<f32>) -> Result<bool> {
        let left = self.frames - self.frames_read;
        let frames = left.min(CHUNK_FRAMES as u64) as usize;
        let wanted = frames * self.channels;
        self.interleaved.clear();
        let result = match self.format {
            SampleFormat::Float => self
                .reader
                .samples::<f32>()
                .take(wanted)
                .try_for_each(|sample| sample.map(|s| self.interleaved.push(s))),
            SampleFormat::Int => {
                let scale = 1.0 / (1u64 << (self.bits - 1)) as f32;
                self.reader
                    .samples::<i32>()
                    .take(wanted)
                    .try_for_each(|sample| sample.map(|s| self.interleaved.push(s as f32 * scale)))
            }
        };
        let truncated = match result {
            Ok(()) => self.interleaved.len() != wanted,
            // The header promised more than the file holds: the end of the data.
            Err(hound::Error::IoError(_)) if self.at_eof.load(Ordering::Relaxed) => true,
            Err(err) => return Err(read_error(&self.path, err)),
        };
        if truncated {
            // Drop a frame cut in the middle, and stop here.
            let whole = self.interleaved.len() / self.channels * self.channels;
            self.interleaved.truncate(whole);
            self.frames = self.frames_read + (whole / self.channels) as u64;
            eprintln!(
                "trama: {} ends early, after {} frames (fewer than its header declares); transcribing what is there",
                self.path.display(),
                self.frames
            );
        }
        self.frames_read += (self.interleaved.len() / self.channels) as u64;
        let last = self.frames_read == self.frames;

        let mono: Vec<f32> = if self.channels == 1 {
            std::mem::take(&mut self.interleaved)
        } else {
            let scale = 1.0 / self.channels as f32;
            self.interleaved
                .chunks_exact(self.channels)
                .map(|frame| frame.iter().sum::<f32>() * scale)
                .collect()
        };
        match &self.resampler {
            Some(resampler) => out.extend(resampler.resample(&mono, last)),
            None => out.extend_from_slice(&mono),
        }
        if self.channels == 1 {
            // Give the buffer back for the next chunk.
            self.interleaved = mono;
        }
        Ok(!last)
    }

    fn progress(&self) -> f32 {
        if self.frames == 0 {
            1.0
        } else {
            (self.frames_read as f64 / self.frames as f64) as f32
        }
    }
}

/// Samples already in memory, at [`SAMPLE_RATE`], read in chunks; for tests and short
/// clips.
#[derive(Debug, Clone)]
pub struct SliceSource<'a> {
    samples: &'a [f32],
    position: usize,
    chunk: usize,
}

impl<'a> SliceSource<'a> {
    /// Reads `samples` `chunk` samples at a time.
    pub fn new(samples: &'a [f32], chunk: usize) -> Self {
        Self {
            samples,
            position: 0,
            chunk: chunk.max(1),
        }
    }
}

impl AudioSource for SliceSource<'_> {
    fn read(&mut self, out: &mut Vec<f32>) -> Result<bool> {
        let end = (self.position + self.chunk).min(self.samples.len());
        out.extend_from_slice(&self.samples[self.position..end]);
        self.position = end;
        Ok(end < self.samples.len())
    }

    fn progress(&self) -> f32 {
        if self.samples.is_empty() {
            1.0
        } else {
            self.position as f32 / self.samples.len() as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, spec: hound::WavSpec, frames: usize, value: impl Fn(usize) -> f32) {
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..frames {
            for _ in 0..spec.channels {
                match spec.sample_format {
                    SampleFormat::Float => writer.write_sample(value(i)).unwrap(),
                    SampleFormat::Int => writer
                        .write_sample((value(i) * f32::from(i16::MAX)) as i16)
                        .unwrap(),
                }
            }
        }
        writer.finalize().unwrap();
    }

    fn read_all(source: &mut dyn AudioSource) -> Vec<f32> {
        let mut out = Vec::new();
        while source.read(&mut out).unwrap() {}
        out
    }

    #[test]
    fn reads_float_stereo_48k_as_16k_mono() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("others.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        // Three seconds of a 440 Hz tone at half scale.
        write_wav(&path, spec, 144_000, |i| {
            0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin()
        });
        let mut source = WavSource::open(&path).unwrap();
        assert_eq!(source.sample_rate(), 48_000);
        assert_eq!(source.frames(), 144_000);
        let samples = read_all(&mut source);
        assert_eq!(source.progress(), 1.0);
        assert!(
            (samples.len() as i64 - 48_000).abs() <= 16,
            "{} samples",
            samples.len()
        );
        let peak = samples[1_000..47_000]
            .iter()
            .fold(0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.02, "peak {peak}");
    }

    #[test]
    fn reads_16_bit_pcm_at_16k_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("me.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        write_wav(
            &path,
            spec,
            40_000,
            |i| if i % 2 == 0 { 0.25 } else { -0.25 },
        );
        let samples = read_all(&mut WavSource::open(&path).unwrap());
        assert_eq!(samples.len(), 40_000);
        assert!((samples[0] - 0.25).abs() < 1e-3);
        assert!((samples[1] + 0.25).abs() < 1e-3);
    }

    #[test]
    fn an_empty_file_has_no_samples() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("me.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        write_wav(&path, spec, 0, |_| 0.0);
        let mut source = WavSource::open(&path).unwrap();
        let mut out = Vec::new();
        assert!(!source.read(&mut out).unwrap());
        assert!(out.is_empty());
    }

    #[test]
    fn a_file_cut_short_is_read_up_to_where_it_ends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("others.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        // Two seconds written, then the file loses its last half second and a few
        // bytes more, ending inside a frame: the header still declares two seconds.
        write_wav(&path, spec, 96_000, |_| 0.25);
        let len = std::fs::metadata(&path).unwrap().len();
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(len - 24_000 * 8 - 6).unwrap();
        drop(file);

        let mut source = WavSource::open(&path).unwrap();
        assert_eq!(source.frames(), 96_000);
        let samples = read_all(&mut source);
        // 1.5 s minus the cut frame, at 16 kHz.
        assert!(
            (samples.len() as i64 - 24_000).abs() <= 16,
            "{} samples",
            samples.len()
        );
        assert_eq!(source.frames(), 72_000 - 1);
        assert_eq!(source.progress(), 1.0);
        assert!(samples[100..23_000].iter().all(|s| (s - 0.25).abs() < 1e-3));
    }

    #[test]
    fn missing_and_invalid_files_have_their_own_codes() {
        let dir = tempfile::tempdir().unwrap();
        let missing = WavSource::open(&dir.path().join("none.wav")).unwrap_err();
        assert_eq!(missing.code, error_code::AUDIO_MISSING);
        assert!(!missing.retryable);

        let junk = dir.path().join("junk.wav");
        std::fs::write(&junk, b"not a wav file at all").unwrap();
        let invalid = WavSource::open(&junk).unwrap_err();
        assert_eq!(invalid.code, error_code::AUDIO_INVALID);
    }

    #[test]
    fn slices_are_read_in_chunks() {
        let samples = vec![0.0; 10];
        let mut source = SliceSource::new(&samples, 4);
        let mut out = Vec::new();
        assert!(source.read(&mut out).unwrap());
        assert!(source.read(&mut out).unwrap());
        assert!(!source.read(&mut out).unwrap());
        assert_eq!(out.len(), 10);
    }
}
