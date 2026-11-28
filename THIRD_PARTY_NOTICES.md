# Third-party notices

Trama is released under the [MIT License](LICENSE). It uses the following third-party
works, which keep their own licenses. Rust and npm dependencies are listed with their
licenses in `Cargo.lock` and `apps/desktop/package-lock.json`; this file covers the
models, data and native libraries that need an attribution of their own.

## Transcription model

Downloaded by the user on purpose, not bundled with the app
([downloaded models](docs/architecture/models.md)).

**Parakeet Ultra** (`parakeet-tdt-0.6b-ultra`, fp32 export for sherpa-onnx)

- Weights by [Moondream](https://huggingface.co/moondream/parakeet-ultra), a
  post-training of NVIDIA's
  [parakeet-tdt-0.6b-v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3).
- Exported to ONNX by the sherpa-onnx project:
  [csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-ultra](https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-ultra).
- License: [Creative Commons Attribution 4.0 International (CC-BY-4.0)](https://creativecommons.org/licenses/by/4.0/).
  Trama uses the weights unmodified.

**Silero VAD** (`silero_vad.onnx`), voice activity detector

- By [Silero Team](https://github.com/snakers4/silero-vad), as published by the
  sherpa-onnx project.
- License: MIT.

## Native libraries

Linked statically into the app, pinned and verified by checksum at build time
([transcription](docs/architecture/transcription.md#native-libraries)).

- **sherpa-onnx** 2.0.3, by the k2-fsa project, <https://github.com/k2-fsa/sherpa-onnx>.
  License: Apache-2.0.
- **ONNX Runtime**, by Microsoft, <https://github.com/microsoft/onnxruntime>, included in
  the sherpa-onnx static libraries. License: MIT.

## Test data

Downloaded by tests that are not run by default; not bundled with the app and not stored
in the repository.

- **FLEURS** (Italian test split), by Google: Conneau et al., *FLEURS: Few-shot Learning
  Evaluation of Universal Representations of Speech*, 2022,
  <https://huggingface.co/datasets/google/fleurs>. License: CC-BY-4.0.
