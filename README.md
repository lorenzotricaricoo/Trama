# Trama

Desktop app for meetings, transcription and dictation — macOS & Windows.

> **Status: early development.** No release is available yet. The tech stack is
> decided ([ADR 0007](docs/adr/0007-tech-stack.md)): a Rust core with a Tauri and React
> interface. Work on the first phase of the MVP has started.

## Goals

- **Meetings** — record and transcribe meetings.
- **Transcription** — turn audio files into searchable text.
- **Dictation** — speak and get text wherever you are writing.

## Platforms

| Platform | Status  |
|----------|---------|
| macOS    | Planned |
| Windows  | Planned |

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

## Documentation

- [Vision](docs/vision.md) — what Trama is, how it works, and the MVP scope
- [Information architecture](docs/design/information-architecture.md) — objects, screens, navigation and flows of the MVP
- [Visual direction](docs/design/visual-direction.md) — look, colours, type, motion and design tokens
- [Data model](docs/architecture/data-model.md) — how sources, meetings and notes are stored, versioned and searched
- [Audio capture](docs/architecture/audio-capture.md) — how a meeting is recorded on two aligned tracks ([manual checks](docs/development/audio-capture.md))
- [Downloaded models](docs/architecture/models.md) — the pinned transcription model, where it lives and how it is downloaded
- [AI providers](docs/architecture/providers.md) — providers, models per task and how calls stay robust
- [Principles](docs/principles.md) — rules for process and architecture
- [Architecture Decision Records](docs/adr/) — decisions and their reasons

## Security

To report a vulnerability, see [SECURITY.md](SECURITY.md).

## License

Released under the [MIT License](LICENSE).
