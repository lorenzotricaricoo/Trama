# Principles

These principles apply to every part of Trama. A change that breaks one of them
needs an [ADR](adr/0001-record-architecture-decisions.md) explaining why.

## Process

### P1 — A small MVP, written down before the code

The MVP scope is defined in the [Vision](vision.md) before development starts.
Anything else goes to the backlog as an issue. Scope changes are explicit decisions,
not side effects of implementation.

### P2 — Decisions belong to the maintainer

Agents and tools propose; the maintainer decides. No unattended runs that make product
or architecture choices. Every significant decision is recorded as an ADR.

### P3 — A spike before every risky technology

When a technology is uncertain (a platform API, a model, a library), a small spike
answers the question first, with a **measurable outcome** and a written conclusion.
Production code starts only after the spike.

### P4 — Design before code

Information architecture, visual direction and prototypes are defined and validated
before the corresponding UI is implemented. The UI is built on an established design
system rather than invented from scratch.

## Architecture

### A1 — The core is independent of the UI

Capture, transcription, the curator and the index live in a core library that runs
and is tested **without the user interface** (headless). The UI is a client of the core.

### A2 — One owner of the data

A single process owns the database. The MCP server, the CLI and any other client go
through that process's API and never open the database directly.

### A3 — The data model is designed up front

The data model follows the [knowledge model](vision.md#knowledge-model) and is designed
before features are built on it. Schema changes are versioned migrations, covered by tests.

### A4 — LLM calls are robust from day one

- Limits (context, output) come from the provider's model catalogue, never hard-coded.
- Truncated or incomplete responses are detected and handled.
- No silent failures: an error is either handled or surfaced, never ignored.

### A5 — Foundations from day one

- **Localization**: no hard-coded user-facing strings, dates or number formats.
- **Stable code signing**: the app keeps its identity across builds, so system
  permissions are granted once.
- **Automated tests**: every component ships with tests; manual checks complement them,
  never replace them.
