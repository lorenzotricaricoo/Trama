# Downloaded models

The transcription model is not bundled with the app ([ADR 0007](../adr/0007-tech-stack.md)):
it is about 2.5 GB, and the user downloads it on purpose, from the onboarding or the
settings. This document describes where models live, how the download works and what
the interface can rely on. The code is in `crates/core/src/models/`.

## The transcription model

Parakeet Ultra exported for sherpa-onnx in **fp32**, with the **Silero** voice activity
detector, as chosen by spike [#10](https://github.com/lorenzotricaricoo/Trama/issues/10).
Every file is pinned by URL, size and SHA-256 in `crates/core/src/models/catalog.rs`:

| File | Size (bytes) | Source |
|------|-------------:|--------|
| `encoder.onnx` | 41,838,396 | Hugging Face `csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-ultra`, commit `4980063` |
| `encoder.weights` | 2,435,420,160 | same |
| `decoder.onnx` | 47,233,744 | same |
| `joiner.onnx` | 25,286,331 | same |
| `tokens.txt` | 93,939 | same |
| `silero_vad.onnx` | 643,854 | GitHub release `asr-models` of `k2-fsa/sherpa-onnx` |
| **Total** | **2,550,516,424** | |

- The Hugging Face URLs point at a **commit**, not at `main`, so the files cannot change
  under the same URL. The VAD comes from a release tag that upstream could replace; the
  checksum catches it if it does.
- `encoder.weights` holds the encoder's external weights: ONNX Runtime loads it from the
  folder of `encoder.onnx`, so the two stay together.
- The weights are **CC-BY-4.0** (Moondream's Parakeet Ultra, a post-training of NVIDIA's
  parakeet-tdt-0.6b-v3): the app must credit them, for example in the About window.
- Upgrading the model means pinning new files under a **new model id**, which is also the
  folder name, so an old download is never mistaken for the new one. The old folder can
  then be removed.

## Where models live

```
~/Library/Application Support/io.github.lorenzotricaricoo.trama/
├── Library/                              the user's library (data model)
└── Models/
    └── parakeet-tdt-0.6b-ultra-fp32/     one folder per model id
        ├── encoder.onnx …                verified files, final names
        ├── encoder.weights.part          a file being downloaded
        ├── SHA256SUMS                    written when every file is verified
        └── CORRUPT                       written when a file failed its checksum
```

- **Outside the library.** Models are not user data: a library backup or move should not
  carry 2.5 GB that can be downloaded again.
- **Application Support, not Caches.** macOS and cleaning tools may empty `Caches`, and
  losing the model would silently break transcription until a new 2.5 GB download.
- The core takes the models folder as a parameter (`ModelStore::new(models_dir, spec)`),
  like `Library::open` takes the library folder. The desktop app passes
  `<app data>/Models`; `TRAMA_MODELS` overrides it in development, for example to share
  one download between builds.

## Download

`ModelStore::download` fetches what is missing, file by file:

1. **Free space.** Before any request it measures the free space of the models folder
   and needs the bytes still missing plus a 256 MiB margin; otherwise it stops with
   `not_enough_space`.
2. **Resume.** Each file is written to `<name>.part`. If a part exists, the request asks
   for the rest with `Range: bytes=<size of part>-`. A server that ignores the range
   (`200`) or answers another range makes the file start over.
3. **Checksum.** The SHA-256 is computed while writing (the existing part is hashed
   first). A file gets its final name only when its checksum matches. When every file is
   in place, `SHA256SUMS` marks the model ready, so the next start doesn't hash 2.5 GB
   again.
4. **Retries.** Network errors, `429` and `5xx` are retried, resuming, with a growing
   pause. An attempt that reaches further than any before doesn't count against the
   limit of four; one that throws data away and starts over does. A resumed file that
   fails its checksum (the old part may come from a different upstream file) is
   downloaded once more from the start.
5. **Cancellation.** A flag checked between reads, and while hashing, stops the download
   and keeps the parts. Every read also has a timeout, so a stalled connection is
   noticed and resumed.

`ModelStore::verify` hashes every file again; a file that doesn't match is deleted and
the model becomes corrupt. `ModelStore::remove` deletes the model folder, parts included.

The download does not use the [job queue](data-model.md): jobs target a source, and a
model has none. `ModelManager` runs downloads and checks on its own thread instead, one
task at a time, and sends the status to an event sink after every change, like the job
runner. Events are sent while the manager's lock is held, so they arrive in the order of
the changes. A removal stops the running task, waits for it and deletes the folder; no
task can start meanwhile. A task that panics ends with the error code `panicked`. No
state is stored in the database: the files on disk are the state, so after a restart a
new download resumes from the parts.

## What the interface sees

The desktop shell exposes `model_status`, `download_model`, `cancel_model_download`,
`verify_model` and `remove_model`, and sends the `model` event after every change. The
commands run off the main thread; downloads and checks continue in the background and
report through events. A failed command returns an error code, never text.

```json
{ "state": "downloading", "downloadedBytes": 1234567, "totalBytes": 2550516424, "errorCode": null, "progress": 0.0005 }
```

| State | Meaning |
|-------|---------|
| `missing` | Not downloaded, or partly: `downloadedBytes > 0` means a download can resume |
| `downloading` | Being downloaded; progress events about every 0.1% |
| `verifying` | The files are being hashed again (`verify_model`); `progress` counts the bytes hashed |
| `ready` | Every file is present and was verified |
| `corrupt` | A file failed its checksum or changed size on disk; downloading again fixes it |

`errorCode` holds the reason of the last failed download, for the interface to translate
(the core never returns text): `not_enough_space`, `network`, `http_status`,
`size_mismatch`, `checksum_mismatch`, `io`, `panicked`. A cancelled download has no
error.

## Testing

`crates/core/tests/models.rs` runs every case against a local fake HTTP server, with no
real download: a dropped connection resumed with a range (through a redirect), a resume
after a restart, a wrong checksum (corrupt, then downloaded again), a file damaged on
disk, a bad partial file, a server that ignores ranges, not enough space, HTTP errors,
retries that make no progress, cancellation, event order after a fast failure,
concurrent removals and a panic in the download. Two `#[ignore]`d tests use the network:

```sh
# Every pinned URL still serves the pinned size (one byte per file)
cargo test -p trama-core --test models the_pinned_urls -- --ignored

# The real download, about 2.5 GB, into TRAMA_MODELS or a temporary folder
TRAMA_MODELS=/tmp/trama-models cargo test --release -p trama-core --test models the_real_transcription -- --ignored --nocapture
```
