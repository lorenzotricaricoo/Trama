//! Provider configuration: storage of providers, catalogues and task models, and the
//! flow from a configured task to a call, with an in-memory Keychain and a mock server.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::http::{MockServer, Reply};
use common::{MINUTE, library};
use serde_json::json;
use trama_core::providers::{
    self, ClientConfig, ErrorKind, MemorySecrets, Message, RetryPolicy, Secret, SecretStore,
};
use trama_core::storage::{
    CatalogEntry, Library, ModelLimits, ModelTask, NewProvider, PROVIDER_KEY_SERVICE_PREFIX,
    SCHEMA_VERSION, StorageError,
};

const KEY: &str = "sk-live-do-not-store-9f8e7d";

fn new_provider(url: &str) -> NewProvider {
    NewProvider {
        name: "  My gateway ".into(),
        base_url: format!("{url}/"),
        needs_key: true,
    }
}

fn entry(id: &str, context: Option<u32>, output: Option<u32>) -> CatalogEntry {
    CatalogEntry {
        model_id: id.into(),
        limits: ModelLimits {
            context_tokens: context,
            max_output_tokens: output,
        },
    }
}

fn config() -> ClientConfig {
    ClientConfig {
        timeout: Duration::from_secs(5),
        connect_timeout: Duration::from_secs(5),
        retry: RetryPolicy {
            base_delay: Duration::from_millis(1),
            ..RetryPolicy::default()
        },
    }
}

#[test]
fn schema_has_the_provider_tables() {
    let (_dir, _clock, library) = library();
    assert_eq!(library.schema_version().unwrap(), SCHEMA_VERSION);
    assert!(library.providers().unwrap().is_empty());
    assert!(library.task_models().unwrap().is_empty());
}

#[test]
fn providers_are_added_updated_and_listed() {
    let (_dir, clock, mut library) = library();
    let first = library
        .add_provider(&new_provider("https://one.example.com/v1"))
        .unwrap();
    assert_eq!(first.name, "My gateway");
    assert_eq!(first.base_url, "https://one.example.com/v1");
    assert_eq!(
        first.key_ref,
        Some(format!("{PROVIDER_KEY_SERVICE_PREFIX}{}", first.id))
    );
    assert_eq!(first.catalog_fetched_at, None);

    clock.advance(MINUTE);
    let local = library
        .add_provider(&NewProvider {
            name: "Local".into(),
            base_url: "http://127.0.0.1:1234/v1".into(),
            needs_key: false,
        })
        .unwrap();
    assert_eq!(local.key_ref, None);
    assert_eq!(library.providers().unwrap(), [first.clone(), local.clone()]);

    clock.advance(MINUTE);
    let renamed = library
        .update_provider(first.id, "Work", "https://two.example.com/v1/")
        .unwrap();
    assert_eq!(renamed.name, "Work");
    assert_eq!(renamed.base_url, "https://two.example.com/v1");
    assert_eq!(renamed.key_ref, first.key_ref);
    assert_eq!(renamed.updated_at, clock.now());
}

#[test]
fn invalid_providers_are_refused() {
    let (_dir, _clock, mut library) = library();
    for (name, url) in [
        ("", "https://x.example.com"),
        ("X", "x.example.com"),
        ("X", "https://"),
    ] {
        let err = library
            .add_provider(&NewProvider {
                name: name.into(),
                base_url: url.into(),
                needs_key: false,
            })
            .unwrap_err();
        assert!(matches!(err, StorageError::Invalid(_)), "{name} {url}");
    }
    let missing = trama_core::storage::Id::now_v7();
    assert!(matches!(
        library.update_provider(missing, "X", "https://x.example.com"),
        Err(StorageError::NotFound { .. })
    ));
    assert!(matches!(
        library.remove_provider(missing),
        Err(StorageError::NotFound { .. })
    ));
}

#[test]
fn a_new_catalogue_keeps_overrides_and_marks_removed_models() {
    let (_dir, clock, mut library) = library();
    let p = library
        .add_provider(&new_provider("https://x.example.com"))
        .unwrap();
    library
        .replace_catalog(
            p.id,
            &[
                entry("big", Some(200_000), Some(8192)),
                entry("old", None, None),
                entry("zero", Some(0), Some(0)),
            ],
        )
        .unwrap();
    assert_eq!(
        library.provider(p.id).unwrap().catalog_fetched_at,
        Some(clock.now())
    );
    let zero = library.provider_model(p.id, "zero").unwrap();
    assert_eq!(zero.catalog, ModelLimits::default(), "0 means unknown");

    let old = library
        .set_model_overrides(
            p.id,
            "old",
            ModelLimits {
                context_tokens: Some(32_000),
                max_output_tokens: None,
            },
        )
        .unwrap();
    assert!(old.listed);
    assert_eq!(old.limits().context_tokens, Some(32_000));

    clock.advance(MINUTE);
    library
        .replace_catalog(p.id, &[entry("big", Some(1_000_000), Some(8192))])
        .unwrap();
    let models = library.provider_models(p.id).unwrap();
    let ids: Vec<_> = models
        .iter()
        .map(|m| (m.model_id.as_str(), m.listed))
        .collect();
    assert_eq!(ids, [("big", true), ("old", false), ("zero", false)]);
    assert_eq!(models[0].catalog.context_tokens, Some(1_000_000));
    assert_eq!(
        models[1].overrides.context_tokens,
        Some(32_000),
        "override kept"
    );
}

#[test]
fn models_can_be_added_by_hand_with_their_limits() {
    let (_dir, _clock, mut library) = library();
    let p = library
        .add_provider(&new_provider("https://x.example.com"))
        .unwrap();
    let limits = ModelLimits {
        context_tokens: Some(128_000),
        max_output_tokens: Some(4096),
    };
    let model = library
        .set_model_overrides(p.id, " custom ", limits)
        .unwrap();
    assert_eq!(model.model_id, "custom");
    assert!(!model.listed);
    assert_eq!(model.limits(), limits);

    let err = library
        .set_model_overrides(
            p.id,
            "custom",
            ModelLimits {
                context_tokens: Some(0),
                max_output_tokens: None,
            },
        )
        .unwrap_err();
    assert!(matches!(err, StorageError::Invalid(_)));
    assert!(library.set_model_overrides(p.id, " ", limits).is_err());
}

#[test]
fn each_task_has_its_own_model() {
    let (_dir, _clock, mut library) = library();
    let p = library
        .add_provider(&new_provider("https://x.example.com"))
        .unwrap();
    library
        .replace_catalog(
            p.id,
            &[entry("a", Some(1000), None), entry("b", Some(2000), None)],
        )
        .unwrap();
    assert_eq!(library.task_model(ModelTask::Summary).unwrap(), None);
    assert!(matches!(
        library.set_task_model(ModelTask::Summary, p.id, "missing"),
        Err(StorageError::NotFound { .. })
    ));

    library
        .set_task_model(ModelTask::Summary, p.id, "a")
        .unwrap();
    library
        .set_task_model(ModelTask::Summary, p.id, "b")
        .unwrap();
    let choice = library.task_model(ModelTask::Summary).unwrap().unwrap();
    assert_eq!(choice.model_id, "b");
    let (provider, model) = library
        .resolve_task_model(ModelTask::Summary)
        .unwrap()
        .unwrap();
    assert_eq!(provider.id, p.id);
    assert_eq!(model.limits().context_tokens, Some(2000));

    library.clear_task_model(ModelTask::Summary).unwrap();
    assert_eq!(
        library.resolve_task_model(ModelTask::Summary).unwrap(),
        None
    );
}

#[test]
fn several_providers_coexist() {
    let (_dir, _clock, mut library) = library();
    let secrets = MemorySecrets::new();
    let one = providers::add_provider(
        &mut library,
        &secrets,
        &new_provider("https://one.example.com/v1"),
        Some(&Secret::new("key-one")),
    )
    .unwrap();
    let two = providers::add_provider(
        &mut library,
        &secrets,
        &new_provider("https://two.example.com/v1"),
        Some(&Secret::new("key-two")),
    )
    .unwrap();
    library
        .replace_catalog(one.id, &[entry("same-name", Some(1000), None)])
        .unwrap();
    library
        .replace_catalog(two.id, &[entry("same-name", Some(9000), None)])
        .unwrap();
    assert_eq!(secrets.len(), 2);
    assert_eq!(
        library
            .provider_model(two.id, "same-name")
            .unwrap()
            .catalog
            .context_tokens,
        Some(9000)
    );
    assert_eq!(
        secrets.get(one.key_ref.as_deref().unwrap()).unwrap(),
        Some(Secret::new("key-one"))
    );
}

/// Every file of the library, as bytes, to look for a secret.
fn library_bytes(library: &Library) -> Vec<u8> {
    let mut bytes = Vec::new();
    for entry in std::fs::read_dir(library.root()).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            bytes.extend(std::fs::read(path).unwrap());
        }
    }
    bytes
}

#[test]
fn the_api_key_goes_to_the_secret_store_never_to_the_library() {
    let (_dir, _clock, mut library) = library();
    let secrets = MemorySecrets::new();
    let provider = providers::add_provider(
        &mut library,
        &secrets,
        &new_provider("https://x.example.com/v1"),
        Some(&Secret::new(KEY)),
    )
    .unwrap();
    let service = provider.key_ref.clone().unwrap();
    assert_eq!(secrets.get(&service).unwrap(), Some(Secret::new(KEY)));

    providers::set_api_key(&secrets, &provider, &Secret::new(format!("{KEY}-rotated"))).unwrap();
    library.set_setting("probe", "flush").unwrap();
    let bytes = library_bytes(&library);
    assert!(
        !bytes.windows(KEY.len()).any(|w| w == KEY.as_bytes()),
        "the key reached the library files"
    );
}

/// A store whose writes fail, like a Keychain that refuses access.
struct BrokenStore;

impl SecretStore for BrokenStore {
    fn get(&self, _: &str) -> Result<Option<Secret>, providers::ProviderError> {
        Err(providers::ProviderError::new(ErrorKind::Keychain, "denied"))
    }
    fn set(&self, _: &str, _: &Secret) -> Result<(), providers::ProviderError> {
        Err(providers::ProviderError::new(ErrorKind::Keychain, "denied"))
    }
    fn delete(&self, _: &str) -> Result<(), providers::ProviderError> {
        Err(providers::ProviderError::new(ErrorKind::Keychain, "denied"))
    }
}

#[test]
fn a_key_that_cannot_be_stored_leaves_no_provider() {
    let (_dir, _clock, mut library) = library();
    let err = providers::add_provider(
        &mut library,
        &BrokenStore,
        &new_provider("https://x.example.com/v1"),
        Some(&Secret::new(KEY)),
    )
    .unwrap_err();
    assert_eq!(err.code(), "keychain");
    assert!(library.providers().unwrap().is_empty());
}

#[test]
fn removing_a_provider_removes_its_key_models_and_task_choices() {
    let (_dir, _clock, mut library) = library();
    let secrets = MemorySecrets::new();
    let provider = providers::add_provider(
        &mut library,
        &secrets,
        &new_provider("https://x.example.com/v1"),
        Some(&Secret::new(KEY)),
    )
    .unwrap();
    library
        .replace_catalog(provider.id, &[entry("m", Some(1000), None)])
        .unwrap();
    library
        .set_task_model(ModelTask::Summary, provider.id, "m")
        .unwrap();

    providers::remove_provider(&mut library, &secrets, provider.id).unwrap();
    assert!(secrets.is_empty());
    assert!(library.providers().unwrap().is_empty());
    assert_eq!(library.task_model(ModelTask::Summary).unwrap(), None);
    assert!(library.provider_model(provider.id, "m").is_err());
}

#[test]
fn refreshing_the_catalogue_reads_the_provider_and_stores_it() {
    let server = MockServer::start();
    server.push(Reply::json(
        200,
        r#"{"data":[{"id":"claude-haiku-5-5","context_length":200000,"max_output_tokens":64000},
                    {"id":"no-limits"}]}"#,
    ));
    let (_dir, _clock, mut library) = library();
    let secrets = MemorySecrets::new();
    let provider = providers::add_provider(
        &mut library,
        &secrets,
        &new_provider(&server.url()),
        Some(&Secret::new(KEY)),
    )
    .unwrap();
    let library = Arc::new(Mutex::new(library));

    let models = providers::refresh_catalog(&library, &secrets, provider.id, config()).unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].model_id, "claude-haiku-5-5");
    assert_eq!(models[0].limits().max_output_tokens, Some(64_000));
    assert_eq!(models[1].limits(), ModelLimits::default());
    assert_eq!(
        server.requests()[0].header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
}

#[test]
fn a_failed_refresh_keeps_the_previous_catalogue() {
    let server = MockServer::start();
    server.push(Reply::json(401, "{}"));
    let (_dir, _clock, mut library) = library();
    let secrets = MemorySecrets::new();
    let provider = providers::add_provider(
        &mut library,
        &secrets,
        &new_provider(&server.url()),
        Some(&Secret::new(KEY)),
    )
    .unwrap();
    library
        .replace_catalog(provider.id, &[entry("kept", Some(1000), None)])
        .unwrap();
    let library = Arc::new(Mutex::new(library));

    let err = providers::refresh_catalog(&library, &secrets, provider.id, config()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::AuthFailed);
    let library = library.lock().unwrap();
    assert!(library.provider_model(provider.id, "kept").unwrap().listed);
}

#[test]
fn a_task_without_a_usable_model_fails_with_a_code() {
    let (_dir, _clock, mut library) = library();
    let secrets = MemorySecrets::new();
    let err = providers::task_client(&library, &secrets, ModelTask::Summary, config()).unwrap_err();
    assert_eq!(err.code(), "no_model_for_task");
    assert!(!err.is_retryable());

    // A key never stored.
    let provider = providers::add_provider(
        &mut library,
        &secrets,
        &new_provider("https://x.example.com/v1"),
        None,
    )
    .unwrap();
    library
        .replace_catalog(provider.id, &[entry("m", None, None)])
        .unwrap();
    library
        .set_task_model(ModelTask::Summary, provider.id, "m")
        .unwrap();
    let err = providers::task_client(&library, &secrets, ModelTask::Summary, config()).unwrap_err();
    assert_eq!(err.code(), "limits_unknown");

    library
        .set_model_overrides(
            provider.id,
            "m",
            ModelLimits {
                context_tokens: Some(16_000),
                max_output_tokens: None,
            },
        )
        .unwrap();
    let err = providers::task_client(&library, &secrets, ModelTask::Summary, config()).unwrap_err();
    assert_eq!(err.code(), "missing_key");

    providers::set_api_key(&secrets, &provider, &Secret::new(KEY)).unwrap();
    let task = providers::task_client(&library, &secrets, ModelTask::Summary, config()).unwrap();
    assert_eq!(task.limits().context_tokens, Some(16_000));
}

#[test]
fn a_configured_task_calls_its_model() {
    let server = MockServer::start();
    let summary = r#"{"title":"Beta","body":"Ship in May.","key_points":["May"]}"#;
    server.push(Reply::json(
        200,
        json!({"choices":[{"message":{"content":summary},"finish_reason":"stop"}]}).to_string(),
    ));
    let (_dir, _clock, mut library) = library();
    let secrets = MemorySecrets::new();
    let provider = providers::add_provider(
        &mut library,
        &secrets,
        &new_provider(&server.url()),
        Some(&Secret::new(KEY)),
    )
    .unwrap();
    library
        .replace_catalog(provider.id, &[entry("cheap", Some(100_000), Some(2000))])
        .unwrap();
    library
        .set_task_model(ModelTask::Summary, provider.id, "cheap")
        .unwrap();

    let task = providers::task_client(&library, &secrets, ModelTask::Summary, config()).unwrap();
    let schema = providers::JsonSchema {
        name: "summary".into(),
        strict: true,
        schema: json!({"type":"object","required":["title","body","key_points"],
                       "properties":{"title":{"type":"string"},"body":{"type":"string"},
                                     "key_points":{"type":"array","items":{"type":"string"}}}}),
    };
    let reply = task
        .chat_structured(vec![Message::user("Summarize the meeting")], &schema, None)
        .unwrap();
    assert_eq!(reply.value["title"], "Beta");
    let sent = server.requests()[0].json();
    assert_eq!(sent["model"], "cheap");
    assert_eq!(sent["max_tokens"], 2000);
}

#[test]
fn provider_errors_become_job_errors() {
    use trama_core::jobs::JobError;
    let err = providers::ProviderError::new(ErrorKind::Truncated, "stopped");
    let job: JobError = err.into();
    assert_eq!(job.code, "truncated");
    assert!(!job.retryable);
}

#[test]
fn a_key_never_follows_the_provider_to_another_origin() {
    let (_dir, _clock, mut library) = library();
    let secrets = MemorySecrets::new();
    let provider = providers::add_provider(
        &mut library,
        &secrets,
        &new_provider("https://api.example.com/v1"),
        Some(&Secret::new(KEY)),
    )
    .unwrap();
    let service = provider.key_ref.clone().unwrap();
    let update = |library: &mut Library, url: &str, key: Option<&Secret>| {
        providers::update_provider(library, &secrets, provider.id, "Renamed", url, key)
    };

    // Same origin, other path: the key stays.
    let renamed = update(&mut library, "https://API.example.com:443/v2", None).unwrap();
    assert_eq!(renamed.name, "Renamed");
    assert_eq!(secrets.get(&service).unwrap(), Some(Secret::new(KEY)));

    // Another host, or another port, without a new key: refused, nothing changes.
    for url in [
        "https://attacker.example.net/v1",
        "https://api.example.com:8443/v1",
    ] {
        let err = update(&mut library, url, None).unwrap_err();
        assert_eq!(err.code(), "key_required_for_new_url");
        assert!(!err.is_retryable());
        assert!(!err.detail.contains(KEY));
    }
    assert_eq!(
        library.provider(provider.id).unwrap().base_url,
        "https://API.example.com:443/v2"
    );

    // With a new key the move is accepted and the new key replaces the old one.
    let new_key = Secret::new("sk-new-key-for-other-host");
    let moved = update(&mut library, "https://other.example.net/v1", Some(&new_key)).unwrap();
    assert_eq!(moved.base_url, "https://other.example.net/v1");
    assert_eq!(secrets.get(&service).unwrap(), Some(new_key));

    // A provider without a key can move freely.
    let local = providers::add_provider(
        &mut library,
        &secrets,
        &NewProvider {
            name: "Local".into(),
            base_url: "http://127.0.0.1:1234/v1".into(),
            needs_key: false,
        },
        None,
    )
    .unwrap();
    providers::update_provider(
        &mut library,
        &secrets,
        local.id,
        "Local",
        "http://localhost:11434/v1",
        None,
    )
    .unwrap();
}
