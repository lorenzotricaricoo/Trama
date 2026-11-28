//! The OpenAI-compatible HTTP client.
//!
//! Blocking on purpose: provider calls run inside background jobs, on the runner's worker
//! thread. Every call has a timeout; transient failures (timeouts, network errors, HTTP 429
//! and 5xx) are retried with exponential backoff, honouring `Retry-After`. Replies are
//! checked before they are returned: truncated, filtered or empty replies become coded
//! errors, and structured replies go through [`super::structured`] (principle A4).

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};
use ureq::Agent;
use ureq::http::Response;
use ureq::tls::{RootCerts, TlsConfig};

use super::catalog::parse_catalog;
use super::error::{ErrorKind, ProviderError};
use super::secrets::Secret;
use super::structured::parse_structured;
use crate::storage::{CatalogEntry, ModelLimits};

/// Largest reply body read, to protect memory from a misbehaving endpoint.
const MAX_BODY_BYTES: u64 = 32 * 1024 * 1024;

/// Characters of a failed reply kept in an error's detail.
const DETAIL_CHARS: usize = 300;

/// Tokens added per message for the chat format's framing, in the prompt estimate.
const MESSAGE_OVERHEAD_TOKENS: u32 = 8;

/// How often and how long to retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Attempts per HTTP request, including the first.
    pub max_attempts: u32,
    /// Delay before the first retry; it doubles with each attempt, plus up to 25% jitter.
    pub base_delay: Duration,
    /// Longest delay between two attempts.
    pub max_delay: Duration,
    /// Longest `Retry-After` the client waits for; a longer one fails the call with
    /// `rate_limited`, so the job is retried later instead of blocking the runner.
    pub max_retry_after: Duration,
    /// Requests per call when the reply is empty, not JSON or degenerate.
    pub content_attempts: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            max_retry_after: Duration::from_secs(60),
            content_attempts: 3,
        }
    }
}

impl RetryPolicy {
    /// Delay after `attempt` failed attempts (1-based), with jitter.
    fn backoff(&self, attempt: u32) -> Duration {
        let doublings = attempt.saturating_sub(1).min(16);
        let delay = self
            .base_delay
            .saturating_mul(1 << doublings)
            .min(self.max_delay);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        delay + delay.mul_f64(f64::from(nanos % 250) / 1000.0)
    }
}

/// Timeouts and retries of a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    /// Longest wait for one HTTP exchange, from connecting to reading the whole reply.
    pub timeout: Duration,
    /// Longest wait to connect (DNS, TCP, TLS).
    pub connect_timeout: Duration,
    pub retry: RetryPolicy,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(10),
            retry: RetryPolicy::default(),
        }
    }
}

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// One message of a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
        }
    }
}

/// A chat completion request.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    /// Longest reply wanted; `None` uses the model's limit. Clamped to the model's limit
    /// and to the room left in the context window.
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

/// The JSON schema a structured reply must follow.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonSchema {
    /// Name sent to the provider, for example `"summary"`.
    pub name: String,
    pub schema: Value,
    /// Ask the provider to enforce the schema strictly (OpenAI `strict`).
    pub strict: bool,
}

/// A checked reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatReply {
    /// The reply text, never empty.
    pub content: String,
    /// The model that answered, as the provider reports it.
    pub model: Option<String>,
    pub finish_reason: Option<String>,
}

/// A checked structured reply.
#[derive(Debug, Clone, PartialEq)]
pub struct StructuredReply {
    /// The reply, valid against the schema and not degenerate.
    pub value: Value,
    pub reply: ChatReply,
}

/// A conservative estimate of the tokens in `text` (about three characters per token,
/// where real tokenizers average about four for English and Italian prose).
pub fn estimate_tokens(text: &str) -> u32 {
    u32::try_from(text.chars().count().div_ceil(3)).unwrap_or(u32::MAX)
}

/// Estimated prompt size of `messages`.
pub fn estimate_prompt_tokens(messages: &[Message]) -> u32 {
    messages.iter().fold(0u32, |sum, message| {
        sum.saturating_add(estimate_tokens(&message.content))
            .saturating_add(MESSAGE_OVERHEAD_TOKENS)
    })
}

/// How a structured reply is requested. Providers that refuse JSON schemas get a JSON
/// object request, and then a plain request, with the schema in the prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    JsonSchema,
    JsonObject,
    Prompted,
}

/// Pauses between retries; replaced in tests.
type Sleeper = Arc<dyn Fn(Duration) + Send + Sync>;

/// Tokens a provider reported for one reply (the `usage` object of a chat completion).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Called with the usage of every chat reply the provider sends, retried and refused
/// replies included, since they are billed too.
pub type UsageHook = Arc<dyn Fn(TokenUsage) + Send + Sync>;

/// A client for one provider.
#[derive(Clone)]
pub struct Client {
    base_url: String,
    key: Option<Secret>,
    agent: Agent,
    config: ClientConfig,
    sleep: Sleeper,
    usage_hook: Option<UsageHook>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("base_url", &self.base_url)
            .field("has_key", &self.key.is_some())
            .finish_non_exhaustive()
    }
}

impl Client {
    /// A client for the API at `base_url` (for example `https://api.example.com/v1`).
    pub fn new(base_url: &str, key: Option<Secret>, config: ClientConfig) -> Self {
        let agent = Agent::config_builder()
            .http_status_as_error(false)
            .timeout_per_call(Some(config.timeout))
            .timeout_connect(Some(config.connect_timeout))
            // Trust the certificates the user trusts in macOS, company CAs included.
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .user_agent(concat!("Trama/", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            key,
            agent,
            config,
            sleep: Arc::new(std::thread::sleep),
            usage_hook: None,
        }
    }

    /// Replaces how the client waits between retries; tests record the delays instead.
    pub fn with_sleeper(mut self, sleep: impl Fn(Duration) + Send + Sync + 'static) -> Self {
        self.sleep = Arc::new(sleep);
        self
    }

    /// Reports the tokens of every chat reply to `hook`, to track usage and cost.
    pub fn with_usage_hook(mut self, hook: UsageHook) -> Self {
        self.usage_hook = Some(hook);
        self
    }

    /// The provider's model catalogue.
    pub fn list_models(&self) -> Result<Vec<CatalogEntry>, ProviderError> {
        let body = self.request("models", None)?;
        parse_catalog(&body).map_err(|err| self.scrub(err))
    }

    /// A chat completion, checked: never truncated, filtered or empty.
    pub fn chat(
        &self,
        request: &ChatRequest,
        limits: ModelLimits,
    ) -> Result<ChatReply, ProviderError> {
        self.with_content_retries(|| self.complete(request, limits, Format::Text, None))
    }

    /// A chat completion whose reply must be JSON following `schema`. Empty, invalid and
    /// degenerate replies are asked again, up to [`RetryPolicy::content_attempts`].
    pub fn chat_structured(
        &self,
        request: &ChatRequest,
        schema: &JsonSchema,
        limits: ModelLimits,
    ) -> Result<StructuredReply, ProviderError> {
        let mut format = Format::JsonSchema;
        self.with_content_retries(|| {
            loop {
                match self.complete(request, limits, format, Some(schema)) {
                    Err(err) if err.kind == ErrorKind::BadRequest && format != Format::Prompted => {
                        // The provider may not support this response format: step down.
                        format = match format {
                            Format::JsonSchema => Format::JsonObject,
                            _ => Format::Prompted,
                        };
                    }
                    Err(err) => return Err(err),
                    Ok(reply) => {
                        let value = parse_structured(&reply.content, &schema.schema)
                            .map_err(|err| self.scrub(err))?;
                        return Ok(StructuredReply { value, reply });
                    }
                }
            }
        })
    }

    fn with_content_retries<T>(
        &self,
        mut call: impl FnMut() -> Result<T, ProviderError>,
    ) -> Result<T, ProviderError> {
        let attempts = self.config.retry.content_attempts.max(1);
        let mut attempt = 1;
        loop {
            match call() {
                Err(err) if err.is_bad_content() && attempt < attempts => attempt += 1,
                result => return result,
            }
        }
    }

    /// One checked completion (with transport retries).
    fn complete(
        &self,
        request: &ChatRequest,
        limits: ModelLimits,
        format: Format,
        schema: Option<&JsonSchema>,
    ) -> Result<ChatReply, ProviderError> {
        let mut messages = request.messages.clone();
        if let (Format::JsonObject | Format::Prompted, Some(schema)) = (format, schema) {
            messages.push(Message::system(format!(
                "Reply with one JSON object only, no other text, following this JSON schema:\n{}",
                schema.schema
            )));
        }
        let max_tokens = output_budget(request, &messages, limits)?;

        let mut body = json!({
            "model": request.model,
            "messages": messages,
        });
        if let Some(max_tokens) = max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }
        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(temperature);
        }
        match (format, schema) {
            (Format::JsonSchema, Some(schema)) => {
                body["response_format"] = json!({
                    "type": "json_schema",
                    "json_schema": {
                        "name": schema.name,
                        "strict": schema.strict,
                        "schema": schema.schema,
                    }
                });
            }
            (Format::JsonObject, Some(_)) => {
                body["response_format"] = json!({"type": "json_object"});
            }
            _ => {}
        }

        let text = self.request("chat/completions", Some(&body))?;
        if let (Some(hook), Some(usage)) = (&self.usage_hook, parse_usage(&text)) {
            hook(usage);
        }
        parse_chat_reply(&text).map_err(|err| self.scrub(err))
    }

    /// Sends a request with transport retries; returns the body of a 2xx reply.
    fn request(&self, path: &str, body: Option<&Value>) -> Result<String, ProviderError> {
        let policy = &self.config.retry;
        let mut attempt = 1;
        loop {
            let err = match self.request_once(path, body) {
                Ok(text) => return Ok(text),
                Err(err) => err,
            };
            if !err.is_transient() || attempt >= policy.max_attempts.max(1) {
                return Err(err);
            }
            let delay = match err.kind {
                ErrorKind::RateLimited {
                    retry_after: Some(wait),
                } => {
                    if wait > policy.max_retry_after {
                        return Err(err);
                    }
                    wait
                }
                _ => policy.backoff(attempt),
            };
            (self.sleep)(delay);
            attempt += 1;
        }
    }

    fn request_once(&self, path: &str, body: Option<&Value>) -> Result<String, ProviderError> {
        let url = format!("{}/{path}", self.base_url);
        let auth = self
            .key
            .as_ref()
            .map(|key| format!("Bearer {}", key.expose()));
        let result = match body {
            Some(body) => {
                let mut request = self
                    .agent
                    .post(&url)
                    .header("Accept", "application/json")
                    .content_type("application/json");
                if let Some(auth) = &auth {
                    request = request.header("Authorization", auth);
                }
                request.send(body.to_string())
            }
            None => {
                let mut request = self.agent.get(&url).header("Accept", "application/json");
                if let Some(auth) = &auth {
                    request = request.header("Authorization", auth);
                }
                request.call()
            }
        };
        let mut response = result.map_err(|err| self.transport_error(&err))?;
        let status = response.status().as_u16();
        let retry_after = retry_after(&response);
        let text = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY_BYTES)
            .read_to_string()
            .map_err(|err| self.transport_error(&err))?;
        if (200..300).contains(&status) {
            return Ok(text);
        }
        let detail = format!("HTTP {status}: {}", self.snippet(&text));
        let kind = match status {
            429 => ErrorKind::RateLimited { retry_after },
            401 | 403 => ErrorKind::AuthFailed,
            404 => ErrorKind::NotFound,
            408 => ErrorKind::Timeout,
            400 | 413 | 422 => ErrorKind::BadRequest,
            500..=599 => ErrorKind::Server { status },
            _ => ErrorKind::InvalidResponse,
        };
        Err(ProviderError::new(kind, detail))
    }

    fn transport_error(&self, err: &ureq::Error) -> ProviderError {
        let timed_out = match err {
            ureq::Error::Timeout(_) => true,
            ureq::Error::Io(io) => io.kind() == std::io::ErrorKind::TimedOut,
            _ => false,
        };
        let kind = if timed_out {
            ErrorKind::Timeout
        } else {
            ErrorKind::Network
        };
        ProviderError::new(kind, self.redact(&err.to_string()))
    }

    /// The start of a reply body, for an error's detail, without the key.
    /// Shortens an error's detail and removes the key from it: details built from a reply
    /// (a provider's error object, for example) may echo what was sent.
    fn scrub(&self, mut err: ProviderError) -> ProviderError {
        err.detail = self.snippet(&err.detail);
        err
    }

    fn snippet(&self, text: &str) -> String {
        // Redact first: a key straddling the cut would otherwise leak its start.
        let text = self.redact(text);
        let mut snippet: String = text.chars().take(DETAIL_CHARS).collect();
        if text.chars().nth(DETAIL_CHARS).is_some() {
            snippet.push('…');
        }
        snippet
    }

    /// Removes the key from text that may echo it (an error page, a URL).
    fn redact(&self, text: &str) -> String {
        match &self.key {
            Some(key) if !key.expose().is_empty() => text.replace(key.expose(), "<redacted>"),
            _ => text.to_owned(),
        }
    }
}

/// `max_tokens` to send: what the request wants, within the model's output limit and the
/// room the prompt leaves in the context window. Fails if the prompt doesn't fit.
fn output_budget(
    request: &ChatRequest,
    messages: &[Message],
    limits: ModelLimits,
) -> Result<Option<u32>, ProviderError> {
    let Some(context) = limits.context_tokens else {
        return Err(ProviderError::new(
            ErrorKind::LimitsUnknown,
            format!("context window of {} unknown", request.model),
        ));
    };
    let prompt = estimate_prompt_tokens(messages);
    let room = context.saturating_sub(prompt);
    let wanted = match (request.max_output_tokens, limits.max_output_tokens) {
        (Some(wanted), Some(limit)) => Some(wanted.min(limit)),
        (wanted, limit) => wanted.or(limit),
    };
    // The reply needs some room; asking for more than is left is the caller's mistake.
    let needed = request.max_output_tokens.map_or(1, |w| wanted.unwrap_or(w));
    if room < needed.max(1) {
        return Err(ProviderError::new(
            ErrorKind::ContextExceeded,
            format!(
                "prompt of about {prompt} tokens plus a reply of {needed} exceeds the context of {context}"
            ),
        ));
    }
    Ok(wanted.map(|w| w.min(room)))
}

/// How long the provider asked to wait: `retry-after-ms`, or `Retry-After` in seconds or
/// as an HTTP date. Huge values saturate instead of overflowing, so a long wait fails the
/// call at once (`max_retry_after`); a `Retry-After` that can't be read counts as a long
/// wait too, rather than as permission to retry at once.
fn retry_after<B>(response: &Response<B>) -> Option<Duration> {
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
    };
    if let Some(ms) = header("retry-after-ms").and_then(|v| v.parse::<f64>().ok())
        && ms.is_finite()
        && ms >= 0.0
    {
        return Some(seconds(ms / 1000.0));
    }
    let value = header("retry-after")?;
    if let Ok(secs) = value.parse::<f64>() {
        return Some(if secs.is_finite() && secs >= 0.0 {
            seconds(secs)
        } else {
            Duration::MAX
        });
    }
    match httpdate::parse_http_date(value) {
        Ok(at) => Some(
            at.duration_since(SystemTime::now())
                .unwrap_or(Duration::ZERO),
        ),
        Err(_) => Some(Duration::MAX),
    }
}

/// A non-negative number of seconds as a duration, saturating at [`Duration::MAX`].
fn seconds(secs: f64) -> Duration {
    Duration::try_from_secs_f64(secs).unwrap_or(Duration::MAX)
}

/// Reads a chat completion and checks it.
fn parse_chat_reply(text: &str) -> Result<ChatReply, ProviderError> {
    let value: Value = serde_json::from_str(text).map_err(|err| {
        ProviderError::new(
            ErrorKind::InvalidResponse,
            format!("chat reply is not JSON: {err}"),
        )
    })?;
    let Some(choice) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
    else {
        let detail = value.get("error").map_or_else(
            || "chat reply has no choices".to_owned(),
            |e| format!("provider error: {e}"),
        );
        return Err(ProviderError::new(ErrorKind::InvalidResponse, detail));
    };
    let message = choice.get("message");
    let finish_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let content = message
        .and_then(|m| m.get("content"))
        .map(content_text)
        .unwrap_or_default();
    let refusal = message
        .and_then(|m| m.get("refusal"))
        .and_then(Value::as_str)
        .filter(|r| !r.trim().is_empty());

    match finish_reason.as_deref() {
        Some("length" | "max_tokens") => {
            return Err(ProviderError::new(
                ErrorKind::Truncated,
                format!(
                    "reply stopped at the output limit after {} characters",
                    content.chars().count()
                ),
            ));
        }
        Some("content_filter") => {
            return Err(ProviderError::new(
                ErrorKind::ContentFiltered,
                "finish_reason content_filter",
            ));
        }
        _ => {}
    }
    if let Some(refusal) = refusal {
        let refusal: String = refusal.chars().take(DETAIL_CHARS).collect();
        return Err(ProviderError::new(
            ErrorKind::ContentFiltered,
            format!("refusal: {refusal}"),
        ));
    }
    if content.trim().is_empty() {
        return Err(ProviderError::new(
            ErrorKind::EmptyReply,
            format!("empty reply (finish_reason {finish_reason:?})"),
        ));
    }
    Ok(ChatReply {
        content,
        model: value
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned),
        finish_reason,
    })
}

/// The tokens a chat completion reports in its `usage` object, under the OpenAI names
/// (`prompt_tokens`, `completion_tokens`) or the Anthropic-style ones (`input_tokens`,
/// `output_tokens`). `None` when the reply has no usage.
fn parse_usage(text: &str) -> Option<TokenUsage> {
    let value: Value = serde_json::from_str(text).ok()?;
    let usage = value.get("usage")?;
    let count = |names: [&str; 2]| {
        names
            .iter()
            .find_map(|name| usage.get(*name).and_then(Value::as_u64))
    };
    let input = count(["prompt_tokens", "input_tokens"]);
    let output = count(["completion_tokens", "output_tokens"]);
    if input.is_none() && output.is_none() {
        return None;
    }
    Some(TokenUsage {
        input_tokens: input.unwrap_or(0),
        output_tokens: output.unwrap_or(0),
    })
}

/// Message content as text: a string, or the text parts of a list.
fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_is_read_under_both_naming_styles() {
        let openai = r#"{"usage":{"prompt_tokens":120,"completion_tokens":30,"total_tokens":150}}"#;
        assert_eq!(
            parse_usage(openai),
            Some(TokenUsage {
                input_tokens: 120,
                output_tokens: 30
            })
        );
        let anthropic = r#"{"usage":{"input_tokens":7,"output_tokens":2}}"#;
        assert_eq!(
            parse_usage(anthropic),
            Some(TokenUsage {
                input_tokens: 7,
                output_tokens: 2
            })
        );
        assert_eq!(parse_usage(r#"{"choices":[]}"#), None);
        assert_eq!(parse_usage(r#"{"usage":{}}"#), None);
        assert_eq!(parse_usage("not json"), None);
    }

    fn request(max_output_tokens: Option<u32>, prompt: &str) -> ChatRequest {
        ChatRequest {
            model: "m".into(),
            messages: vec![Message::user(prompt)],
            max_output_tokens,
            temperature: None,
        }
    }

    fn limits(context: Option<u32>, output: Option<u32>) -> ModelLimits {
        ModelLimits {
            context_tokens: context,
            max_output_tokens: output,
        }
    }

    #[test]
    fn output_budget_respects_the_catalogue() {
        let req = request(None, "hello");
        let msgs = &req.messages;
        assert_eq!(
            output_budget(&req, msgs, limits(Some(1000), Some(100))).unwrap(),
            Some(100)
        );
        assert_eq!(
            output_budget(&req, msgs, limits(Some(1000), None)).unwrap(),
            None
        );
        // Room left in the context caps the reply.
        assert_eq!(
            output_budget(&req, msgs, limits(Some(50), Some(100))).unwrap(),
            Some(40)
        );

        let req = request(Some(500), "hello");
        let msgs = &req.messages;
        assert_eq!(
            output_budget(&req, msgs, limits(Some(1000), Some(100))).unwrap(),
            Some(100)
        );
        assert_eq!(
            output_budget(&req, msgs, limits(Some(1000), None)).unwrap(),
            Some(500)
        );
    }

    #[test]
    fn output_budget_refuses_unknown_or_exceeded_contexts() {
        let req = request(None, "hello");
        let err = output_budget(&req, &req.messages, limits(None, Some(100))).unwrap_err();
        assert_eq!(err.kind, ErrorKind::LimitsUnknown);

        let long = "x".repeat(3000);
        let req = request(None, &long);
        let err = output_budget(&req, &req.messages, limits(Some(1000), None)).unwrap_err();
        assert_eq!(err.kind, ErrorKind::ContextExceeded);

        let req = request(Some(900), "hello");
        let err = output_budget(&req, &req.messages, limits(Some(500), None)).unwrap_err();
        assert_eq!(err.kind, ErrorKind::ContextExceeded);
    }

    #[test]
    fn token_estimate_is_conservative() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 2);
        assert_eq!(estimate_tokens("perché"), 2);
    }

    #[test]
    fn backoff_doubles_with_jitter_and_a_cap() {
        let policy = RetryPolicy {
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(350),
            ..RetryPolicy::default()
        };
        for (attempt, base) in [(1, 100), (2, 200), (3, 350), (10, 350)] {
            let delay = policy.backoff(attempt).as_millis();
            assert!((base..=base * 5 / 4).contains(&delay), "{attempt}: {delay}");
        }
    }

    #[test]
    fn chat_replies_are_checked() {
        let ok =
            r#"{"model":"m-1","choices":[{"message":{"content":"Hi"},"finish_reason":"stop"}]}"#;
        let reply = parse_chat_reply(ok).unwrap();
        assert_eq!(reply.content, "Hi");
        assert_eq!(reply.model.as_deref(), Some("m-1"));

        let parts = r#"{"choices":[{"message":{"content":[{"type":"text","text":"A"},{"type":"text","text":"B"}]}}]}"#;
        assert_eq!(parse_chat_reply(parts).unwrap().content, "AB");

        let cases = [
            (
                r#"{"choices":[{"message":{"content":"par"},"finish_reason":"length"}]}"#,
                ErrorKind::Truncated,
            ),
            (
                r#"{"choices":[{"message":{"content":"x"},"finish_reason":"max_tokens"}]}"#,
                ErrorKind::Truncated,
            ),
            (
                r#"{"choices":[{"message":{"content":""},"finish_reason":"content_filter"}]}"#,
                ErrorKind::ContentFiltered,
            ),
            (
                r#"{"choices":[{"message":{"content":null,"refusal":"No."},"finish_reason":"stop"}]}"#,
                ErrorKind::ContentFiltered,
            ),
            (
                r#"{"choices":[{"message":{"content":"  "},"finish_reason":"stop"}]}"#,
                ErrorKind::EmptyReply,
            ),
            (r#"{"choices":[]}"#, ErrorKind::InvalidResponse),
            (
                r#"{"error":{"message":"overloaded"}}"#,
                ErrorKind::InvalidResponse,
            ),
            ("<html>", ErrorKind::InvalidResponse),
        ];
        for (body, kind) in cases {
            assert_eq!(parse_chat_reply(body).unwrap_err().kind, kind, "{body}");
        }
    }
}
