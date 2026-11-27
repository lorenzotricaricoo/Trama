//! The pinned transcription model: where each file comes from, its size and checksum.
//!
//! Spike #10 and ADR 0007 chose Parakeet Ultra exported for sherpa-onnx in fp32, with
//! the Silero voice activity detector. The model files come from the sherpa-onnx
//! export on Hugging Face, pinned to one commit; the detector from the sherpa-onnx
//! GitHub release `asr-models`. Every file is verified against its SHA-256, so a file
//! that changes upstream is rejected instead of used.
//!
//! The model weights are CC-BY-4.0 (Moondream, from NVIDIA's parakeet-tdt-0.6b-v3):
//! the app must credit them.

use super::{ModelFile, ModelSpec};

/// Folder of the transcription model inside the models folder. A new pinned version
/// gets a new folder, so an old download is never mistaken for the new one.
pub const TRANSCRIPTION_MODEL_ID: &str = "parakeet-tdt-0.6b-ultra-fp32";

/// Hugging Face repository and commit of the fp32 export.
const HF_BASE: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-ultra/resolve/498006306bf134c16efde32386d42a9ee96005c9";

/// Silero VAD as published by sherpa-onnx.
const VAD_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx";

/// File names inside the model folder, for the transcription engine.
pub mod files {
    /// Encoder graph; its weights are in [`ENCODER_WEIGHTS`], next to it.
    pub const ENCODER: &str = "encoder.onnx";
    /// External weights of the encoder, loaded by ONNX Runtime from the same folder.
    pub const ENCODER_WEIGHTS: &str = "encoder.weights";
    pub const DECODER: &str = "decoder.onnx";
    pub const JOINER: &str = "joiner.onnx";
    pub const TOKENS: &str = "tokens.txt";
    /// Silero voice activity detector.
    pub const VAD: &str = "silero_vad.onnx";
}

/// Name, URL, size in bytes and SHA-256 of every file.
const TRANSCRIPTION_FILES: &[(&str, &str, u64, &str)] = &[
    (
        files::ENCODER,
        HF_BASE,
        41_838_396,
        "609ff3ce430c94f7404e31ef6f6c0704fc190c03af1542fc53121332c05b3ae2",
    ),
    (
        files::ENCODER_WEIGHTS,
        HF_BASE,
        2_435_420_160,
        "1209bc92c87a2f596767ae080aec04702d94a2a41eaba4cee5a89f69a9c22c12",
    ),
    (
        files::DECODER,
        HF_BASE,
        47_233_744,
        "b35bd094acefa4071a1eabe8680977429c30e9bfe4b3feb064a1fa880413852c",
    ),
    (
        files::JOINER,
        HF_BASE,
        25_286_331,
        "bc237a184a0cb925cf34e4e6b8f50dbe186c72a2fdf66d0b2ff7f72221ddcbce",
    ),
    (
        files::TOKENS,
        HF_BASE,
        93_939,
        "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
    ),
    (
        files::VAD,
        VAD_URL,
        643_854,
        "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6",
    ),
];

/// The transcription model chosen by ADR 0007: Parakeet Ultra fp32 and Silero VAD.
pub fn transcription_model() -> ModelSpec {
    let files = TRANSCRIPTION_FILES
        .iter()
        .map(|&(name, base, size, sha256)| ModelFile {
            name: name.to_owned(),
            url: if base == HF_BASE {
                format!("{base}/{name}")
            } else {
                base.to_owned()
            },
            size,
            sha256: sha256.to_owned(),
        })
        .collect();
    ModelSpec {
        id: TRANSCRIPTION_MODEL_ID.to_owned(),
        files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pinned_model_is_well_formed() {
        let spec = transcription_model();
        assert_eq!(spec.files.len(), 6);
        assert_eq!(spec.total_size(), 2_550_516_424);
        for file in &spec.files {
            assert!(file.url.starts_with("https://"), "{}", file.url);
            assert!(file.url.ends_with(&file.name), "{}", file.url);
            assert_eq!(file.sha256.len(), 64);
            assert!(file.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
        }
    }
}
