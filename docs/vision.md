# Vision

## In one sentence

Trama is a **second brain that organizes itself**: you bring in meetings, notes,
documents and files, and Trama turns them into an up-to-date, cited knowledge base
of your projects — for you to read and for AI agents to work from.

## The problem

Work knowledge is scattered across meetings, notes, documents and files. Keeping it
organized by hand (folders, areas, tags) is work nobody does consistently, so
requirements, decisions and open actions get lost, and every new task starts by
reconstructing context from scratch.

## The idea

**The less the user does, the better.** The user only brings material in. Trama:

1. decides which project each piece of material belongs to;
2. extracts what matters: projects, people, decisions, actions, requirements, terms;
3. keeps a set of wiki pages up to date, one per project, person and topic,
   where every statement cites its source;
4. answers questions in a chat, with citations;
5. gives AI agents (via MCP) a ready-made **project brief** before they start working.

The user corrects instead of organizing — and less and less over time.

There are no areas or folders to maintain: a source belongs to zero, one or more
projects, and material without a project stays searchable and useful. Actions found
in sources are opened and closed by the curator, not ticked off by hand.

## Knowledge model

Knowledge is built in four layers. Each layer is derived from the one below and
always points back to it.

```
Sources  →  Index  →  Entities & relations  →  Wiki pages
```

| Layer | Contains | Purpose |
|-------|----------|---------|
| **1. Sources** | Transcripts, notes, documents, files — stored as they are, never modified | Ground truth: every claim above points here |
| **2. Index** | Text chunks with embeddings and full-text search | Find the exact passage |
| **3. Entities & relations** | Projects, people, decisions, actions, requirements, terms and their links, each with evidence | The structure of the knowledge — the "weave" |
| **4. Wiki pages** | One page per project, person and topic, written and maintained by an LLM, with citations | What the user reads and what agents receive |

Updates are **incremental**: a new source updates only the entities and pages it touches.
See [ADR 0004](adr/0004-layered-knowledge-model.md).

## The curator

An automatic curator runs the pipeline:

- **On every new source**: assign it to projects, extract entities, propose updates,
  each with a confidence level.
- **Policy**: confident changes are applied automatically; uncertain ones go to a
  **review queue** that takes seconds to clear. Every change is logged and can be undone.
- **Maintenance**: periodically merge duplicates, flag outdated or conflicting information,
  consolidate pages.
- **Model tiers**: small, cheap models for classification and extraction; a strong model
  for writing pages.
- **Measured**: a set of test cases checks that the curator decides correctly.

## AI agents

Through an MCP server, an agent can:

- get the **project brief**: goal, requirements, decisions (with date and reason),
  open actions, open questions, people, glossary — each with links to sources;
- get a **task brief**: only what is relevant to a given task;
- **search and read sources** when it needs the exact detail.

The brief is the starting point; sources are for verification and detail.

## Local and remote

| Runs locally | Runs through configurable providers |
|--------------|-------------------------------------|
| Recording and transcription | LLMs and embeddings |
| All data and indexes | |

Providers are generic (OpenAI-compatible endpoints, more than one at a time), with the
model catalogue read from the provider and a model chosen per task.
See [ADR 0002](adr/0002-local-transcription-parakeet.md) and [ADR 0003](adr/0003-hybrid-ai-providers.md).

The transcript is the source; meeting audio is optional. By default it is deleted as
soon as the transcription succeeds, and a setting can keep it for 7 days, 30 days
or always.

## Platforms and languages

- **macOS first**; architecture and stack chosen so that Windows can follow.
  See [ADR 0005](adr/0005-macos-first-windows-ready.md).
- Transcription in **Italian and English**, including both in the same conversation.
- The user interface supports multiple languages from day one.

## MVP

The MVP (macOS) ships in three phases. Each phase is usable on its own, so real
meetings start piling up while the rest is still being built.

| Phase | What the user can do | Why in this order |
|-------|----------------------|-------------------|
| **1. Record and summarize** | Record meetings on two tracks (the user's microphone and the other participants) with local transcription, notes pinned to the minute while recording, and a global shortcut; get a summary and a title for each meeting; write Markdown notes; browse and search sources; configure providers with a model per task; choose how long audio is kept | Start collecting real meetings as early as possible |
| **2. The curator** | Projects and people found automatically; decisions, actions, requirements and open questions; wiki pages for projects and people, with citations; review queue; undoable change history | The curator also processes the sources recorded in phase 1 |
| **3. Use the knowledge** | Chat and search with citations; MCP server with project brief, task brief and access to sources; import of documents and files (PDF, DOCX, MD, TXT) | Arrives when the pages are already full |

Screens, navigation and flows are described in the
[information architecture](design/information-architecture.md).

**Out of scope for the MVP** (backlog):

- Dictation
- Windows
- Topic pages (subjects that cross projects)
- A personal to-do view
- Telling apart speakers among the other participants (diarization)
- Graphs and advanced visualizations
- Detecting ongoing calls
- Token and cost tracking

## How we work

See [Principles](principles.md). Decisions are recorded as
[Architecture Decision Records](adr/0001-record-architecture-decisions.md).
