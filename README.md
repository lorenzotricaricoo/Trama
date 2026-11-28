# Trama

Desktop app for meetings, transcription and dictation — macOS & Windows.

> **Status: early development.** The first pre-release of phase 1 is out for macOS
> ([changelog](CHANGELOG.md)): it records, transcribes and summarizes meetings, but
> search, the Home, onboarding and the menu bar are still missing. A Rust core with a
> Tauri and React interface ([ADR 0007](docs/adr/0007-tech-stack.md)).

## Goals

- **Meetings** — record and transcribe meetings.
- **Transcription** — turn audio files into searchable text.
- **Dictation** — speak and get text wherever you are writing.

## Platforms

| Platform | Status  |
|----------|---------|
| macOS    | Pre-release (macOS 14.2 or later, Apple silicon) |
| Windows  | Planned |

## Install

Trama is signed but not notarized by Apple
([ADR 0008](docs/adr/0008-distribution-without-notarization.md)), so macOS blocks the
downloaded `.dmg` the first time, and may block the app once more. Each block is lifted
the same way:

1. Download the `.dmg` from the [latest release](https://github.com/lorenzotricaricoo/Trama/releases).
2. Open it. macOS says it cannot verify the file: click **Done**, not *Move to Trash*.
3. Open **System Settings → Privacy & Security**, scroll to *Security*, click
   **Open Anyway** next to the message about the `.dmg` and confirm.
4. In the window that opens, drag **Trama** to **Applications**.
5. Open Trama. If macOS shows the same message for Trama, repeat step 3 for it.

If you trust the file you downloaded, you can instead remove the quarantine flag macOS
puts on downloads, so neither the `.dmg` nor the app is blocked:

```sh
xattr -d com.apple.quarantine ~/Downloads/Trama_<version>_aarch64.dmg
```

From then on Trama opens normally. Before the first meeting, in Trama's settings:
download the transcription model (**Transcription**) and add an AI provider with a
model for the summary (**Providers**). The first recording asks for the microphone and
the system audio.

Your data stays on your Mac, in
`~/Library/Application Support/io.github.lorenzotricaricoo.trama`.

To update, download the new `.dmg` the same way and replace Trama in Applications; your
data and settings stay.

## Development

### Requirements

- macOS with Xcode or the Command Line Tools
- [Rust](https://rustup.rs/): the version is pinned in `rust-toolchain.toml` and installed
  automatically by `rustup`
- [Node.js](https://nodejs.org/): the version is pinned in `.nvmrc` (`nvm use`)

### Layout

| Path | Content |
|------|---------|
| `crates/core` | Core library, independent of the UI |
| `apps/desktop` | Desktop app: React frontend (`src/`) and Tauri shell (`src-tauri/`) |
| `locales` | Interface text in [Fluent](https://projectfluent.org/), one folder per language |
| `docs` | Vision, principles, design and ADRs |

### Commands

From `apps/desktop`, after `npm ci`:

| Command | What it does |
|---------|--------------|
| `npm run tauri dev` | Runs the app with hot reload |
| `npm run bundle` | Builds `Trama.app` and the `.dmg`, signed, in `target/release/bundle` ([signing setup](docs/development/macos-signing.md)) |
| `npm test` | Frontend tests (Vitest) |
| `npm run lint` | ESLint, including the check for text not taken from `locales/` |
| `npm run typecheck` | TypeScript |
| `npm run format` | Prettier |

From the repository root:

| Command | What it does |
|---------|--------------|
| `cargo test --workspace` | Rust tests, including the check that every language has the same messages |
| `cargo clippy --workspace --all-targets -- -D warnings` | Rust lints |
| `cargo fmt --all` | Rust formatting |

The interface follows the system language (Italian or English, otherwise English). To
try another language, set `TRAMA_LOCALE`, for example `TRAMA_LOCALE=en npm run tauri dev`.

The app keeps its data in `~/Library/Application Support/io.github.lorenzotricaricoo.trama/Library`.
To keep development data apart, set `TRAMA_LIBRARY` to another folder, for example
`TRAMA_LIBRARY=/tmp/trama-dev npm run tauri dev`.
The transcription model (about 2.5 GB) is downloaded into `Models`, next to the library;
`TRAMA_MODELS` points development builds at another folder, so they can share one
download ([downloaded models](docs/architecture/models.md)).

The first Rust build downloads the prebuilt sherpa-onnx and ONNX Runtime libraries
(about 22 MB) used for transcription, and stops before linking if the archive or the
libraries extracted from it don't match the checksums pinned in `crates/core/build.rs` ([native libraries](docs/architecture/transcription.md#native-libraries)).
Tests that need the transcription model are ignored by default; see
[transcription tests](docs/architecture/transcription.md#tests) to run them.

## Documentation

- [Vision](docs/vision.md) — what Trama is, how it works, and the MVP scope
- [Information architecture](docs/design/information-architecture.md) — objects, screens, navigation and flows of the MVP
- [Visual direction](docs/design/visual-direction.md) — look, colours, type, motion and design tokens
- [Data model](docs/architecture/data-model.md) — how sources, meetings and notes are stored, versioned and searched
- [Audio capture](docs/architecture/audio-capture.md) — how a meeting is recorded on two aligned tracks ([manual checks](docs/development/audio-capture.md))
- [Downloaded models](docs/architecture/models.md) — the pinned transcription model, where it lives and how it is downloaded
- [AI providers](docs/architecture/providers.md) — providers, models per task and how calls stay robust
- [Transcription](docs/architecture/transcription.md) — the local engine, how both tracks become one timeline, the background job and its tests
- [Releasing](docs/development/releasing.md) — versions, how a release is built and published, manual checks
- [Principles](docs/principles.md) — rules for process and architecture
- [Architecture Decision Records](docs/adr/) — decisions and their reasons

## Security

To report a vulnerability, see [SECURITY.md](SECURITY.md).

## License

Released under the [MIT License](LICENSE). Third-party models, libraries and data, with
their licenses and attributions, are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
