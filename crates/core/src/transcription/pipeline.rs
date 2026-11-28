//! One track through the detector and the recognizer.
//!
//! The track is read a chunk at a time and fed to the detector. Each stretch of speech it
//! finds is decoded with [`VadSettings::padding_ms`] of audio on both sides, from a
//! buffer that keeps only the last few tens of seconds, so memory does not grow with the
//! length of the meeting.
//!
//! Decoding windows never overlap, so no word is decoded twice: a window ends where the
//! next stretch starts, and starts where the previous window ended. To know where the
//! next stretch starts, a stretch is decoded only once the next one is found, the track
//! is over, or the detector has been out of speech long enough after it that the next
//! stretch can't reach back into its padding. That matters when speech longer than
//! [`VadSettings::max_speech_s`] is split at a short pause.

use std::collections::VecDeque;

use super::{
    AudioSource, Result, SAMPLE_RATE, SpeechRecognizer, SpeechSpan, TranscriptionError,
    VadSettings, VoiceDetector,
};

/// Text recognized in one stretch of speech, in samples at [`SAMPLE_RATE`] from the start
/// of the track (without the padding); `end` is exclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpokenSegment {
    pub start: u64,
    pub end: u64,
    pub text: String,
}

/// Seconds of audio kept besides the stretches waiting to be decoded: enough for the
/// detector to report a stretch that started up to [`VadSettings::max_speech_s`] ago.
const KEEP_MARGIN_S: f32 = 5.0;

/// Transcribes one track. Stretches whose text is empty are left out.
///
/// `progress` receives the fraction of the track read; `should_stop` is checked after
/// every chunk and every stretch.
pub fn transcribe_track(
    audio: &mut dyn AudioSource,
    detector: &mut dyn VoiceDetector,
    recognizer: &mut dyn SpeechRecognizer,
    settings: &VadSettings,
    progress: &mut dyn FnMut(f32),
    should_stop: &dyn Fn() -> bool,
) -> Result<Vec<SpokenSegment>> {
    let rate = SAMPLE_RATE as f32;
    let pad = settings.padding_samples();
    // Out of speech this long after a stretch, the next one can't start inside its
    // padding: the detector reports speech after `min_speech_s`, plus a few windows.
    let settle = pad + ((settings.min_speech_s + 0.25) * rate) as u64;
    let keep =
        ((settings.max_speech_s + settings.min_silence_s + KEEP_MARGIN_S) * rate) as u64 + pad;

    let mut buffer = Buffer::default();
    let mut pending: VecDeque<SpeechSpan> = VecDeque::new();
    let mut decoded_to = 0u64;
    let mut chunk = Vec::new();
    let mut finished = false;
    let mut out = Vec::new();

    loop {
        if should_stop() {
            return Err(TranscriptionError::stopped());
        }
        if !finished {
            chunk.clear();
            finished = !audio.read(&mut chunk)?;
            detector.accept(&chunk);
            buffer.extend(&chunk);
            if finished {
                detector.flush();
            }
            while let Some(span) = detector.pop() {
                pending.push_back(span);
            }
        }

        while let Some(&span) = pending.front() {
            let next = pending.get(1).copied();
            let settled = !detector.in_speech() && buffer.end() >= span.end + settle;
            if next.is_none() && !finished && !settled {
                break;
            }
            pending.pop_front();
            let start = span
                .start
                .saturating_sub(pad)
                .max(decoded_to)
                .max(buffer.start);
            let end = (span.end + pad)
                .min(next.map_or(u64::MAX, |n| n.start))
                .min(buffer.end())
                .max(start);
            let text = if end > start {
                recognizer.recognize(buffer.slice(start, end))?
            } else {
                String::new()
            };
            decoded_to = end;
            let text = text.trim();
            if !text.is_empty() {
                out.push(SpokenSegment {
                    start: span.start,
                    end: span.end.max(span.start),
                    text: text.to_owned(),
                });
            }
            if should_stop() {
                return Err(TranscriptionError::stopped());
            }
        }

        let needed = pending
            .front()
            .map_or(u64::MAX, |span| span.start.saturating_sub(pad));
        buffer.trim(needed.min(buffer.end().saturating_sub(keep)));
        progress(audio.progress());

        if finished && pending.is_empty() {
            return Ok(out);
        }
    }
}

/// The most recent samples of a track, addressed by their index in the track.
#[derive(Debug, Default)]
struct Buffer {
    /// Index of `samples[0]` in the track.
    start: u64,
    samples: Vec<f32>,
}

impl Buffer {
    fn end(&self) -> u64 {
        self.start + self.samples.len() as u64
    }

    fn extend(&mut self, samples: &[f32]) {
        self.samples.extend_from_slice(samples);
    }

    /// Samples from `start` to `end` (track indices), clamped to what is buffered.
    fn slice(&self, start: u64, end: u64) -> &[f32] {
        let from = start
            .saturating_sub(self.start)
            .min(self.samples.len() as u64) as usize;
        let to = end
            .saturating_sub(self.start)
            .min(self.samples.len() as u64) as usize;
        &self.samples[from..to.max(from)]
    }

    /// Forgets the samples before `index`, in large steps to keep it cheap.
    fn trim(&mut self, index: u64) {
        let drop = index
            .saturating_sub(self.start)
            .min(self.samples.len() as u64) as usize;
        if drop >= SAMPLE_RATE as usize * 10 {
            self.samples.drain(..drop);
            self.start += drop as u64;
        }
    }
}
