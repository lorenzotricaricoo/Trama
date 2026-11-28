# AI providers

How Trama talks to language models. It follows
[ADR 0003](../adr/0003-hybrid-ai-providers.md) and principle
[A4](../principles.md#a4--llm-calls-are-robust-from-day-one); the stored configuration is
described in the [data model](data-model.md#provider). The code is in
`crates/core/src/providers/`.

## Providers and models

- A **provider** is any OpenAI-compatible endpoint: a cloud service, a company gateway or
  a local server. Several can be configured at once. Its URL must use `https://`; plain
  `http://` is accepted only for local servers (loopback, private or link-local
  addresses, `localhost`, `.local` names).
- Its **API key** lives in the macOS Keychain (service `trama-provider-<provider id>`,
  account `api-key`). The database keeps only the service name. Keys never appear in
  logs or error details: the client removes the key from any text it reports, and its
  debug output hides it.
- The **model catalogue** is read with `GET /models`. Limits (context window, longest
  reply) come from the catalogue when the provider exposes them; otherwise the user sets
  them by hand. A model whose context window is unknown can't be used for a task.
- Each **task** uses the model the user chose for it. Phase 1 has one task, `summary`; the
  curator's tasks are added in phase 2. Spike
  [#12](https://github.com/lorenzotricaricoo/Trama/issues/12) found that cheap models are
  enough for the curator; the recommended default is `claude-haiku-5-5`, where the
  provider offers it.

## Robust calls

Provider calls run inside [background jobs](data-model.md#job), so the client is
blocking and every failure ends as an error code the job stores and the interface
translates (`locales/<locale>/providers.ftl`).

| Situation | What the client does | Code | Job retries later |
|-----------|----------------------|------|-------------------|
| No reply within the timeout (120 s per exchange, 10 s to connect) | Retries with backoff | `timeout` | yes |
| Network, DNS or TLS failure | Retries with backoff | `network` | yes |
| HTTP 429 | Waits for `Retry-After` (seconds or HTTP date) or `retry-after-ms`, else backs off; a wait over 60 s, or a `Retry-After` it can't read, ends the call at once | `rate_limited` | yes |
| HTTP 5xx | Retries with backoff | `server_error` | yes |
| HTTP 401, 403 | Stops | `auth_failed` | no |
| HTTP 404 | Stops | `not_found` | no |
| HTTP 400, 413, 422 | Stops; a structured call first tries a simpler response format | `bad_request` | no |
| Reply cut at the output limit (`finish_reason` `length`) | Stops: the same request would be cut again | `truncated` | no |
| Content filter or refusal | Stops | `content_filtered` | no |
| Empty reply | Asks again | `empty_reply` | yes |
| Structured reply that isn't JSON, or doesn't match the schema | Asks again | `invalid_json`, `schema_mismatch` | yes |
| Structured reply that matches the schema but is empty or placeholder text | Asks again | `degenerate_output` | yes |
| Prompt too long for the context window | Refuses before sending | `context_exceeded` | no |
| Unknown context window, no model for the task, missing key | Refuses before sending | `limits_unknown`, `no_model_for_task`, `missing_key` | no |

- **Backoff**: 1 s before the first retry, doubling up to 30 s, with up to 25% jitter;
  4 attempts per request. A reply with bad content is asked for up to 3 times.
- **Output budget**: the reply limit sent (`max_tokens`) is what the task asks for,
  capped by the model's output limit and by the room the prompt leaves in the context
  window. The prompt size is estimated conservatively (about three characters per token).
- **Structured output**: the client asks for `response_format` `json_schema`; if the
  provider refuses it, it falls back to `json_object`, then to a plain request, with the
  schema in the prompt. Code fences around the JSON are tolerated.
- **Degenerate output** ([spike #12](https://github.com/lorenzotricaricoo/Trama/issues/12)):
  a reply is refused when a required string is empty, a placeholder (`...`, `N/A`,
  `<title>`, `string`, …) or has no letter or digit, or when every value in it is empty
  or zero. Values the schema lists in an `enum` are always accepted, and empty arrays
  ("nothing found") are a valid answer.

## Meeting summaries

The `summary` task gives each meeting a title, a short summary and key points with
timestamps ([data model](data-model.md#summary)). The code is in
`crates/core/src/summary/`.

- **When**: a `summarize` [job](data-model.md#job) runs after each transcription (the
  transcription job calls `summary::enqueue_after_transcription`) and whenever the user
  asks to generate it again. It summarizes the meeting's current transcription version;
  the summary points to that version. When it is stored, a meeting in `processing`
  becomes `ready`.
- **Language**: the meeting's language (`source.language`, else the `meeting.language`
  setting). The prompt names it and asks for it whatever language the transcript is in;
  it is never detected.
- **Input**: one line per segment, `[HH:MM:SS] Me|Others: text`. The model copies the time
  of each key point from those lines; the core snaps it to the start of the nearest
  segment, and keeps the point without a time if it can't read it.
- **Long transcripts** never get truncated. Each request is sized from the model's
  limits: the reply gets up to 2048 tokens (less if the model's output limit or a quarter
  of its context is smaller), the prompts, the schema and a margin are set aside, and
  80% of the rest is room for text (20% stays free as a margin on the estimates). Text is
  counted more pessimistically than the client does: three characters per token for most
  languages but one for Chinese, Japanese and Korean, and two for the dense line
  prefixes and note headers. A transcript that doesn't fit is split into parts between
  segments (inside a segment only if it is longer than a part on its own); each part gets
  notes (summary and timed key points), and the notes are merged into the summary. If
  the notes don't fit one request either, neighbouring notes are combined first, in as
  many rounds as needed. If the provider still refuses a request as too long (HTTP 413,
  or a 400/422 whose message says so), the whole summary starts again once with half the
  room. A model whose context leaves less than 1024 tokens of room, or notes that can't
  be combined further, fail with `context_exceeded`; a provider that refuses the smaller
  parts too fails with `bad_request`.
- **Versions**: prompts, schemas and the input format are versioned together
  (`PROMPT_VERSION`, now `summary-v1`); any change to them needs a new version. Each
  summary stores it with the model id.
- **Titles**: a summary titles its meeting unless the user wrote the title
  (`title_origin = user`); generating again then adds a summary and leaves the title
  alone. Clearing the title gives it back to the summaries.
- **Errors**: provider failures keep their codes and retryability (table above), so the
  job retries what may succeed later and fails at once on what needs the user. The
  summary job adds `unsupported_source`, `no_transcript` and `empty_transcript`
  (`locales/<locale>/summary.ftl`). After the last attempt (or a permanent error) a
  meeting in `processing` becomes `ready` anyway: the transcript is complete, the summary
  is optional, and the failed job stays visible for the user to retry. A job cancelled
  while waiting for the model stores nothing. A retry starts the summary from the first
  part again: notes on the parts are not kept between attempts.
- **Desktop commands**: `source_summary`, `regenerate_summary` (returns the job) and
  `set_source_title`; progress and failures arrive through the `job` event.

## Trying a real provider

CI never calls a real provider. To check one locally, store its key in the Keychain (it
is asked at the prompt) and run the ignored test:

```sh
security add-generic-password -U -a api-key -s trama-provider-manual-test -w
TRAMA_TEST_PROVIDER_URL=https://<openai-compatible>/v1 TRAMA_TEST_MODEL=claude-haiku-5-5 \
  cargo test -p trama-core --test providers real_provider -- --ignored --nocapture
```

It lists the models, then asks for a short reply and a structured summary.
