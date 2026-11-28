//! AI providers: OpenAI-compatible endpoints for language models (ADR 0003, see
//! `docs/architecture/providers.md`).
//!
//! - Several providers can be configured at once; each is a row in the library
//!   ([`crate::storage::Provider`]) and its API key is an item in the macOS Keychain
//!   ([`secrets`]), never in the database or in logs.
//! - The model catalogue is read from the provider (`GET /models`) with the limits it
//!   exposes; the user can override them ([`crate::storage::ModelLimits`]).
//! - Each task uses the model the user chose for it ([`crate::storage::ModelTask`]).
//! - The tokens of every reply can be recorded with their cost
//!   ([`TaskModelClient::recording_usage`]). When the user enforces a monthly budget, a
//!   call is refused with `budget_exceeded` once the month's spend reached it, and with
//!   `price_missing` for a model without prices, whose cost could not be counted
//!   ([`check_budget`]).
//! - Calls are robust (principle A4): timeouts, retries with backoff, `Retry-After`,
//!   and checks for truncated, filtered, empty and degenerate replies, all surfaced as
//!   coded errors ([`ProviderError`]).

mod catalog;
mod client;
mod error;
pub mod secrets;
pub mod structured;

pub use catalog::parse_catalog;
pub use client::{
    ChatReply, ChatRequest, Client, ClientConfig, JsonSchema, Message, MessageRole, RequestGuard,
    RetryPolicy, StructuredReply, TokenUsage, UsageHook, estimate_prompt_tokens, estimate_tokens,
};
pub use error::{ErrorKind, ProviderError, error_code};
pub use secrets::{Keychain, MemorySecrets, Secret, SecretStore};

use crate::i18n::Catalog;
use crate::jobs::{SharedLibrary, lock};
use crate::storage::{
    Id, Library, ModelLimits, ModelTask, NewProvider, Provider, ProviderModel, StorageError,
    UsageRecord, url_origin,
};

/// Messages for the provider error codes, one catalog per supported locale.
pub const CATALOGS: &[Catalog] = &[
    Catalog {
        locale: "en",
        name: "providers",
        source: include_str!("../../../../locales/en/providers.ftl"),
    },
    Catalog {
        locale: "it",
        name: "providers",
        source: include_str!("../../../../locales/it/providers.ftl"),
    },
];

/// Message id of an error code in [`CATALOGS`], for example `provider-error-auth-failed`.
pub fn error_message_id(code: &str) -> String {
    format!("provider-error-{}", code.replace('_', "-"))
}

/// Adds a provider and stores its API key in `secrets`. A provider that needs a key can
/// be added without one and get it later with [`set_api_key`].
pub fn add_provider(
    library: &mut Library,
    secrets: &dyn SecretStore,
    new: &NewProvider,
    key: Option<&Secret>,
) -> Result<Provider, ProviderError> {
    let provider = library.add_provider(new)?;
    if let (Some(service), Some(key)) = (&provider.key_ref, key)
        && let Err(err) = secrets.set(service, key)
    {
        // Don't leave a provider whose key could not be stored.
        library.remove_provider(provider.id)?;
        return Err(err);
    }
    Ok(provider)
}

/// Renames a provider, changes its URL and, if `key` is given, replaces its API key.
///
/// A key belongs to the origin (scheme, host and port) it was entered for: when the new URL
/// has another origin, a provider that needs a key must get a new one in the same call,
/// otherwise the change is refused with `key_required_for_new_url` and nothing changes.
/// The stored key is never sent to a different server.
pub fn update_provider(
    library: &mut Library,
    secrets: &dyn SecretStore,
    id: Id,
    name: &str,
    base_url: &str,
    key: Option<&Secret>,
) -> Result<Provider, ProviderError> {
    let old = library.provider(id)?;
    let moved = url_origin(&old.base_url) != url_origin(base_url);
    if moved && old.key_ref.is_some() && key.is_none() {
        return Err(ProviderError::new(
            ErrorKind::KeyRequiredForNewUrl,
            format!("provider {id} moved to another origin without a new key"),
        ));
    }
    let provider = library.update_provider(id, name, base_url)?;
    if let Some(key) = key
        && let Err(err) = set_api_key(secrets, &provider, key)
    {
        if moved {
            // The old key must not follow the provider to the new origin.
            library.update_provider(id, &old.name, &old.base_url)?;
        }
        return Err(err);
    }
    Ok(provider)
}

/// Stores or replaces the API key of a provider that needs one.
pub fn set_api_key(
    secrets: &dyn SecretStore,
    provider: &Provider,
    key: &Secret,
) -> Result<(), ProviderError> {
    let service = provider.key_ref.as_deref().ok_or_else(|| {
        ProviderError::from(StorageError::Invalid("this provider needs no API key"))
    })?;
    secrets.set(service, key)
}

/// Removes a provider, its models and its Keychain item. The key goes first: if the
/// Keychain refuses, the provider stays and can be removed again, so no key is left
/// behind without a provider pointing at it. A key already gone is not an error.
pub fn remove_provider(
    library: &mut Library,
    secrets: &dyn SecretStore,
    id: Id,
) -> Result<(), ProviderError> {
    let provider = library.provider(id)?;
    if let Some(service) = &provider.key_ref {
        secrets.delete(service)?;
    }
    library.remove_provider(id)?;
    Ok(())
}

/// A client for a provider, with its key read from `secrets`.
pub fn client_for(
    provider: &Provider,
    secrets: &dyn SecretStore,
    config: ClientConfig,
) -> Result<Client, ProviderError> {
    let key = match &provider.key_ref {
        Some(service) => Some(secrets.get(service)?.ok_or_else(|| {
            ProviderError::new(
                ErrorKind::MissingKey,
                format!("no API key stored for provider {}", provider.id),
            )
        })?),
        None => None,
    };
    Ok(Client::new(&provider.base_url, key, config))
}

/// Reads a provider's model catalogue and stores it. The library is locked only to read
/// the provider and to store the result, never during the HTTP call.
pub fn refresh_catalog(
    library: &SharedLibrary,
    secrets: &dyn SecretStore,
    provider_id: Id,
    config: ClientConfig,
) -> Result<Vec<ProviderModel>, ProviderError> {
    let provider = lock(library).provider(provider_id)?;
    let entries = client_for(&provider, secrets, config)?.list_models()?;
    let mut library = lock(library);
    library.replace_catalog(provider_id, &entries)?;
    Ok(library.provider_models(provider_id)?)
}

/// Everything needed to run a task: its provider, model, limits and a client.
#[derive(Debug, Clone)]
pub struct TaskModelClient {
    pub task: ModelTask,
    pub provider: Provider,
    pub model: ProviderModel,
    pub client: Client,
}

impl TaskModelClient {
    /// Records the tokens of every reply in `library`, with their cost, and checks the
    /// budget again before every request ([`check_budget`]), so a task with many calls
    /// stops at the budget. The library is locked briefly around each call: don't hold
    /// its lock while calling the model.
    pub fn recording_usage(mut self, library: SharedLibrary) -> Self {
        let provider_id = self.provider.id;
        let model_id = self.model.model_id.clone();
        let task = self.task;
        let guard_library = std::sync::Arc::clone(&library);
        let guard_model = model_id.clone();
        let guard: RequestGuard = std::sync::Arc::new(move || {
            let library = lock(&guard_library);
            let model = library.provider_model(provider_id, &guard_model)?;
            check_budget(&library, &model)
        });
        let hook: UsageHook = std::sync::Arc::new(move |usage: TokenUsage| {
            let record = UsageRecord {
                provider_id,
                model_id: model_id.clone(),
                task: Some(task),
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
            };
            if let Err(err) = lock(&library).record_usage(&record) {
                // Losing one usage row must not fail a reply that was already paid for.
                eprintln!("trama: cannot record model usage: {err}");
            }
        });
        self.client = self.client.with_usage_hook(hook).with_request_guard(guard);
        self
    }

    /// The limits in effect for the task's model.
    pub fn limits(&self) -> ModelLimits {
        self.model.limits()
    }

    /// A checked chat completion with the task's model.
    pub fn chat(
        &self,
        messages: Vec<Message>,
        max_output_tokens: Option<u32>,
    ) -> Result<ChatReply, ProviderError> {
        self.client
            .chat(&self.request(messages, max_output_tokens), self.limits())
    }

    /// A checked structured completion with the task's model.
    pub fn chat_structured(
        &self,
        messages: Vec<Message>,
        schema: &JsonSchema,
        max_output_tokens: Option<u32>,
    ) -> Result<StructuredReply, ProviderError> {
        self.client.chat_structured(
            &self.request(messages, max_output_tokens),
            schema,
            self.limits(),
        )
    }

    fn request(&self, messages: Vec<Message>, max_output_tokens: Option<u32>) -> ChatRequest {
        ChatRequest {
            model: self.model.model_id.clone(),
            messages,
            max_output_tokens,
            temperature: None,
        }
    }
}

/// Whether a call to `model` fits the monthly budget. Only an enforced budget refuses:
/// with `price_missing` if the model lacks a price (its cost could not be counted), with
/// `budget_exceeded` once the month's spend reached the budget.
pub fn check_budget(library: &Library, model: &ProviderModel) -> Result<(), ProviderError> {
    let budget = library.budget_status()?;
    if !budget.enforce || budget.budget_micros.is_none() {
        return Ok(());
    }
    if model.prices.input_micros.is_none() || model.prices.output_micros.is_none() {
        return Err(ProviderError::new(
            ErrorKind::PriceMissing,
            format!(
                "{} has no price and the monthly budget is enforced",
                model.model_id
            ),
        ));
    }
    if budget.blocks_calls() {
        return Err(ProviderError::new(
            ErrorKind::BudgetExceeded,
            format!(
                "spent {} of a monthly budget of {} (millionths)",
                budget.totals.cost_micros,
                budget.budget_micros.unwrap_or_default()
            ),
        ));
    }
    Ok(())
}

/// The client for the model chosen for `task`. Fails with `no_model_for_task` if none is
/// chosen, `limits_unknown` if its context window is unknown, `missing_key` if the key
/// is not in the Keychain, and as [`check_budget`] does.
pub fn task_client(
    library: &Library,
    secrets: &dyn SecretStore,
    task: ModelTask,
    config: ClientConfig,
) -> Result<TaskModelClient, ProviderError> {
    let (provider, model) = library.resolve_task_model(task)?.ok_or_else(|| {
        ProviderError::new(
            ErrorKind::NoModelForTask,
            format!("no model chosen for {}", task.as_str()),
        )
    })?;
    if model.limits().context_tokens.is_none() {
        return Err(ProviderError::new(
            ErrorKind::LimitsUnknown,
            format!(
                "context window of {} unknown: set it by hand",
                model.model_id
            ),
        ));
    }
    check_budget(library, &model)?;
    let client = client_for(&provider, secrets, config)?;
    Ok(TaskModelClient {
        task,
        provider,
        model,
        client,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Localizer, SUPPORTED_LOCALES};

    #[test]
    fn every_error_code_is_translated() {
        for locale in SUPPORTED_LOCALES {
            let locale = locale.to_string();
            let l10n = Localizer::new(&[locale.as_str()], CATALOGS).unwrap();
            assert_eq!(l10n.locale().to_string(), locale);
            for code in error_code::ALL {
                let id = error_message_id(code);
                assert_ne!(l10n.message(&id, None), id, "{id} missing in {locale}");
            }
        }
    }
}
