# 0004. Layered knowledge model and automatic curator

- **Status:** Accepted
- **Date:** 2026-11-27

## Context

Trama must turn heterogeneous material (meetings, notes, documents, files) into
knowledge the user and AI agents can use, **without the user organizing it by hand**.

Retrieval alone (chunks + embeddings) answers questions, but every answer is rebuilt
from scratch, and there is no durable picture of a project. An LLM-maintained wiki
gives a durable picture, but without structure it does not scale and cannot be verified.

## Decision

Knowledge is organized in **four layers**, each derived from the one below and always
pointing back to it:

1. **Sources** — the original material, immutable.
2. **Index** — text chunks with embeddings and full-text search, for retrieval with citations.
3. **Entities & relations** — projects, people, decisions, actions, requirements, terms,
   and their links, each with evidence in the sources.
4. **Wiki pages** — one per project, person and topic, written by an LLM from layers 3 and 2,
   with a citation for every statement.

An **automatic curator** maintains layers 3 and 4:

- runs **incrementally**: a new source updates only the entities and pages it touches;
- attaches a **confidence** to each proposed change: confident changes are applied,
  uncertain ones go to a **review queue**;
- **logs every change** and makes it undoable;
- runs **periodic maintenance**: merging duplicates, flagging outdated or conflicting
  information, consolidating pages;
- uses **cheap models** for classification and extraction and a **strong model** for
  writing pages;
- is **evaluated** against a set of test cases.

Chat and AI agents start from layer 4 and go down to layers 2 and 1 when they need detail.

## Consequences

- Projects, areas and links are created by Trama, not by the user.
- Every statement can be verified against its source.
- The data model must represent all four layers and the change history from the start
  (see [principle A3](../principles.md#a3--the-data-model-is-designed-up-front)).
- The curator's quality is a product risk: it needs its own spike and test set
  before the MVP builds on it.

## Alternatives considered

- **Retrieval only (RAG)**: no durable project picture; agents must rebuild context every time.
- **Unstructured LLM wiki**: hard to scale, hard to verify.
- **Manual organization** (folders, areas, tags): rejected by the product goal.
