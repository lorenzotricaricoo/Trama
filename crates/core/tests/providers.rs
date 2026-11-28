//! Provider client tests against a local mock server: no real provider is called.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::http::{MockServer, Reply};
use serde_json::json;
use trama_core::providers::{
    ChatRequest, Client, ClientConfig, ErrorKind, JsonSchema, Message, RetryPolicy, Secret,
};
use trama_core::storage::ModelLimits;

const KEY: &str = "sk-test-0123456789";

/// A client that records its retry delays instead of sleeping, with timeouts generous
/// enough for a loaded CI runner.
fn fast_client(server: &MockServer) -> (Client, Arc<Mutex<Vec<Duration>>>) {
    client_with(server, config(Duration::from_secs(5)))
}

/// Like [`fast_client`], with a short timeout, for the tests that delay replies on purpose.
fn timing_out_client(server: &MockServer) -> (Client, Arc<Mutex<Vec<Duration>>>) {
    client_with(server, config(Duration::from_millis(300)))
}

fn config(timeout: Duration) -> ClientConfig {
    ClientConfig {
        timeout,
        connect_timeout: timeout,
        retry: RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(1),
            max_retry_after: Duration::from_secs(60),
            content_attempts: 3,
        },
    }
}

fn client_with(server: &MockServer, config: ClientConfig) -> (Client, Arc<Mutex<Vec<Duration>>>) {
    let sleeps = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&sleeps);
    let client = Client::new(&server.url(), Some(Secret::new(KEY)), config)
        .with_sleeper(move |d| recorded.lock().unwrap().push(d));
    (client, sleeps)
}

fn limits() -> ModelLimits {
    ModelLimits {
        context_tokens: Some(8000),
        max_output_tokens: Some(1000),
    }
}

fn request() -> ChatRequest {
    ChatRequest {
        model: "cheap-model".into(),
        messages: vec![Message::system("Be brief."), Message::user("Hello")],
        max_output_tokens: None,
        temperature: Some(0.0),
    }
}

fn chat_body(content: &str, finish_reason: &str) -> String {
    json!({
        "id": "x",
        "model": "cheap-model",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": content},
                     "finish_reason": finish_reason}]
    })
    .to_string()
}

fn ok(content: &str) -> Reply {
    Reply::json(200, chat_body(content, "stop"))
}

fn summary_schema() -> JsonSchema {
    JsonSchema {
        name: "summary".into(),
        strict: true,
        schema: json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["title", "body", "key_points"],
            "properties": {
                "title": {"type": "string"},
                "body": {"type": "string"},
                "key_points": {"type": "array", "items": {"type": "string"}}
            }
        }),
    }
}

const GOOD_SUMMARY: &str =
    r#"{"title":"Kick-off","body":"We agreed on May.","key_points":["Ship in May"]}"#;
const EMPTY_SUMMARY: &str = r#"{"title":"","body":"","key_points":[]}"#;

#[test]
fn lists_models_with_limits_and_sends_the_key() {
    let server = MockServer::start();
    server.push(Reply::json(
        200,
        r#"{"object":"list","data":[
            {"id":"cheap-model","context_length":200000,"max_output_tokens":8192},
            {"id":"plain-model","object":"model"}]}"#,
    ));
    let (client, _) = fast_client(&server);
    let models = client.list_models().unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].model_id, "cheap-model");
    assert_eq!(models[0].limits.context_tokens, Some(200_000));
    assert_eq!(models[1].limits, ModelLimits::default());

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/v1/models");
    assert_eq!(
        requests[0].header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
}

#[test]
fn chat_sends_the_request_and_clamps_the_reply_to_the_model_limit() {
    let server = MockServer::start();
    server.push(ok("Hi there"));
    let (client, sleeps) = fast_client(&server);
    let mut req = request();
    req.max_output_tokens = Some(50_000);
    let reply = client.chat(&req, limits()).unwrap();
    assert_eq!(reply.content, "Hi there");
    assert_eq!(reply.finish_reason.as_deref(), Some("stop"));

    let sent = server.requests()[0].json();
    assert_eq!(server.requests()[0].path, "/v1/chat/completions");
    assert_eq!(sent["model"], "cheap-model");
    assert_eq!(sent["max_tokens"], 1000);
    assert_eq!(sent["temperature"], 0.0);
    assert_eq!(sent["messages"][0]["role"], "system");
    assert_eq!(sent["messages"][1]["content"], "Hello");
    assert!(sent.get("response_format").is_none());
    assert!(sleeps.lock().unwrap().is_empty());
}

#[test]
fn a_timeout_is_retried_and_then_reported() {
    let server = MockServer::start();
    for _ in 0..3 {
        server.push(ok("late").delayed(Duration::from_secs(2)));
    }
    let (client, sleeps) = timing_out_client(&server);
    let err = client.chat(&request(), limits()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Timeout);
    assert_eq!(err.code(), "timeout");
    assert!(err.is_retryable());
    assert_eq!(server.requests().len(), 3);
    assert_eq!(sleeps.lock().unwrap().len(), 2);
}

#[test]
fn a_timeout_followed_by_a_reply_succeeds() {
    let server = MockServer::start();
    server.push(ok("late").delayed(Duration::from_secs(2)));
    server.push(ok("on time"));
    let (client, _) = timing_out_client(&server);
    assert_eq!(
        client.chat(&request(), limits()).unwrap().content,
        "on time"
    );
}

#[test]
fn server_errors_are_retried_with_growing_backoff() {
    let server = MockServer::start();
    server.push(Reply::json(503, r#"{"error":"overloaded"}"#));
    server.push(Reply::json(502, "bad gateway"));
    server.push(ok("finally"));
    let (client, sleeps) = fast_client(&server);
    assert_eq!(
        client.chat(&request(), limits()).unwrap().content,
        "finally"
    );
    let sleeps = sleeps.lock().unwrap();
    assert_eq!(sleeps.len(), 2);
    assert!(sleeps[0] >= Duration::from_millis(100) && sleeps[0] <= Duration::from_millis(125));
    assert!(sleeps[1] >= Duration::from_millis(200) && sleeps[1] <= Duration::from_millis(250));
}

#[test]
fn server_errors_on_every_attempt_are_reported() {
    let server = MockServer::start();
    for _ in 0..3 {
        server.push(Reply::json(500, "boom"));
    }
    let (client, _) = fast_client(&server);
    let err = client.chat(&request(), limits()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Server { status: 500 });
    assert_eq!(err.code(), "server_error");
    assert_eq!(server.requests().len(), 3);
}

#[test]
fn rate_limits_honour_retry_after() {
    let server = MockServer::start();
    server.push(Reply::json(429, r#"{"error":"slow down"}"#).header("Retry-After", "7"));
    server.push(Reply::json(429, "{}").header("retry-after-ms", "1500"));
    server.push(ok("ok"));
    let (client, sleeps) = fast_client(&server);
    assert_eq!(client.chat(&request(), limits()).unwrap().content, "ok");
    assert_eq!(
        *sleeps.lock().unwrap(),
        [Duration::from_secs(7), Duration::from_millis(1500)]
    );
}

#[test]
fn a_rate_limit_without_retry_after_uses_backoff() {
    let server = MockServer::start();
    server.push(Reply::json(429, "{}"));
    server.push(ok("ok"));
    let (client, sleeps) = fast_client(&server);
    client.chat(&request(), limits()).unwrap();
    let sleeps = sleeps.lock().unwrap();
    assert!(sleeps[0] >= Duration::from_millis(100) && sleeps[0] <= Duration::from_millis(125));
}

#[test]
fn a_long_retry_after_fails_at_once_for_the_job_to_retry_later() {
    let server = MockServer::start();
    server.push(Reply::json(429, "{}").header("Retry-After", "3600"));
    let (client, sleeps) = fast_client(&server);
    let err = client.chat(&request(), limits()).unwrap_err();
    assert_eq!(
        err.kind,
        ErrorKind::RateLimited {
            retry_after: Some(Duration::from_secs(3600))
        }
    );
    assert_eq!(err.code(), "rate_limited");
    assert!(err.is_retryable());
    assert_eq!(server.requests().len(), 1);
    assert!(sleeps.lock().unwrap().is_empty());
}

/// Sends one 429 with `name: value` and returns the error and the recorded delays.
fn rate_limited_once(name: &str, value: &str) -> (Result<(), ErrorKind>, usize, Vec<Duration>) {
    let server = MockServer::start();
    server.push(Reply::json(429, "{}").header(name, value));
    server.push(ok("ok"));
    let (client, sleeps) = fast_client(&server);
    let kind = client
        .chat(&request(), limits())
        .map(drop)
        .map_err(|err| err.kind);
    let sleeps = sleeps.lock().unwrap().clone();
    (kind, server.requests().len(), sleeps)
}

#[test]
fn huge_or_unreadable_waits_fail_at_once_without_panicking() {
    for (name, value) in [
        ("retry-after-ms", "1e30"),
        ("Retry-After", "86400"),
        ("Retry-After", "1e300"),
        ("Retry-After", "soon"),
        ("Retry-After", "-5"),
    ] {
        let (kind, requests, sleeps) = rate_limited_once(name, value);
        assert!(
            matches!(kind, Err(ErrorKind::RateLimited { retry_after: Some(wait) }) if wait >= Duration::from_secs(86_400)),
            "{name}: {value} gave {kind:?}"
        );
        assert_eq!(requests, 1, "{name}: {value} was retried");
        assert!(sleeps.is_empty());
    }
}

#[test]
fn retry_after_can_be_an_http_date() {
    let tomorrow = std::time::SystemTime::now() + Duration::from_secs(86_400);
    let (kind, requests, _) = rate_limited_once("Retry-After", &httpdate::fmt_http_date(tomorrow));
    assert!(
        matches!(kind, Err(ErrorKind::RateLimited { retry_after: Some(wait) }) if wait > Duration::from_secs(80_000)),
        "{kind:?}"
    );
    assert_eq!(requests, 1);

    let soon = std::time::SystemTime::now() + Duration::from_secs(5);
    let (kind, requests, sleeps) = rate_limited_once("Retry-After", &httpdate::fmt_http_date(soon));
    assert_eq!(kind, Ok(()), "retried and succeeded");
    assert_eq!(requests, 2);
    assert!(sleeps[0] <= Duration::from_secs(5));
}

#[test]
fn a_key_cut_by_the_detail_limit_never_leaks_a_prefix() {
    // The key starts a few characters before the 300-character cut.
    let body = format!("{}{KEY}", "x".repeat(295));
    let server = MockServer::start();
    server.push(Reply::json(401, body));
    let (client, _) = fast_client(&server);
    let err = client.chat(&request(), limits()).unwrap_err();
    assert!(!err.detail.contains(&KEY[..5]), "{}", err.detail);
}

#[test]
fn a_refused_key_is_not_retried_and_never_echoed() {
    let server = MockServer::start();
    server.push(Reply::json(
        401,
        format!(r#"{{"error":"invalid key {KEY}"}}"#),
    ));
    let (client, _) = fast_client(&server);
    let err = client.chat(&request(), limits()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::AuthFailed);
    assert!(!err.is_retryable());
    assert_eq!(server.requests().len(), 1);
    assert!(!err.detail.contains(KEY), "{}", err.detail);
    assert!(!format!("{err:?} {err}").contains(KEY));
    assert!(!format!("{client:?}").contains(KEY));
}

#[test]
fn an_error_object_in_a_successful_reply_is_redacted_and_shortened() {
    let server = MockServer::start();
    let long = "x".repeat(2000);
    let body = json!({"error": {"message": format!("bad key {KEY} {long}")}}).to_string();
    for _ in 0..3 {
        server.push(Reply::json(200, body.clone()));
    }
    let (client, _) = fast_client(&server);
    let err = client.chat(&request(), limits()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidResponse);
    assert!(!err.detail.contains(KEY), "{}", err.detail);
    assert!(err.detail.chars().count() <= 301, "{}", err.detail.len());
}

#[test]
fn http_errors_map_to_codes() {
    let cases = [
        (404, ErrorKind::NotFound, "not_found"),
        (400, ErrorKind::BadRequest, "bad_request"),
        (403, ErrorKind::AuthFailed, "auth_failed"),
        (418, ErrorKind::InvalidResponse, "invalid_response"),
    ];
    for (status, kind, code) in cases {
        let server = MockServer::start();
        server.push(Reply::json(status, "{}"));
        let (client, _) = fast_client(&server);
        let err = client.chat(&request(), limits()).unwrap_err();
        assert_eq!(err.kind, kind, "{status}");
        assert_eq!(err.code(), code);
        assert_eq!(server.requests().len(), 1, "{status} is not retried");
    }
}

#[test]
fn an_unreachable_provider_is_a_network_error() {
    // Bind and drop to find a port nobody listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let sleeps = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&sleeps);
    let client = Client::new(
        &format!("http://127.0.0.1:{port}/v1"),
        None,
        ClientConfig {
            retry: RetryPolicy {
                max_attempts: 2,
                base_delay: Duration::from_millis(1),
                ..RetryPolicy::default()
            },
            ..ClientConfig::default()
        },
    )
    .with_sleeper(move |d| recorded.lock().unwrap().push(d));
    let err = client.list_models().unwrap_err();
    assert_eq!(err.kind, ErrorKind::Network);
    assert!(err.is_retryable());
    assert_eq!(sleeps.lock().unwrap().len(), 1);
}

#[test]
fn a_truncated_reply_is_an_error_and_is_not_retried() {
    let server = MockServer::start();
    server.push(Reply::json(200, chat_body("The meeting was ab", "length")));
    let (client, _) = fast_client(&server);
    let err = client.chat(&request(), limits()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Truncated);
    assert_eq!(err.code(), "truncated");
    assert!(!err.is_retryable());
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn a_filtered_reply_is_an_error() {
    let server = MockServer::start();
    server.push(Reply::json(200, chat_body("", "content_filter")));
    let (client, _) = fast_client(&server);
    let err = client.chat(&request(), limits()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::ContentFiltered);
    assert!(!err.is_retryable());
}

#[test]
fn an_empty_reply_is_asked_again() {
    let server = MockServer::start();
    server.push(ok("   "));
    server.push(ok("Now with text"));
    let (client, _) = fast_client(&server);
    assert_eq!(
        client.chat(&request(), limits()).unwrap().content,
        "Now with text"
    );
    assert_eq!(server.requests().len(), 2);
}

#[test]
fn empty_replies_on_every_attempt_are_reported() {
    let server = MockServer::start();
    for _ in 0..3 {
        server.push(ok(""));
    }
    let (client, _) = fast_client(&server);
    let err = client.chat(&request(), limits()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::EmptyReply);
    assert_eq!(server.requests().len(), 3);
}

#[test]
fn structured_replies_use_the_json_schema() {
    let server = MockServer::start();
    server.push(ok(GOOD_SUMMARY));
    let (client, _) = fast_client(&server);
    let reply = client
        .chat_structured(&request(), &summary_schema(), limits())
        .unwrap();
    assert_eq!(reply.value["title"], "Kick-off");

    let sent = server.requests()[0].json();
    assert_eq!(sent["response_format"]["type"], "json_schema");
    assert_eq!(sent["response_format"]["json_schema"]["name"], "summary");
    assert_eq!(sent["response_format"]["json_schema"]["strict"], true);
}

#[test]
fn a_degenerate_structured_reply_is_asked_again() {
    let server = MockServer::start();
    server.push(ok(EMPTY_SUMMARY));
    server.push(ok(GOOD_SUMMARY));
    let (client, _) = fast_client(&server);
    let reply = client
        .chat_structured(&request(), &summary_schema(), limits())
        .unwrap();
    assert_eq!(reply.value["body"], "We agreed on May.");
    assert_eq!(server.requests().len(), 2);
}

#[test]
fn degenerate_structured_replies_on_every_attempt_are_reported() {
    let server = MockServer::start();
    for _ in 0..3 {
        server.push(ok(EMPTY_SUMMARY));
    }
    let (client, _) = fast_client(&server);
    let err = client
        .chat_structured(&request(), &summary_schema(), limits())
        .unwrap_err();
    let ErrorKind::Degenerate { fields } = &err.kind else {
        panic!("expected degenerate output, got {err:?}");
    };
    assert_eq!(fields, &["$.body", "$.title"]);
    assert_eq!(err.code(), "degenerate_output");
    assert!(err.is_retryable());
    assert_eq!(server.requests().len(), 3);
}

#[test]
fn invalid_json_and_schema_mismatches_are_asked_again() {
    let server = MockServer::start();
    server.push(ok("Sure! Here is the summary: {"));
    server.push(ok(r#"{"title":"T","body":"B"}"#));
    server.push(ok(GOOD_SUMMARY));
    let (client, _) = fast_client(&server);
    client
        .chat_structured(&request(), &summary_schema(), limits())
        .unwrap();
    assert_eq!(server.requests().len(), 3);

    let server = MockServer::start();
    for _ in 0..3 {
        server.push(ok(r#"{"title":"T","body":"B"}"#));
    }
    let (client, _) = fast_client(&server);
    let err = client
        .chat_structured(&request(), &summary_schema(), limits())
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::SchemaMismatch);
}

#[test]
fn providers_without_json_schema_get_a_json_object_request() {
    let server = MockServer::start();
    server.push(Reply::json(
        400,
        r#"{"error":"response_format json_schema unsupported"}"#,
    ));
    server.push(ok(&format!("```json\n{GOOD_SUMMARY}\n```")));
    let (client, _) = fast_client(&server);
    let reply = client
        .chat_structured(&request(), &summary_schema(), limits())
        .unwrap();
    assert_eq!(reply.value["title"], "Kick-off");

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    let fallback = requests[1].json();
    assert_eq!(fallback["response_format"]["type"], "json_object");
    let messages = fallback["messages"].as_array().unwrap();
    assert!(
        messages.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("JSON schema")
    );
}

#[test]
fn a_prompt_too_long_for_the_context_is_refused_before_sending() {
    let server = MockServer::start();
    let (client, _) = fast_client(&server);
    let mut req = request();
    req.messages.push(Message::user("word ".repeat(10_000)));
    let err = client.chat(&req, limits()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::ContextExceeded);
    assert!(!err.is_retryable());
    assert!(server.requests().is_empty());
}

#[test]
fn unknown_limits_are_refused_before_sending() {
    let server = MockServer::start();
    let (client, _) = fast_client(&server);
    let err = client.chat(&request(), ModelLimits::default()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::LimitsUnknown);
    assert!(server.requests().is_empty());
}

/// The manual check of issue #28 against a real provider. Ignored: it needs the
/// maintainer's own key and network access. Store the key once (it is asked at the
/// prompt, never typed on the command line):
///
/// ```sh
/// security add-generic-password -U -a api-key -s trama-provider-manual-test -w
/// TRAMA_TEST_PROVIDER_URL=https://<openai-compatible>/v1 TRAMA_TEST_MODEL=claude-haiku-5-5 \
///   cargo test -p trama-core --test providers real_provider -- --ignored --nocapture
/// ```
#[test]
#[ignore = "calls a real provider with the maintainer's key"]
fn real_provider_lists_models_and_answers() {
    use trama_core::providers::{Keychain, SecretStore};

    let url = std::env::var("TRAMA_TEST_PROVIDER_URL").expect("TRAMA_TEST_PROVIDER_URL");
    let model = std::env::var("TRAMA_TEST_MODEL").unwrap_or_else(|_| "claude-haiku-5-5".into());
    let service = std::env::var("TRAMA_TEST_KEYCHAIN_SERVICE")
        .unwrap_or_else(|_| "trama-provider-manual-test".into());
    let key = Keychain.get(&service).unwrap();
    let client = Client::new(&url, key, ClientConfig::default());

    let models = client.list_models().unwrap();
    println!("{} models listed", models.len());
    let entry = models
        .iter()
        .find(|m| m.model_id == model)
        .unwrap_or_else(|| panic!("{model} is not in the catalogue"));
    println!("{model}: {:?}", entry.limits);
    // The catalogue may not expose a context window: assume a small one for this check.
    let limits = entry.limits.or(ModelLimits {
        context_tokens: Some(8000),
        max_output_tokens: None,
    });

    let req = ChatRequest {
        model: model.clone(),
        messages: vec![Message::user("Reply with the single word: pong")],
        max_output_tokens: Some(20),
        temperature: Some(0.0),
    };
    let reply = client.chat(&req, limits).unwrap();
    println!("chat: {:?}", reply.content);

    let reply = client
        .chat_structured(
            &ChatRequest {
                messages: vec![Message::user(
                    "Summarize: Anna and Luca agreed to ship the beta in May. Luca writes the notes.",
                )],
                max_output_tokens: Some(400),
                ..req
            },
            &summary_schema(),
            limits,
        )
        .unwrap();
    println!("structured: {}", reply.value);
}
