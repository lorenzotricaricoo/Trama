# 0003. Hybrid AI: local transcription, configurable providers for LLMs and embeddings

- **Status:** Accepted
- **Date:** 2026-11-27

## Context

Besides transcription ([ADR 0002](0002-local-transcription-parakeet.md)), Trama needs
LLMs (curator, wiki pages, chat) and embeddings (index). Running capable LLMs locally
is expensive and slow on a laptop, while many users — including in companies — already
have access to OpenAI-compatible endpoints offering many models, free and paid,
including embedding models.

## Decision

- **Local**: recording, transcription, and **all data and indexes**.
- **Through providers**: LLM and embedding calls.
- Providers are **generic**: any OpenAI-compatible endpoint, more than one configured
  at a time. API keys are stored in the operating system's credential store.
- The **model catalogue** is read from the provider (context size, output limits,
  pricing when available) and drives the limits used by Trama
  (see [principle A4](../principles.md#a4--llm-calls-are-robust-from-day-one)).
- The user chooses **a model per task** (for example: extraction, page writing, chat,
  embeddings), with cheap models as the default for high-volume tasks.
- Every stored vector records the **embedding model** that produced it. Changing the
  embedding model triggers a re-index.

## Consequences

- No dependency on a single vendor.
- Text sent to providers is limited to what each task needs; audio is never sent.
- Provider availability affects curator and chat, but never recording or transcription.
- Re-indexing must be supported from the start.

## Alternatives considered

- **Everything local** (local LLMs and embeddings): rejected for the MVP because of
  quality and performance on a laptop; local providers can be added later as one more
  OpenAI-compatible endpoint.
- **A single hard-wired provider**: rejected, too restrictive.
