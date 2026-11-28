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

## Usage, cost and budget

- **Prices.** The user can set, per model, the price of a million prompt and reply tokens
  (in the currency chosen in Settings). Providers don't expose prices in `GET /models`, so
  they are never guessed.
- **Usage.** `TaskModelClient::recording_usage` records the tokens of every reply the
  provider sends (the `usage` object), with its cost, in the library
  ([model usage](data-model.md#model-usage)). Retried and refused replies are recorded
  too, since they are billed. Every job that calls a model wraps its client with it.
- **Budget.** A monthly budget (UTC calendar month) with a warning threshold, 80% by
  default. Settings shows the month's spend and warns from the threshold on. If the user
  turns on the limit, `task_client` refuses a task once the spend reaches the budget,
  with `budget_exceeded` (not retried: the user raises the budget or turns the limit
  off). A call already running is never cut off.

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
| The month's spend reached an enforced budget | Refuses before sending | `budget_exceeded` | no |

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

## Trying a real provider

CI never calls a real provider. To check one locally, store its key in the Keychain (it
is asked at the prompt) and run the ignored test:

```sh
security add-generic-password -U -a api-key -s trama-provider-manual-test -w
TRAMA_TEST_PROVIDER_URL=https://<openai-compatible>/v1 TRAMA_TEST_MODEL=claude-haiku-5-5 \
  cargo test -p trama-core --test providers real_provider -- --ignored --nocapture
```

It lists the models, then asks for a short reply and a structured summary.
