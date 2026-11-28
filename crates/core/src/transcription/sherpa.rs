//! The real engine: Parakeet Ultra fp32 and Silero VAD through sherpa-onnx, loaded from
//! the models folder managed by [`crate::models`].

use std::path::{Path, PathBuf};

use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig, SileroVadModelConfig,
    VadModelConfig, VoiceActivityDetector,
};

use super::{
    Engine, EngineLoader, Result, SAMPLE_RATE, SpeechRecognizer, SpeechSpan, TranscriptionError,
    VadSettings, VoiceDetector, error_code,
};
use crate::models::{ModelSpec, ModelState, ModelStore, files, transcription_model};

/// Recognizer threads: four performance cores gave the best speed in spike #10 (more
/// threads were slower).
const MAX_THREADS: usize = 4;

/// Samples fed to the detector at a time: one Silero window.
const VAD_WINDOW: usize = 512;

/// Loads the transcription model from a models folder, the same folder (and model
/// specification) the [`crate::models::ModelManager`] downloads into.
#[derive(Debug, Clone)]
pub struct SherpaLoader {
    models_dir: PathBuf,
    spec: ModelSpec,
    settings: VadSettings,
}

impl SherpaLoader {
    /// The pinned transcription model ([`transcription_model`]) in `models_dir`, with the
    /// detector settings of ADR 0007.
    pub fn new(models_dir: impl Into<PathBuf>) -> Self {
        Self::with_spec(models_dir, transcription_model())
    }

    /// Another model specification with the same files, for tests.
    pub fn with_spec(models_dir: impl Into<PathBuf>, spec: ModelSpec) -> Self {
        Self {
            models_dir: models_dir.into(),
            spec,
            settings: VadSettings::ADR_0007,
        }
    }

    /// Loads the model, checking first that it was downloaded and verified.
    pub fn load_engine(&self) -> Result<SherpaEngine> {
        let store = ModelStore::new(&self.models_dir, self.spec.clone());
        let status = store.status().map_err(|err| {
            TranscriptionError::retryable(error_code::MODEL_LOAD_FAILED, err.to_string())
        })?;
        match status.state {
            ModelState::Ready => {}
            ModelState::Corrupt => {
                return Err(TranscriptionError::permanent(
                    error_code::MODEL_CORRUPT,
                    store.dir().display(),
                ));
            }
            _ => {
                return Err(TranscriptionError::permanent(
                    error_code::MODEL_MISSING,
                    store.dir().display(),
                ));
            }
        }

        let path = |name: &str| path_string(&store.file(name));
        let threads = std::thread::available_parallelism()
            .map_or(MAX_THREADS, |n| n.get().min(MAX_THREADS)) as i32;
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.transducer = OfflineTransducerModelConfig {
            encoder: Some(path(files::ENCODER)),
            decoder: Some(path(files::DECODER)),
            joiner: Some(path(files::JOINER)),
        };
        config.model_config.tokens = Some(path(files::TOKENS));
        config.model_config.model_type = Some("nemo_transducer".into());
        config.model_config.num_threads = threads;
        config.model_config.provider = Some("cpu".into());
        config.decoding_method = Some("greedy_search".into());
        let recognizer = OfflineRecognizer::create(&config).ok_or_else(|| {
            TranscriptionError::retryable(
                error_code::MODEL_LOAD_FAILED,
                format!(
                    "sherpa-onnx could not load the model in {}",
                    store.dir().display()
                ),
            )
        })?;

        let s = self.settings;
        let vad = VadModelConfig {
            silero_vad: SileroVadModelConfig {
                model: Some(path(files::VAD)),
                threshold: s.threshold,
                min_silence_duration: s.min_silence_s,
                min_speech_duration: s.min_speech_s,
                window_size: VAD_WINDOW as i32,
                max_speech_duration: s.max_speech_s,
            },
            sample_rate: SAMPLE_RATE as i32,
            num_threads: 1,
            provider: Some("cpu".into()),
            ..Default::default()
        };
        // Fail now rather than at the first track if the detector can't load.
        SherpaDetector::new(&vad, &s)?;

        Ok(SherpaEngine {
            recognizer: SherpaRecognizer(recognizer),
            vad,
            settings: s,
            description: format!(
                "{} (sherpa-onnx {}, Silero VAD)",
                self.spec.id,
                sherpa_onnx::version()
            ),
        })
    }
}

impl EngineLoader for SherpaLoader {
    fn load(&self) -> Result<Box<dyn Engine>> {
        Ok(Box::new(self.load_engine()?))
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Parakeet and Silero, loaded.
pub struct SherpaEngine {
    recognizer: SherpaRecognizer,
    vad: VadModelConfig,
    settings: VadSettings,
    description: String,
}

impl std::fmt::Debug for SherpaEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SherpaEngine")
            .field("description", &self.description)
            .finish_non_exhaustive()
    }
}

impl Engine for SherpaEngine {
    fn description(&self) -> String {
        self.description.clone()
    }

    fn settings(&self) -> VadSettings {
        self.settings
    }

    fn detector(&mut self) -> Result<Box<dyn VoiceDetector>> {
        Ok(Box::new(SherpaDetector::new(&self.vad, &self.settings)?))
    }

    fn recognizer(&mut self) -> &mut dyn SpeechRecognizer {
        &mut self.recognizer
    }
}

struct SherpaRecognizer(OfflineRecognizer);

impl SpeechRecognizer for SherpaRecognizer {
    fn recognize(&mut self, samples: &[f32]) -> Result<String> {
        let stream = self.0.create_stream();
        stream.accept_waveform(SAMPLE_RATE as i32, samples);
        self.0.decode(&stream);
        stream
            .get_result()
            .map(|result| clean_text(&result.text))
            .ok_or_else(|| {
                TranscriptionError::retryable(
                    error_code::RECOGNITION_FAILED,
                    format!("no result for {} samples", samples.len()),
                )
            })
    }
}

/// The recognized text without the model's special tokens: `<unk>` for a piece it could
/// not name, and markers like `<|nospeech|>`. They mean nothing to a reader.
fn clean_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let tail = &rest[open..];
        let special = if tail.starts_with("<unk>") {
            Some("<unk>".len())
        } else if tail.starts_with("<|") {
            tail.find("|>").map(|end| end + 2)
        } else {
            None
        };
        match special {
            Some(len) => {
                out.push(' ');
                rest = &tail[len..];
            }
            None => {
                out.push('<');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Silero VAD, fed one window at a time.
struct SherpaDetector {
    vad: VoiceActivityDetector,
    /// Samples not yet a whole window.
    partial: Vec<f32>,
}

impl SherpaDetector {
    fn new(config: &VadModelConfig, settings: &VadSettings) -> Result<Self> {
        // The detector buffers the open stretch: room for the longest one and then some.
        let buffer_s = (settings.max_speech_s + 10.0).max(30.0);
        let vad = VoiceActivityDetector::create(config, buffer_s).ok_or_else(|| {
            TranscriptionError::retryable(
                error_code::MODEL_LOAD_FAILED,
                "sherpa-onnx could not load the voice activity detector",
            )
        })?;
        Ok(Self {
            vad,
            partial: Vec::with_capacity(VAD_WINDOW),
        })
    }
}

impl VoiceDetector for SherpaDetector {
    fn accept(&mut self, samples: &[f32]) {
        let mut rest = samples;
        if !self.partial.is_empty() {
            let take = (VAD_WINDOW - self.partial.len()).min(rest.len());
            self.partial.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.partial.len() < VAD_WINDOW {
                return;
            }
            self.vad.accept_waveform(&self.partial);
            self.partial.clear();
        }
        let whole = rest.len() / VAD_WINDOW * VAD_WINDOW;
        for window in rest[..whole].chunks_exact(VAD_WINDOW) {
            self.vad.accept_waveform(window);
        }
        self.partial.extend_from_slice(&rest[whole..]);
    }

    fn flush(&mut self) {
        if !self.partial.is_empty() {
            self.vad.accept_waveform(&self.partial);
            self.partial.clear();
        }
        self.vad.flush();
    }

    fn pop(&mut self) -> Option<SpeechSpan> {
        let segment = self.vad.front()?;
        let start = segment.start().max(0) as u64;
        let span = SpeechSpan {
            start,
            end: start + segment.n().max(0) as u64,
        };
        drop(segment);
        self.vad.pop();
        Some(span)
    }

    fn in_speech(&self) -> bool {
        self.vad.detected()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_tokens_are_removed_from_the_text() {
        assert_eq!(clean_text(" Ciao, <unk> come stai?"), "Ciao, come stai?");
        assert_eq!(clean_text("<|nospeech|>"), "");
        assert_eq!(clean_text("visto<unk>verificare"), "visto verificare");
        assert_eq!(clean_text("a < b e c <| d"), "a < b e c <| d");
    }
}
