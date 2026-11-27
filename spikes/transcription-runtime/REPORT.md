# Spike #10: local transcription runtime (report)

## Answer

**Yes, with one condition still open.** Parakeet runs from Rust through **sherpa-onnx**
(ONNX Runtime underneath) on the CPU. On Apple Silicon it runs at **about 16× real
time**. It invented **0 words** in 40 minutes of silence and noise. The same crate and
model are published for Windows x64.

Still open:

- **Benchmark WER.** The 8.8% figure needs a run on the private benchmark set. The
  harness is ready and takes one command (see below).
- **Mixed Italian/English on real meetings.** Synthetic audio scores well (3.1% WER),
  but when a single decoded chunk contains **both languages** the model can **silently
  drop the sentences in the other language** (up to 43% WER on one file). Short chunks,
  cut by VAD at every pause of 0.5 s or more, avoid it. Real meetings may not have those
  pauses, so this must be checked on real meeting audio.

## Measurable outcomes

| Outcome (issue #10) | Target | Result | Status |
|---|---|---|---|
| WER on the benchmark set | within 1 pt of 8.8% | Not run: the private set is not available. Harness ready (`trama-stt eval`), plus `trama-stt score` to re-score the original benchmark outputs with the same normalization | **pending maintainer run** |
| WER on mixed IT/EN | reported, with error examples | Synthetic (`say`), Ultra int8 + VAD: **3.09%** (8 files, 421 words). v3 int8: 3.56%. Robotic voices: 15.2%. 17-min long file: 2.53%. Examples below | **reported (synthetic only)**; real audio pending |
| Words invented on silence | 0 | **0** on 8 × 5 min (digital silence, white noise at −60/−40/−25 dBFS, room tone at −50/−35/−20 dBFS, keyboard-like clicks), with VAD and with the model alone, for both Ultra and v3. **0** insertions in a 4-min file with 60 s pauses between turns | **met** (synthetic noise) |
| Speed on Apple Silicon | ≥ 5× real time | M3 Pro, 17.1-min file, CPU: **15.8×** with 4 threads, 9.1× with 2, 13.9× with 8. Model load 0.8–1.0 s. Peak memory ≈ 1.25 GB | **met** |
| Same runtime and model on Windows | CPU at minimum | Official prebuilt sherpa-onnx v2.0.3 static libraries for `win-x64` and `win-arm64`, picked automatically by the crate; upstream CI tests the Rust crate on Windows. Model files are platform-independent. `cargo check --target x86_64-pc-windows-msvc` passes. **Not run on Windows** | **met on paper**; Windows run pending |

## Chosen runtime and model

- **Runtime: `sherpa-onnx` 2.0.3** (crates.io), the official Rust crate from k2-fsa,
  Apache-2.0. It links ONNX Runtime 1.28.2 statically into one 27 MB binary and covers
  the whole pipeline: NeMo transducer, TDT decoder, mel frontend, Silero VAD,
  resampling.
- **Model: `sherpa-onnx-nemo-parakeet-tdt-0.6b-ultra-int8`**: Moondream's *Parakeet
  Ultra*, a post-training of NVIDIA `parakeet-tdt-0.6b-v3`, exported to ONNX by
  sherpa-onnx. 25 European languages including Italian and English. **int8**
  (652 MB encoder). Weights are **CC-BY-4.0**, so attribution is required.
- **Comparison model:** `parakeet-tdt-0.6b-v3-int8`: same speed, same 0 on silence,
  slightly worse on mixed audio (3.56% vs 3.09%).
- **Chunking: Silero VAD** (built into sherpa-onnx): threshold 0.5, minimum silence
  0.5 s, maximum segment 20 s, 200 ms padding. VAD is **required**: decoding a whole
  file at once dropped entire sentences (18.3% WER on the mixed set, against 3.1%).
- **Execution provider: CPU.** The prebuilt ONNX Runtime has no CoreML execution
  provider (`--provider coreml` falls back to CPU). CPU is already 3× above the target.
- **`ort` was not tried:** same ONNX Runtime, but the frontend, TDT decoding and VAD
  would have to be rewritten, with no quality reason to do it.

## Mixed IT/EN: what the errors look like

Synthetic set: 8 files (2.7 min) of meeting-style text, either alternating Italian and
English voices or English terms inside Italian sentences, using macOS `say` voices.
Synthetic speech is a **weak proxy**: clean, no overlap, no far-field audio.

Examples, written `{reference→hypothesis}`, where `∅` means missing:

- English terms mangled: `sessione di {grooming→groaming}`, `meeting di
  {kickoff→kicav}`, `{stakeholder→stackholder}`. With robotic voices: `la
  {pipeline→peperina}`, `{refactoring→reflettore}`, `{sprint→screen}`.
- First word lost after a language switch: `said yesterday {certo→∅} il cliente`.
- English spelling normalized to American: `{summarise→summarize}`.
- **Whole sentences dropped when one chunk mixes languages.** With
  `--vad-min-silence 1.0` chunks span several turns and mixed WER rises from 3.1% to
  11–14%, almost all deletions (one file 43%, another 31%). Whole-file decoding shows
  the same behaviour. The model **leaves text out; it does not invent it**.

Monolingual sanity checks: Italian 0.00% (103 words), English 0.92% (109 words).

## Files downloaded

All from the official sherpa-onnx GitHub releases or crates.io, stored in
`~/Library/Caches/trama-spikes/`; nothing in the repository.

| File | Source | Size |
|---|---|---|
| `sherpa-onnx-nemo-parakeet-tdt-0.6b-ultra-int8.tar.bz2` | github.com/k2-fsa/sherpa-onnx, release `asr-models` | 487 MB (670 MB unpacked) |
| `sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2` | same | 487 MB (670 MB unpacked) |
| `silero_vad.onnx` | same | 0.6 MB |
| `sherpa-onnx-v2.0.3-osx-arm64-static-lib.tar.bz2` | release `v2.0.3`, fetched by the crate's build script | 22 MB |
| Rust crates | crates.io | via `Cargo.lock` |

`download_models.sh` verifies SHA-256 checksums. Not downloaded: the **fp32** exports,
which are only on a personal Hugging Face account; the int8 vs fp32 gap is unmeasured.

## Risks

1. **Language switches inside a chunk delete sentences.** Main risk for Trama.
   Mitigation: short chunks (VAD, 0.5 s minimum silence, ≤ 20 s segments); validate on
   real meetings; possibly a shorter maximum segment (8–10 s).
2. **Synthetic data only.** No real meeting audio evaluated; 3.1% is not comparable to
   8.8%.
3. **Benchmark comparability.** ADR 0002 does not describe the benchmark's
   normalization, chunking or quantization. The harness uses NFKC, lowercase,
   punctuation and apostrophes to spaces, accents kept, no number normalization.
   Re-score the original outputs with `trama-stt score` to compare like for like.
4. **int8 only;** fp32 not tested.
5. **Resources:** ≈ 1.25 GB peak memory, 4 performance cores busy while decoding
   (about 1 minute of CPU per 16 minutes of audio); no Neural Engine or GPU path.
6. **Build supply chain:** the build script downloads native libraries at build time
   without a checksum. Real builds should pin the archives (`SHERPA_ONNX_ARCHIVE_DIR` or
   `SHERPA_ONNX_LIB_DIR`). sherpa-onnx 2.x is young and ships often.
7. **Known upstream issue with very quiet input** (k2-fsa/sherpa-onnx#4024): not
   observed; `--normalize-gain` is available.

## Windows notes

- `sherpa-onnx-sys` maps `x86_64-pc-windows-msvc` to the official
  `win-x64-static-MT` library (127.6 MB; static CRT, no redistributable needed) and
  `aarch64` to `win-arm64`.
- Model files are the same on every platform.
- `DOCS_RS=1 cargo check --release --target x86_64-pc-windows-msvc` passes on macOS
  (type-check only; linking needs MSVC).
- To do: one real run on a Windows x64 machine.

## To finish the spike

Setup (macOS, from the repo root, on branch `spike/10-transcription-runtime`):

```sh
cd spikes/transcription-runtime
scripts/download_models.sh          # ~975 MB into ~/Library/Caches/trama-spikes/ (checksums verified)
cargo build --release
M=~/Library/Caches/trama-spikes/models
```

**1. Private benchmark set.** One audio file per recording plus a reference transcript
with the same name (`X.txt`; an empty `.txt` marks a silence-only file):

```sh
./target/release/trama-stt eval \
  --model $M/sherpa-onnx-nemo-parakeet-tdt-0.6b-ultra-int8 \
  --vad $M/silero_vad.onnx \
  /path/to/benchmark-folder
```

It prints WER per file and overall, speed and memory, and writes `results.tsv`,
`alignments.txt` and `*.hyp.txt` to `~/Library/Caches/trama-spikes/eval-out/<folder>/`.
To compare with the original 8.8% using the same normalization, save the benchmark's
outputs as `X.hyp.txt` next to each `X.txt` and run
`./target/release/trama-stt score /path/to/benchmark-folder`.

**2. Real mixed IT/EN meetings.** Same command on real excerpts with hand-corrected
references; then again with `--model $M/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8`
and with `--vad-max-speech 10` to test shorter chunks.

**3. Windows x64 (CPU).** With Rust (MSVC toolchain): copy the model folder,
`cargo build --release`, run `eval` on a few 16 kHz WAV files.
