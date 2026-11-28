//! Bridge between the AI providers and the frontend: providers, their catalogue, limits
//! and prices, and the model of each task. API keys go from the frontend straight to the
//! Keychain; the frontend only ever learns whether a key is stored, never the key.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use trama_core::jobs::lock;
use trama_core::providers::{self, ClientConfig, Keychain, ProviderError, Secret, SecretStore};
use trama_core::storage::{
    Id, ModelLimits, ModelPrices, ModelTask, NewProvider, Provider, ProviderModel,
    check_provider_input,
};

use crate::jobs::Core;
use crate::settings::{INVALID_SETTING, blocking, storage_code};

/// A model of a provider, as the frontend sees it. Prices are per million tokens in
/// millionths of the currency unit.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPayload {
    pub model_id: String,
    pub listed: bool,
    pub catalog_context_tokens: Option<u32>,
    pub catalog_max_output_tokens: Option<u32>,
    pub context_tokens_override: Option<u32>,
    pub max_output_tokens_override: Option<u32>,
    pub input_price_micros: Option<u64>,
    pub output_price_micros: Option<u64>,
}

impl From<&ProviderModel> for ModelPayload {
    fn from(model: &ProviderModel) -> Self {
        Self {
            model_id: model.model_id.clone(),
            listed: model.listed,
            catalog_context_tokens: model.catalog.context_tokens,
            catalog_max_output_tokens: model.catalog.max_output_tokens,
            context_tokens_override: model.overrides.context_tokens,
            max_output_tokens_override: model.overrides.max_output_tokens,
            input_price_micros: model.prices.input_micros,
            output_price_micros: model.prices.output_micros,
        }
    }
}

/// A provider, as the frontend sees it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPayload {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub needs_key: bool,
    /// Whether the Keychain holds its key. The key itself never leaves the core.
    pub has_key: bool,
    pub catalog_fetched_at: Option<i64>,
    pub models: Vec<ModelPayload>,
}

/// The model chosen for a task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskModelPayload {
    pub task: String,
    pub provider_id: String,
    pub model_id: String,
}

/// A provider to add or change. `api_key` is set only when the user typed one.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub needs_key: bool,
    #[serde(default)]
    pub api_key: Option<String>,
}

// Hand-written, so the key can never reach a log through `{:?}`.
impl std::fmt::Debug for ProviderInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderInput")
            .field("name", &self.name)
            .field("base_url", &self.base_url)
            .field("needs_key", &self.needs_key)
            .field("has_api_key", &self.api_key.is_some())
            .finish()
    }
}

impl ProviderInput {
    /// The key typed by the user, if any.
    fn secret(&self) -> Option<Secret> {
        self.api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(Secret::new)
    }
}

/// Limits and prices the user set for a model; `None` clears one.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettingsInput {
    pub provider_id: String,
    pub model_id: String,
    pub context_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub input_price_micros: Option<u64>,
    pub output_price_micros: Option<u64>,
}

/// A provider error as a code; the detail (never the key) goes to the log.
fn provider_code(err: ProviderError) -> String {
    eprintln!("trama: provider operation failed: {err}");
    err.code().to_owned()
}

fn parse_id(id: &str) -> Result<Id, String> {
    Id::parse_str(id).map_err(|_| INVALID_SETTING.to_owned())
}

fn parse_task(task: &str) -> Result<ModelTask, String> {
    task.parse().map_err(|_| INVALID_SETTING.to_owned())
}

fn has_key(provider: &Provider) -> bool {
    provider.key_ref.as_deref().is_some_and(|service| {
        Keychain
            .get(service)
            .inspect_err(|err| eprintln!("trama: cannot read a provider key: {err}"))
            .is_ok_and(|key| key.is_some())
    })
}

fn provider_payload(app: &AppHandle, provider: &Provider) -> Result<ProviderPayload, String> {
    let models = {
        let core = app.state::<Core>();
        let library = lock(&core.library);
        library.provider_models(provider.id).map_err(storage_code)?
    };
    Ok(ProviderPayload {
        id: provider.id.to_string(),
        name: provider.name.clone(),
        base_url: provider.base_url.clone(),
        needs_key: provider.key_ref.is_some(),
        has_key: has_key(provider),
        catalog_fetched_at: provider.catalog_fetched_at,
        models: models.iter().map(ModelPayload::from).collect(),
    })
}

/// Every provider with its models.
#[tauri::command]
pub async fn list_providers(app: AppHandle) -> Result<Vec<ProviderPayload>, String> {
    blocking(app, |app| {
        let providers = {
            let core = app.state::<Core>();
            let library = lock(&core.library);
            library.providers().map_err(storage_code)?
        };
        providers
            .iter()
            .map(|provider| provider_payload(app, provider))
            .collect()
    })
    .await
}

/// Adds a provider; its key, if typed, goes to the Keychain.
#[tauri::command]
pub async fn add_provider(app: AppHandle, input: ProviderInput) -> Result<ProviderPayload, String> {
    blocking(app, move |app| {
        if let Some(code) = check_provider_input(&input.name, &input.base_url) {
            return Err(code.to_owned());
        }
        let provider = {
            let core = app.state::<Core>();
            let mut library = lock(&core.library);
            let new = NewProvider {
                name: input.name.clone(),
                base_url: input.base_url.clone(),
                needs_key: input.needs_key,
            };
            providers::add_provider(&mut library, &Keychain, &new, input.secret().as_ref())
                .map_err(provider_code)?
        };
        provider_payload(app, &provider)
    })
    .await
}

/// Renames a provider, changes its URL and, if one is typed, replaces its key.
#[tauri::command]
pub async fn update_provider(
    app: AppHandle,
    id: String,
    input: ProviderInput,
) -> Result<ProviderPayload, String> {
    blocking(app, move |app| {
        let id = parse_id(&id)?;
        if let Some(code) = check_provider_input(&input.name, &input.base_url) {
            return Err(code.to_owned());
        }
        let provider = {
            let core = app.state::<Core>();
            let mut library = lock(&core.library);
            library
                .update_provider(id, &input.name, &input.base_url)
                .map_err(storage_code)?
        };
        if let Some(key) = input.secret() {
            providers::set_api_key(&Keychain, &provider, &key).map_err(provider_code)?;
        }
        provider_payload(app, &provider)
    })
    .await
}

/// Removes a provider, its models, its task choices and its Keychain item.
#[tauri::command]
pub async fn remove_provider(app: AppHandle, id: String) -> Result<(), String> {
    blocking(app, move |app| {
        let id = parse_id(&id)?;
        let core = app.state::<Core>();
        let mut library = lock(&core.library);
        providers::remove_provider(&mut library, &Keychain, id).map_err(provider_code)
    })
    .await
}

/// Tests the connection by reading the provider's model catalogue, and stores it.
#[tauri::command]
pub async fn test_provider(app: AppHandle, id: String) -> Result<ProviderPayload, String> {
    blocking(app, move |app| {
        let id = parse_id(&id)?;
        let library = app.state::<Core>().library.clone();
        // A quick answer matters more than retries while the user waits.
        let config = ClientConfig {
            timeout: std::time::Duration::from_secs(20),
            retry: providers::RetryPolicy {
                max_attempts: 1,
                ..providers::RetryPolicy::default()
            },
            ..ClientConfig::default()
        };
        providers::refresh_catalog(&library, &Keychain, id, config).map_err(provider_code)?;
        let provider = lock(&library).provider(id).map_err(storage_code)?;
        provider_payload(app, &provider)
    })
    .await
}

/// Sets the limits and prices of a model, adding it if the catalogue doesn't list it.
#[tauri::command]
pub async fn set_model_settings(
    app: AppHandle,
    input: ModelSettingsInput,
) -> Result<ModelPayload, String> {
    blocking(app, move |app| {
        let provider_id = parse_id(&input.provider_id)?;
        let core = app.state::<Core>();
        let mut library = lock(&core.library);
        library
            .set_model_overrides(
                provider_id,
                &input.model_id,
                ModelLimits {
                    context_tokens: input.context_tokens,
                    max_output_tokens: input.max_output_tokens,
                },
            )
            .map_err(storage_code)?;
        let model = library
            .set_model_prices(
                provider_id,
                &input.model_id,
                ModelPrices {
                    input_micros: input.input_price_micros,
                    output_micros: input.output_price_micros,
                },
            )
            .map_err(storage_code)?;
        Ok(ModelPayload::from(&model))
    })
    .await
}

/// The model chosen for each task that has one.
#[tauri::command]
pub async fn task_models(app: AppHandle) -> Result<Vec<TaskModelPayload>, String> {
    blocking(app, |app| {
        let core = app.state::<Core>();
        let choices = lock(&core.library).task_models().map_err(storage_code)?;
        Ok(choices
            .into_iter()
            .map(|choice| TaskModelPayload {
                task: choice.task.as_str().to_owned(),
                provider_id: choice.provider_id.to_string(),
                model_id: choice.model_id,
            })
            .collect())
    })
    .await
}

/// Chooses the model of a task, or leaves the task without one.
#[tauri::command]
pub async fn set_task_model(
    app: AppHandle,
    task: String,
    choice: Option<TaskModelPayload>,
) -> Result<(), String> {
    blocking(app, move |app| {
        let task = parse_task(&task)?;
        let core = app.state::<Core>();
        let mut library = lock(&core.library);
        match choice {
            Some(choice) => library
                .set_task_model(task, parse_id(&choice.provider_id)?, &choice.model_id)
                .map(drop),
            None => library.clear_task_model(task),
        }
        .map_err(storage_code)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_typed_key_never_shows_in_debug_output() {
        let input = ProviderInput {
            name: "Work".into(),
            base_url: "https://api.example.com/v1".into(),
            needs_key: true,
            api_key: Some("sk-secret-value".into()),
        };
        let debug = format!("{input:?}");
        assert!(!debug.contains("sk-secret-value"), "{debug}");
        assert!(debug.contains("has_api_key: true"));
    }

    #[test]
    fn a_blank_key_is_no_key() {
        let input = ProviderInput {
            name: "Work".into(),
            base_url: "https://api.example.com/v1".into(),
            needs_key: true,
            api_key: Some("   ".into()),
        };
        assert!(input.secret().is_none());
    }
}
