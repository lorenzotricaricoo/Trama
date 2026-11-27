# 0001. Record architecture decisions

- **Status:** Accepted
- **Date:** 2026-11-27

## Context

Trama's product and architecture decisions need to be traceable: what was decided,
why, and which alternatives were rejected. Without a record, decisions get re-discussed
or silently changed during implementation (see [principle P2](../principles.md#p2--decisions-belong-to-the-maintainer)).

## Decision

Significant decisions are recorded as **Architecture Decision Records (ADRs)** in
`docs/adr/`.

- One file per decision: `NNNN-short-title.md`, numbered in order.
- Each ADR has: **Status**, **Date**, **Context**, **Decision**, **Consequences**,
  and optionally **Alternatives considered**.
- Status values: `Proposed`, `Accepted`, `Superseded by NNNN`, `Deprecated`.
- An accepted ADR is not rewritten. A changed decision gets a new ADR that supersedes it.
- ADRs go through the normal workflow: issue → pull request → squash merge.

## Consequences

- Every significant decision has a short, permanent explanation next to the code.
- Changing a decision requires writing down why, which makes changes deliberate.
- Small overhead per decision; only decisions that affect architecture, scope or
  technology choices need an ADR.
