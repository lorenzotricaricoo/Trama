//! Settings kept in the library, model prices, token usage and the monthly budget, with an
//! in-memory Keychain and a mock server (no real provider is ever called).

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::http::{MockServer, Reply};
use common::{DAY, ManualClock, library};
use serde_json::json;
use trama_core::providers::{self, ClientConfig, MemorySecrets, Message, RetryPolicy, Secret};
use trama_core::storage::{
    AudioRetention, BudgetState, CatalogEntry, Library, LibrarySettings, ModelLimits, ModelPrices,
    ModelTask, NewProvider, StorageError, UsageRecord, UsageSettings, month_start, setting_key,
};

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

fn priced(input: u64, output: u64) -> ModelPrices {
    ModelPrices {
        input_micros: Some(input),
        output_micros: Some(output),
    }
}

#[test]
fn library_settings_have_defaults() {
    let (_dir, _clock, library) = library();
    let settings = library.library_settings().unwrap();
    assert_eq!(settings, LibrarySettings::default());
    assert_eq!(settings.meeting_language, "en");
    assert_eq!(settings.audio_retention, AudioRetention::Never);
    assert_eq!(settings.usage.currency, "USD");
    assert_eq!(settings.usage.monthly_budget_micros, None);
}

#[test]
fn library_settings_survive_a_restart() {
    let dir = tempfile::TempDir::new().unwrap();
    let wanted = LibrarySettings {
        meeting_language: "it".into(),
        audio_retention: AudioRetention::Days30,
        usage: UsageSettings {
            currency: "EUR".into(),
            monthly_budget_micros: Some(20_000_000),
            warn_percent: 75,
            enforce_budget: true,
        },
    };
    {
        let mut library = Library::open(dir.path()).unwrap();
        assert_eq!(library.set_library_settings(&wanted).unwrap(), wanted);
    }
    let library = Library::open(dir.path()).unwrap();
    assert_eq!(library.library_settings().unwrap(), wanted);
    // The meeting language uses the key the summary job reads.
    assert_eq!(
        library.setting(setting_key::MEETING_LANGUAGE).unwrap(),
        Some("it".into())
    );

    // Removing the budget removes its row.
    let mut library = library;
    let mut no_budget = wanted.clone();
    no_budget.usage.monthly_budget_micros = None;
    library.set_library_settings(&no_budget).unwrap();
    assert_eq!(
        library.setting(setting_key::BUDGET_MONTHLY_MICROS).unwrap(),
        None
    );
}

#[test]
fn invalid_library_settings_are_refused_and_change_nothing() {
    let (_dir, _clock, mut library) = library();
    let base = LibrarySettings::default();
    let cases = [
        LibrarySettings {
            meeting_language: "Italian".into(),
            ..base.clone()
        },
        LibrarySettings {
            usage: UsageSettings {
                currency: "euro".into(),
                ..base.usage.clone()
            },
            ..base.clone()
        },
        LibrarySettings {
            usage: UsageSettings {
                warn_percent: 0,
                ..base.usage.clone()
            },
            ..base.clone()
        },
        LibrarySettings {
            usage: UsageSettings {
                monthly_budget_micros: Some(u64::MAX),
                ..base.usage.clone()
            },
            ..base.clone()
        },
    ];
    for settings in cases {
        assert!(matches!(
            library.set_library_settings(&settings),
            Err(StorageError::Invalid(_))
        ));
    }
    assert_eq!(library.library_settings().unwrap(), base);
}

#[test]
fn damaged_settings_fall_back_to_their_defaults() {
    let (_dir, _clock, mut library) = library();
    library
        .set_setting(setting_key::AUDIO_RETENTION, "forever")
        .unwrap();
    library
        .set_setting(setting_key::BUDGET_WARN_PERCENT, "500")
        .unwrap();
    library
        .set_setting(setting_key::MEETING_LANGUAGE, "??")
        .unwrap();
    assert_eq!(
        library.library_settings().unwrap(),
        LibrarySettings::default()
    );
}

#[test]
fn the_meeting_language_is_the_one_summaries_use() {
    let (_dir, _clock, mut library) = library();
    assert_eq!(
        library.meeting_language().unwrap(),
        library.library_settings().unwrap().meeting_language
    );
    let mut settings = library.library_settings().unwrap();
    settings.meeting_language = "it".into();
    library.set_library_settings(&settings).unwrap();
    assert_eq!(library.meeting_language().unwrap(), "it");
}

/// A library with one provider, one priced model chosen for the summary.
fn priced_library(server: &MockServer) -> (tempfile::TempDir, ManualClock, Library, MemorySecrets) {
    let (dir, clock, mut library) = library();
    let secrets = MemorySecrets::new();
    let provider = providers::add_provider(
        &mut library,
        &secrets,
        &NewProvider {
            name: "Mock".into(),
            base_url: server.url(),
            needs_key: true,
        },
        Some(&Secret::new("sk-test-not-real")),
    )
    .unwrap();
    library
        .replace_catalog(
            provider.id,
            &[CatalogEntry {
                model_id: "cheap".into(),
                limits: ModelLimits {
                    context_tokens: Some(100_000),
                    max_output_tokens: Some(2000),
                },
            }],
        )
        .unwrap();
    library
        .set_model_prices(provider.id, "cheap", priced(3_000_000, 15_000_000))
        .unwrap();
    library
        .set_task_model(ModelTask::Summary, provider.id, "cheap")
        .unwrap();
    (dir, clock, library, secrets)
}

fn reply_with_usage(input: u64, output: u64) -> Reply {
    Reply::json(
        200,
        json!({
            "choices": [{"message": {"content": "Fine."}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": input, "completion_tokens": output}
        })
        .to_string(),
    )
}

#[test]
fn prices_survive_a_new_catalogue() {
    let server = MockServer::start();
    let (_dir, _clock, mut library, _secrets) = priced_library(&server);
    let provider = library.providers().unwrap()[0].clone();
    library
        .replace_catalog(
            provider.id,
            &[CatalogEntry {
                model_id: "cheap".into(),
                limits: ModelLimits::default(),
            }],
        )
        .unwrap();
    let model = library.provider_model(provider.id, "cheap").unwrap();
    assert_eq!(model.prices, priced(3_000_000, 15_000_000));

    // A model can be priced before any catalogue lists it.
    let manual = library
        .set_model_prices(provider.id, "by-hand", ModelPrices::default())
        .unwrap();
    assert!(!manual.listed);
}

#[test]
fn every_reply_is_recorded_with_its_cost() {
    let server = MockServer::start();
    server.push(reply_with_usage(1000, 200));
    server.push(reply_with_usage(2000, 0));
    let (_dir, clock, library, secrets) = priced_library(&server);
    let library = Arc::new(Mutex::new(library));

    let task = {
        let guard = library.lock().unwrap();
        providers::task_client(&guard, &secrets, ModelTask::Summary, config()).unwrap()
    }
    .recording_usage(Arc::clone(&library));
    task.chat(vec![Message::user("one")], None).unwrap();
    task.chat(vec![Message::user("two")], None).unwrap();

    let library = library.lock().unwrap();
    let totals = library.usage_since(month_start(clock.now())).unwrap();
    assert_eq!(totals.requests, 2);
    assert_eq!(totals.input_tokens, 3000);
    assert_eq!(totals.output_tokens, 200);
    // 3000 × 3 + 200 × 15 millionths per token.
    assert_eq!(totals.cost_micros, 9000 + 3000);
    assert_eq!(totals.unpriced_requests, 0);
}

#[test]
fn usage_of_an_unpriced_model_is_counted_apart() {
    let (_dir, clock, mut library) = library();
    let provider = library
        .add_provider(&NewProvider {
            name: "Local".into(),
            base_url: "http://127.0.0.1:1/v1".into(),
            needs_key: false,
        })
        .unwrap();
    let record = UsageRecord {
        provider_id: provider.id,
        model_id: "unknown".into(),
        task: None,
        input_tokens: 10,
        output_tokens: 5,
    };
    assert_eq!(library.record_usage(&record).unwrap(), None);
    let totals = library.usage_since(month_start(clock.now())).unwrap();
    assert_eq!((totals.requests, totals.unpriced_requests), (1, 1));
    assert_eq!(totals.cost_micros, 0);
}

#[test]
fn the_budget_warns_then_blocks_when_enforced() {
    let server = MockServer::start();
    let (_dir, clock, mut library, secrets) = priced_library(&server);
    let provider = library.providers().unwrap()[0].clone();
    let spend = |library: &mut Library, micros: u64| {
        // Prompt tokens at 3 millionths each.
        let record = UsageRecord {
            provider_id: provider.id,
            model_id: "cheap".into(),
            task: Some(ModelTask::Summary),
            input_tokens: micros / 3,
            output_tokens: 0,
        };
        library.record_usage(&record).unwrap();
    };

    assert_eq!(
        library.budget_status().unwrap().state,
        BudgetState::NoBudget
    );
    let mut settings = library.library_settings().unwrap();
    settings.usage.monthly_budget_micros = Some(30_000);
    settings.usage.warn_percent = 80;
    library.set_library_settings(&settings).unwrap();
    assert_eq!(library.budget_status().unwrap().state, BudgetState::Ok);

    spend(&mut library, 24_000);
    let status = library.budget_status().unwrap();
    assert_eq!(status.state, BudgetState::Warning);
    assert_eq!(status.totals.cost_micros, 24_000);
    assert_eq!(status.period_start, month_start(clock.now()));

    spend(&mut library, 6000);
    let status = library.budget_status().unwrap();
    assert_eq!(status.state, BudgetState::Exceeded);
    // Not enforced: the task still runs.
    assert!(!status.blocks_calls());
    assert!(providers::task_client(&library, &secrets, ModelTask::Summary, config()).is_ok());

    settings.usage.enforce_budget = true;
    library.set_library_settings(&settings).unwrap();
    let err = providers::task_client(&library, &secrets, ModelTask::Summary, config()).unwrap_err();
    assert_eq!(err.code(), "budget_exceeded");
    assert!(!err.is_retryable());

    // A new month starts from zero.
    clock.advance(31 * DAY);
    assert_eq!(library.budget_status().unwrap().state, BudgetState::Ok);
    assert!(providers::task_client(&library, &secrets, ModelTask::Summary, config()).is_ok());
}

/// Turns on an enforced monthly budget of `micros`.
fn enforce_budget(library: &mut Library, micros: u64) {
    let mut settings = library.library_settings().unwrap();
    settings.usage.monthly_budget_micros = Some(micros);
    settings.usage.enforce_budget = true;
    library.set_library_settings(&settings).unwrap();
}

#[test]
fn an_enforced_budget_refuses_models_without_a_price() {
    let server = MockServer::start();
    let (_dir, _clock, mut library, secrets) = priced_library(&server);
    let provider = library.providers().unwrap()[0].clone();
    library
        .set_model_prices(
            provider.id,
            "cheap",
            ModelPrices {
                input_micros: Some(3_000_000),
                output_micros: None,
            },
        )
        .unwrap();
    // Without a budget, or with one that is only a warning, the price isn't needed.
    assert!(providers::task_client(&library, &secrets, ModelTask::Summary, config()).is_ok());

    enforce_budget(&mut library, 1_000_000);
    let err = providers::task_client(&library, &secrets, ModelTask::Summary, config()).unwrap_err();
    assert_eq!(err.code(), "price_missing");
    assert!(!err.is_retryable());

    library
        .set_model_prices(provider.id, "cheap", priced(3_000_000, 15_000_000))
        .unwrap();
    assert!(providers::task_client(&library, &secrets, ModelTask::Summary, config()).is_ok());
}

#[test]
fn the_budget_is_checked_again_before_every_request() {
    let server = MockServer::start();
    // 4000 prompt tokens at 3 millionths each: 12 000, past a budget of 10 000.
    server.push(reply_with_usage(4000, 0));
    server.push(reply_with_usage(10, 0));
    let (_dir, _clock, mut library, secrets) = priced_library(&server);
    enforce_budget(&mut library, 10_000);
    let library = Arc::new(Mutex::new(library));

    let task = {
        let guard = library.lock().unwrap();
        providers::task_client(&guard, &secrets, ModelTask::Summary, config()).unwrap()
    }
    .recording_usage(Arc::clone(&library));
    task.chat(vec![Message::user("one")], None).unwrap();
    let err = task.chat(vec![Message::user("two")], None).unwrap_err();
    assert_eq!(err.code(), "budget_exceeded");
    // The refused request was never sent.
    assert_eq!(server.requests().len(), 1);

    // Prices removed during a task stop it too.
    let mut guard = library.lock().unwrap();
    let provider = guard.providers().unwrap()[0].clone();
    enforce_budget(&mut guard, 1_000_000);
    guard
        .set_model_prices(provider.id, "cheap", ModelPrices::default())
        .unwrap();
    drop(guard);
    let err = task.chat(vec![Message::user("three")], None).unwrap_err();
    assert_eq!(err.code(), "price_missing");
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn spend_is_kept_when_the_provider_is_removed() {
    let server = MockServer::start();
    let (_dir, clock, mut library, secrets) = priced_library(&server);
    let provider = library.providers().unwrap()[0].clone();
    library
        .record_usage(&UsageRecord {
            provider_id: provider.id,
            model_id: "cheap".into(),
            task: Some(ModelTask::Summary),
            input_tokens: 1000,
            output_tokens: 0,
        })
        .unwrap();
    providers::remove_provider(&mut library, &secrets, provider.id).unwrap();
    let totals = library.usage_since(month_start(clock.now())).unwrap();
    assert_eq!(totals.cost_micros, 3000);
}
