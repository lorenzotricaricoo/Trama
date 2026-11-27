# Spike #12 — Curator quality

**Question.** Can cheap provider models assign sources to the right projects and
extract entities (people, decisions, actions, requirements) reliably enough to run the
curator (ADR 0004) with little human review?

## Answer

**Yes for project assignment and for people and decisions; actions and requirements
need a conservative policy.**

- **Assignment:** three cheap models (claude-haiku-5-5, gpt-6-luna,
  deepseek-v4-flash-sovereign) assigned **all 18 sources exactly right in all 5
  repeats** (100%, fully stable), including sources with two projects, with no project,
  and with projects named only indirectly. The strong models were not better.
- **Entities:** core F1 0.81–0.88 for every model. People (F1 ≈ 0.98) and decisions
  (0.87–0.99) are reliable. **Actions (0.67–0.75) and requirements (0.68–0.84) are
  over-extracted**: recall is high (85–99%) but precision is 54–65% for most models,
  because models turn plans, wishes and checklist lines into actions or requirements.
- **Confidence:** only claude-haiku-5-5 gives a confidence that separates errors from
  correct items (at threshold 0.8: 86% of entity errors caught, 11% of correct items
  sent to review). The other cheap models are over-confident, so their confidence is
  not usable as a review signal.
- **Cost:** about **$0.001–0.003 per source** for cheap models, $0.02–0.04 for strong
  ones. Real 45-minute meetings are about 5–10× longer.

**Recommended default for the curator: claude-haiku-5-5** — exact and stable on
assignment, F1 0.83 on entities, the only cheap model with useful confidence, and
$0.0016 per source. Total spent on the spike: **$6.05**.

## Results

Run on an OpenAI-compatible gateway, 6 models × 18 sources × 2 tasks × 5 repeats =
1,080 calls, 0 failed calls, structured output (`json_schema`) for every call. Prices
are the gateway's list prices.

| Model | Assignment exact | Stability | Core entity F1 (strict) | Cost / source | Latency p50 / p95 |
|---|---|---|---|---|---|
| claude-haiku-5-5 | **100%** | 100% | 0.83 (0.82–0.83) | $0.0016 | 8.8 s / 11.4 s |
| gpt-6-luna | **100%** | 100% | 0.81 (0.80–0.84) | $0.0011 | 14.1 s / 20.5 s |
| deepseek-v4-flash-sovereign | **100%** | 100% | 0.83 (0.82–0.84) | free | 48.7 s / 127.8 s |
| gemini-3.5-flash-lite | 93% (89–100) | 89% | **0.88** (0.87–0.89) | $0.0028 | 3.1 s / 4.2 s |
| claude-sonnet-5-5 | **100%** | 100% | 0.81 (0.80–0.82) | $0.0234 | 7.5 s / 11.5 s |
| claude-opus-5-5 | 82% (78–83)\* | 83% | 0.71 (0.68–0.77)\* | $0.0384 | 9.0 s / 13.8 s |

\* **Opus results are not a quality measure.** 32 of its 180 replies (18%) were
degenerate structured output through the gateway — valid JSON matching the schema, but
with empty strings, confidence 0.0 or garbled `reason` fields. Sonnet had 2 such
replies, the other models none. See *Findings for the implementation*.

**Entities, strict F1 per type** (precision / recall in brackets, mean of 5 repeats):

| Model | People | Decisions | Actions | Requirements |
|---|---|---|---|---|
| claude-haiku-5-5 | 0.98 (99/97) | 0.89 (80/100) | 0.75 (65/90) | 0.68 (57/85) |
| gpt-6-luna | 0.98 (98/98) | 0.98 (96/99) | 0.67 (54/88) | 0.72 (57/99) |
| deepseek-v4-flash-sovereign | 0.98 (98/98) | 0.87 (78/100) | 0.73 (64/84) | 0.73 (60/94) |
| gemini-3.5-flash-lite | 0.96 (95/98) | 0.99 (100/98) | 0.74 (84/66) | 0.84 (81/87) |
| claude-sonnet-5-5 | 0.98 (98/98) | 0.89 (82/98) | 0.71 (58/92) | 0.69 (54/96) |

Field accuracy on matched items is high for all models: action status 98–100%, action
owner 89–97%, decision date 100%, changed decisions detected (`supersedes`) 92–94%.

**Confidence as a review signal** (entities; errors caught / correct items sent to
review / total review load):

| Model | t = 0.7 | t = 0.8 | t = 0.9 |
|---|---|---|---|
| claude-haiku-5-5 | 65% / 4% / 20% | **86% / 11% / 30%** | 96% / 56% / 66% |
| claude-sonnet-5-5 | 46% / 3% / 16% | 61% / 9% / 24% | 94% / 22% / 44% |
| gpt-6-luna | 0% / 0% / 0% | 4% / 0% / 1% | 21% / 4% / 9% |
| deepseek-v4-flash-sovereign | 0% / 0% / 0% | 4% / 0% / 1% | 38% / 5% / 13% |
| gemini-3.5-flash-lite | 0% / 0% / 0% | 0% / 0% / 0% | 11% / 0% / 1% |

For assignment, the three best cheap models made no errors, so the threshold only
matters for the ambiguous source S11 (truth: no confident assignment). Haiku and Sonnet
returned no project with confidence 0.40–0.60, which would go to review — the desired
behaviour. gpt-6-luna and deepseek returned no project at 0.86–0.96. Gemini twice
assigned projects, once all four. Gemini also assigned the source S14, where a project
is named only as an example, in 4 of 5 repeats.

## Findings for the implementation

1. **Structured output can be valid and still empty.** Schema-valid but degenerate
   replies (empty text, confidence 0) must be detected and retried, not stored
   ([principle A4](../../docs/principles.md#a4--llm-calls-are-robust-from-day-one)).
2. **Actions and requirements need a stricter policy** than people and decisions:
   tighter prompt rules (an action needs an owner and a commitment), a higher
   confidence threshold, or a second cheap pass that filters candidates.
3. **Confidence is model-specific.** The review threshold must be calibrated per model;
   for models with uninformative confidence, use other signals (agreement between two
   cheap models, evidence checks).
4. **Linking across sources** (an action closed later, a decision changed later) was
   detected inside each source (92–100% field accuracy) but not linked to the earlier
   items: that is a curator step still to measure.
5. **The free EU-hosted model** (deepseek-v4-flash-sovereign) matches Haiku on accuracy
   but is slow (p50 49 s per source) and over-confident: an option for batch processing
   where data residency matters.

## Test set and method

- **18 fictional sources** (12 two-track meetings, 4 notes, 2 documents), in Italian
  with English work jargon, across 4 fictional projects of a fictional agency. Ground
  truth per source: expected projects and 125 scored entities (42 people,
  16 decisions, 36 actions, 22 requirements, 9 open questions) plus 23 optional ones,
  each with a verbatim evidence quote.
- **Hard cases:** no project (S05, S14), a project named only as an example (S14), a
  genuinely ambiguous source (S11), two projects (S08, S16, S17), projects named only
  indirectly, people with the same first name, decisions changed later, actions closed
  in a later source, vague requirements, requirements written as tables or checklists,
  owners reported by a third party.
- **Prompts and schemas** in `prompts/` and `schemas/`: assignment with a confidence per
  project and for the whole decision; extraction with type-specific fields, evidence and
  confidence. OpenAI-compatible `json_schema` (strict), with automatic fallbacks.
- **Scoring:** exact and Jaccard assignment accuracy, stability across repeats;
  one-to-one deterministic entity matching (normalized text, keywords, evidence
  overlap, threshold 0.45); strict match also requires key fields (action owner and
  status). The optional LLM judge for paraphrases was not used, so entity scores are a
  lower bound.
- **Limits:** synthetic and short sources (1–4 minutes); entities scored per source
  without the knowledge base.

## How to run

Python 3 standard library only, from `spikes/curator-quality/`:

```sh
python3 -I evaluate.py validate                  # dataset integrity
python3 -I -m unittest discover -s tests         # 27 tests, no network
python3 -I evaluate.py run --mock --models mock-good,mock-noisy,mock-bad --repeats 3 --out out/mock-run

export CURATOR_BASE_URL='<OpenAI-compatible base URL>'           # never commit it
export CURATOR_KEYCHAIN_SERVICE='<Keychain service of the API key>'
python3 -I evaluate.py catalog
python3 -I evaluate.py run --models claude-haiku-5-5,gpt-6-luna,gemini-3.5-flash-lite,deepseek-v4-flash-sovereign,claude-sonnet-5-5,claude-opus-5-5 \
  --repeats 5 --max-cost-usd 19.8 --concurrency 4 --out out/real-1
python3 -I evaluate.py score out/real-1           # re-score without calling anything
```

The API key is read from the Keychain at runtime and never printed or stored. A budget
guard refuses any call that could exceed `--max-cost-usd`. Raw outputs go to `out/`
(gitignored).
