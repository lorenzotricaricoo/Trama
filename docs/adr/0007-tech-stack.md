# 0007. Tech stack

- **Status:** Accepted; the *Packaging* row is replaced by [ADR 0008](0008-distribution-without-notarization.md)
- **Date:** 2026-11-27

## Context

[ADR 0005](0005-macos-first-windows-ready.md) ships the MVP on macOS with a stack that
can also run on Windows, and leaves the stack to be decided after the spikes. The
candidates were a Rust core with a web UI (Tauri) and a Rust core with a native UI per
platform.

The spikes answered the risky questions:

- **Meeting audio capture** ([#11](https://github.com/lorenzotricaricoo/Trama/issues/11)):
  a Rust program records the microphone (`cpal`) and the system audio (Core Audio
  process tap) on two tracks. Writing is progressive, a crash loses at most about
  0.1 s, and the permissions survive rebuilds when the app is signed with a stable
  identity. Windows has equivalent APIs (WASAPI loopback, process loopback).
- **Local transcription** ([#10](https://github.com/lorenzotricaricoo/Trama/issues/10)):
  Parakeet Ultra runs from Rust through sherpa-onnx (ONNX Runtime). With the fp32 export
  and voice activity detection it reaches 2.15% word error rate on the Italian FLEURS
  set (benchmark engine: 1.82%), invents no words on silence, and runs about 23× faster
  than real time on Apple Silicon. The same runtime and model are published for Windows.

The principles also constrain the choice: the core runs without the UI
([A1](../principles.md#a1--the-core-is-independent-of-the-ui)), one process owns the
data ([A2](../principles.md#a2--one-owner-of-the-data)), the data model is versioned
([A3](../principles.md#a3--the-data-model-is-designed-up-front)), and localization,
stable signing and automated tests are there from day one
([A5](../principles.md#a5--foundations-from-day-one)). The visual direction
([ADR 0006](0006-visual-direction-and-design-system.md)) needs translucent windows and
rich, smooth animation.

## Decision

| Area | Choice |
|------|--------|
| **Core language** | **Rust**, as a Cargo workspace. The core library (capture, transcription, curator, index, storage) has no UI dependency and is tested headless |
| **Audio capture** | `cpal` for the microphone; Core Audio process tap through `objc2-core-audio` for system audio, ScreenCaptureKit as fallback. Each track is written progressively with a timing sidecar used to realign the tracks. On Windows: WASAPI loopback and process loopback behind the same interface |
| **Transcription** | `sherpa-onnx` (Rust crate, ONNX Runtime linked statically), **Parakeet Ultra fp32** export, Silero VAD (1.0 s minimum silence, 30 s maximum segment, 500 ms padding), on the CPU. The model is downloaded on explicit user action and verified by checksum |
| **UI** | **Tauri 2** with a web frontend in **React + TypeScript** (Vite). Components on **React Aria Components**, as foreseen by ADR 0006; animation with **Motion**. Translucent window through Tauri's window effects (Liquid Glass or vibrancy on macOS, Mica on Windows), with the opaque fallback already defined in the design tokens |
| **Storage** | **SQLite**, one database file per user library, accessed only by the core process. Full-text search with **FTS5**, vectors with **sqlite-vec**. Schema changes are versioned migrations. Each vector records the embedding model that produced it ([ADR 0003](0003-hybrid-ai-providers.md)) |
| **LLM and embedding providers** | OpenAI-compatible HTTP clients in the core. API keys in the operating system's credential store (Keychain on macOS, Credential Manager on Windows) |
| **MCP server and CLI** | Separate binaries that talk to the running core through a local API (Unix socket on macOS, named pipe on Windows). They never open the database |
| **Packaging** | Tauri bundler: `.app` and `.dmg`, signed with a stable identity from the first build (Developer ID for distribution) and notarized; updates through Tauri's updater. Windows installers when Windows ships. Models are not bundled |

## Consequences

- **One codebase** for the UI on macOS and Windows; the prototype, its tokens and its
  animations carry over to the real frontend.
- Everything performance-sensitive or platform-specific (audio, transcription,
  storage, providers) is Rust in the core. Only the capture backend is written per
  platform.
- The frontend is a client of the core through Tauri commands and events, which keeps
  the UI replaceable and the core testable on its own.
- A single SQLite file keeps backup and export simple; sqlite-vec is enough for one
  user's library, and the index can be rebuilt from the sources at any time.
- Native libraries (ONNX Runtime, sherpa-onnx) are pinned and verified at build time
  instead of being downloaded unverified by build scripts.
- Transcription peaks at about 2.8 GB of memory with the fp32 model; it runs after the
  meeting, not during it, and memory use is monitored.

**Risks carried forward:**

- **Windows not tested.** Neither capture nor transcription was run on Windows; the
  APIs and prebuilt libraries exist, but the first Windows build may find surprises.
- **Webview differences.** The UI runs in WebKit on macOS and WebView2 on Windows;
  visual effects and animations need testing on both.
- **Audio edge cases** from the capture spike: headsets on separate clocks, device
  changes during a call, and the system track starting only at the first sound.
- **Real meetings vs. benchmark:** the real-meeting score (12.05%) was measured against
  a reference that favours the benchmark engines; it is re-checked on new recordings.
- **Young dependencies:** sherpa-onnx 2.x and the Liquid Glass window effects are
  recent; versions are pinned and upgraded deliberately.

## Alternatives considered

- **Native UI per platform** (SwiftUI/AppKit on macOS, WinUI 3 on Windows, over the
  Rust core): the most native result, but two interfaces to write and keep in sync,
  and every animation designed twice.
- **Electron**: a mature web shell, but it ships its own browser engine (larger and
  heavier app) and pushes the backend towards Node instead of a Rust core.
- **Core ML (FluidAudio) for transcription**: slightly more accurate in the benchmark
  (1.82% vs 2.15% on FLEURS) and able to use the Neural Engine, but macOS-only. It can
  be added later as a macOS engine behind the transcription interface, after a new
  benchmark ([ADR 0002](0002-local-transcription-parakeet.md)).
- **ONNX Runtime directly (`ort`)**: the same runtime, but the audio frontend, decoder
  and VAD would have to be written by hand.
- **A separate vector database** (for example LanceDB or Qdrant): more capacity than one
  user's library needs, and a second store to keep consistent with SQLite.
