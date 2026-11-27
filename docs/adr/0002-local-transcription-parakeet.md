# 0002. Local transcription with Parakeet

- **Status:** Accepted
- **Date:** 2026-11-27

## Context

Meetings contain work information that should not leave the user's computer just to be
transcribed. Transcription must be accurate on real meetings, fast enough to process a
meeting in a fraction of its duration, and must not invent text — especially during
silence, which is frequent in meetings.

A benchmark was run on real meeting recordings, with blind listening, comparing
candidate engines.

| Engine | Word error rate | Speed | Words invented on silence |
|--------|-----------------|-------|---------------------------|
| Parakeet (Ultra) | **8.8%** | **5–14× faster** than Whisper | **0** |
| Whisper | 9.8% | baseline | 8–78 |

## Decision

Transcription runs **locally**, using the **Parakeet** model family as the default engine.

The engine sits behind an interface, so it can be replaced after a new benchmark.
How Parakeet is run from the chosen stack (for example through ONNX Runtime) is decided
after a spike (see [principle P3](../principles.md#p3--a-spike-before-every-risky-technology)).

## Consequences

- Audio and transcripts never leave the computer.
- The model is downloaded once, on explicit user action.
- Open question for the spike: accuracy on **Italian and English mixed** in the same
  conversation, and the runtime available on both macOS and Windows.
- Swapping the engine requires a new benchmark and a new ADR.

## Alternatives considered

- **Whisper**: slightly less accurate, much slower, and invents text on silence.
- **Cloud transcription APIs**: rejected, audio would leave the computer.
