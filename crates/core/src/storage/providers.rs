//! AI providers, their model catalogue and the model chosen for each task.
//!
//! Only configuration lives here. API keys are kept in the macOS Keychain: a provider
//! stores the name of its Keychain item ([`Provider::key_ref`]), never the key. The HTTP
//! client and the Keychain access live in [`crate::providers`].

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{Id, Library, Result, StorageError, Timestamp, enum_column, id_column, new_id};

/// Prefix of the Keychain service that holds a provider's API key; the provider id follows.
pub const PROVIDER_KEY_SERVICE_PREFIX: &str = "trama-provider-";

text_enum! {
    /// Work that needs a language model; each task uses the model the user chose for it.
    /// The curator's tasks (phase 2) are added here.
    ModelTask {
        /// Title, summary and key points of a source.
        Summary = "summary",
    }
}

impl ModelTask {
    /// Every task, in the order the interface lists them.
    pub const ALL: &[ModelTask] = &[ModelTask::Summary];
}

/// An OpenAI-compatible endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    pub id: Id,
    /// Chosen by the user.
    pub name: String,
    /// Base URL of the API, without a trailing slash, for example `https://api.example.com/v1`.
    pub base_url: String,
    /// Keychain service that holds the API key; `None` for providers that need no key
    /// (a local server).
    pub key_ref: Option<String>,
    /// Last successful read of the model catalogue.
    pub catalog_fetched_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// A provider to add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProvider {
    pub name: String,
    pub base_url: String,
    /// Whether the provider needs an API key, kept in the Keychain.
    pub needs_key: bool,
}

/// Token limits of a model. Never hard-coded (principle A4): they come from the
/// provider's catalogue or from the user.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModelLimits {
    /// Context window: prompt and reply together.
    pub context_tokens: Option<u32>,
    /// Longest reply.
    pub max_output_tokens: Option<u32>,
}

impl ModelLimits {
    /// Each limit of `self`, or of `fallback` where `self` has none.
    pub fn or(self, fallback: ModelLimits) -> ModelLimits {
        ModelLimits {
            context_tokens: self.context_tokens.or(fallback.context_tokens),
            max_output_tokens: self.max_output_tokens.or(fallback.max_output_tokens),
        }
    }
}

/// A model offered by a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModel {
    pub provider_id: Id,
    /// The model id as the provider names it.
    pub model_id: String,
    /// Listed by the provider at the last catalogue read. A model that disappears is
    /// kept, with the user's overrides, so a task that uses it fails with a clear error.
    pub listed: bool,
    /// Limits read from the catalogue.
    pub catalog: ModelLimits,
    /// Limits set by the user, for providers that don't expose them or expose wrong ones.
    pub overrides: ModelLimits,
    pub updated_at: Timestamp,
}

impl ProviderModel {
    /// The limits in effect: the user's overrides, then the catalogue.
    pub fn limits(&self) -> ModelLimits {
        self.overrides.or(self.catalog)
    }
}

/// One model read from a provider's catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub model_id: String,
    pub limits: ModelLimits,
}

/// The model chosen for a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskModel {
    pub task: ModelTask,
    pub provider_id: Id,
    pub model_id: String,
    pub updated_at: Timestamp,
}

const PROVIDER_COLUMNS: &str =
    "id, name, base_url, key_ref, catalog_fetched_at, created_at, updated_at";

const MODEL_COLUMNS: &str = "provider_id, model_id, listed, context_tokens, max_output_tokens, \
     context_tokens_override, max_output_tokens_override, updated_at";

fn provider_row(row: &Row<'_>) -> rusqlite::Result<Provider> {
    Ok(Provider {
        id: id_column(row, 0)?,
        name: row.get(1)?,
        base_url: row.get(2)?,
        key_ref: row.get(3)?,
        catalog_fetched_at: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn model_row(row: &Row<'_>) -> rusqlite::Result<ProviderModel> {
    Ok(ProviderModel {
        provider_id: id_column(row, 0)?,
        model_id: row.get(1)?,
        listed: row.get(2)?,
        catalog: ModelLimits {
            context_tokens: row.get(3)?,
            max_output_tokens: row.get(4)?,
        },
        overrides: ModelLimits {
            context_tokens: row.get(5)?,
            max_output_tokens: row.get(6)?,
        },
        updated_at: row.get(7)?,
    })
}

fn task_model_row(row: &Row<'_>) -> rusqlite::Result<TaskModel> {
    Ok(TaskModel {
        task: enum_column(row, 0)?,
        provider_id: id_column(row, 1)?,
        model_id: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

/// Trims a provider name and refuses an empty one.
fn clean_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StorageError::Invalid("a provider needs a name"));
    }
    Ok(name.to_owned())
}

/// Checks a base URL and removes trailing slashes. Plain `http://` would send the API key
/// and the user's text in clear, so it is accepted only for local servers (Ollama, LM
/// Studio, a gateway on the local network): loopback, private and link-local addresses,
/// `localhost` and `.local` names. Every other host needs `https://`.
fn clean_base_url(url: &str) -> Result<String> {
    let url = url.trim().trim_end_matches('/');
    let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    } else {
        return Err(StorageError::Invalid(
            "a provider URL must start with http:// or https://",
        ));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') || rest.chars().any(char::is_whitespace) {
        return Err(StorageError::Invalid("invalid provider URL"));
    }
    if !secure && !is_local_host(host_of(authority)) {
        return Err(StorageError::Invalid(
            "plain http:// is allowed only for local servers; use https://",
        ));
    }
    Ok(url.to_owned())
}

/// The host of an authority (`host:port`, `[v6]:port`), without brackets or port.
fn host_of(authority: &str) -> &str {
    if let Some(v6) = authority.strip_prefix('[') {
        return v6.split(']').next().unwrap_or_default();
    }
    authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host)
}

/// Whether a host is this machine or the local network.
fn is_local_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => {
            ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()
        }
        Err(_) => false,
    }
}

/// Refuses a limit of zero, which would make every call fail.
fn check_limits(limits: ModelLimits) -> Result<()> {
    if limits.context_tokens == Some(0) || limits.max_output_tokens == Some(0) {
        return Err(StorageError::Invalid("model limits must be positive"));
    }
    Ok(())
}

fn load_provider(conn: &Connection, id: Id) -> Result<Provider> {
    conn.query_row(
        &format!("SELECT {PROVIDER_COLUMNS} FROM provider WHERE id = ?1"),
        [id.to_string()],
        provider_row,
    )
    .optional()?
    .ok_or_else(|| StorageError::not_found("provider", id))
}

fn load_model(conn: &Connection, provider_id: Id, model_id: &str) -> Result<ProviderModel> {
    conn.query_row(
        &format!(
            "SELECT {MODEL_COLUMNS} FROM provider_model WHERE provider_id = ?1 AND model_id = ?2"
        ),
        params![provider_id.to_string(), model_id],
        model_row,
    )
    .optional()?
    .ok_or_else(|| StorageError::not_found("model", format!("{provider_id}/{model_id}")))
}

impl Library {
    /// Adds a provider. If it needs a key, the caller stores the key in the Keychain
    /// under [`Provider::key_ref`].
    pub fn add_provider(&mut self, new: &NewProvider) -> Result<Provider> {
        let name = clean_name(&new.name)?;
        let base_url = clean_base_url(&new.base_url)?;
        let id = new_id();
        let key_ref = new
            .needs_key
            .then(|| format!("{PROVIDER_KEY_SERVICE_PREFIX}{id}"));
        let now = self.now();
        self.conn.execute(
            "INSERT INTO provider (id, name, base_url, key_ref, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id.to_string(), name, base_url, key_ref, now],
        )?;
        load_provider(&self.conn, id)
    }

    /// A provider.
    pub fn provider(&self, id: Id) -> Result<Provider> {
        load_provider(&self.conn, id)
    }

    /// Every provider, in the order they were added.
    pub fn providers(&self) -> Result<Vec<Provider>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {PROVIDER_COLUMNS} FROM provider ORDER BY created_at, id"
        ))?;
        let rows = stmt.query_map([], provider_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Renames a provider or changes its URL. The model catalogue is kept: read it again
    /// if the URL now points elsewhere.
    pub fn update_provider(&mut self, id: Id, name: &str, base_url: &str) -> Result<Provider> {
        let name = clean_name(name)?;
        let base_url = clean_base_url(base_url)?;
        let now = self.now();
        let changed = self.conn.execute(
            "UPDATE provider SET name = ?2, base_url = ?3, updated_at = ?4 WHERE id = ?1",
            params![id.to_string(), name, base_url, now],
        )?;
        if changed == 0 {
            return Err(StorageError::not_found("provider", id));
        }
        load_provider(&self.conn, id)
    }

    /// Removes a provider with its models; tasks that used one of them are left without a
    /// model. Returns the removed provider, so the caller can delete its Keychain item.
    pub fn remove_provider(&mut self, id: Id) -> Result<Provider> {
        let tx = self.conn.transaction()?;
        let provider = load_provider(&tx, id)?;
        tx.execute("DELETE FROM provider WHERE id = ?1", [id.to_string()])?;
        tx.commit()?;
        Ok(provider)
    }

    /// Stores a catalogue just read from the provider. Listed models get the catalogue's
    /// limits; models no longer listed are kept, marked as not listed. The user's
    /// overrides are never touched.
    pub fn replace_catalog(&mut self, provider_id: Id, entries: &[CatalogEntry]) -> Result<()> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        load_provider(&tx, provider_id)?;
        let provider = provider_id.to_string();
        tx.execute(
            "UPDATE provider_model SET listed = 0, updated_at = ?2
             WHERE provider_id = ?1 AND listed = 1",
            params![provider, now],
        )?;
        {
            let mut upsert = tx.prepare(
                "INSERT INTO provider_model
                     (provider_id, model_id, listed, context_tokens, max_output_tokens, updated_at)
                 VALUES (?1, ?2, 1, ?3, ?4, ?5)
                 ON CONFLICT (provider_id, model_id) DO UPDATE SET
                     listed = 1,
                     context_tokens = excluded.context_tokens,
                     max_output_tokens = excluded.max_output_tokens,
                     updated_at = excluded.updated_at",
            )?;
            for entry in entries {
                if entry.model_id.trim().is_empty() {
                    return Err(StorageError::Invalid("a model needs an id"));
                }
                // A provider reporting 0 means "unknown", not a real limit.
                let limits = ModelLimits {
                    context_tokens: entry.limits.context_tokens.filter(|&n| n > 0),
                    max_output_tokens: entry.limits.max_output_tokens.filter(|&n| n > 0),
                };
                upsert.execute(params![
                    provider,
                    entry.model_id,
                    limits.context_tokens,
                    limits.max_output_tokens,
                    now
                ])?;
            }
        }
        tx.execute(
            "UPDATE provider SET catalog_fetched_at = ?2, updated_at = ?2 WHERE id = ?1",
            params![provider, now],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The models of a provider, listed ones first, then by id.
    pub fn provider_models(&self, provider_id: Id) -> Result<Vec<ProviderModel>> {
        load_provider(&self.conn, provider_id)?;
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {MODEL_COLUMNS} FROM provider_model WHERE provider_id = ?1
             ORDER BY listed DESC, model_id"
        ))?;
        let rows = stmt.query_map([provider_id.to_string()], model_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// One model of a provider.
    pub fn provider_model(&self, provider_id: Id, model_id: &str) -> Result<ProviderModel> {
        load_model(&self.conn, provider_id, model_id)
    }

    /// Sets the user's limits for a model (`None` clears one). A model the catalogue
    /// doesn't list is added, for providers without a usable catalogue.
    pub fn set_model_overrides(
        &mut self,
        provider_id: Id,
        model_id: &str,
        overrides: ModelLimits,
    ) -> Result<ProviderModel> {
        let model_id = model_id.trim();
        if model_id.is_empty() {
            return Err(StorageError::Invalid("a model needs an id"));
        }
        check_limits(overrides)?;
        let now = self.now();
        let tx = self.conn.transaction()?;
        load_provider(&tx, provider_id)?;
        tx.execute(
            "INSERT INTO provider_model (provider_id, model_id, listed,
                 context_tokens_override, max_output_tokens_override, updated_at)
             VALUES (?1, ?2, 0, ?3, ?4, ?5)
             ON CONFLICT (provider_id, model_id) DO UPDATE SET
                 context_tokens_override = excluded.context_tokens_override,
                 max_output_tokens_override = excluded.max_output_tokens_override,
                 updated_at = excluded.updated_at",
            params![
                provider_id.to_string(),
                model_id,
                overrides.context_tokens,
                overrides.max_output_tokens,
                now
            ],
        )?;
        let model = load_model(&tx, provider_id, model_id)?;
        tx.commit()?;
        Ok(model)
    }

    /// Chooses the model for a task. The model must be known (read from the catalogue or
    /// added with [`Library::set_model_overrides`]).
    pub fn set_task_model(
        &mut self,
        task: ModelTask,
        provider_id: Id,
        model_id: &str,
    ) -> Result<TaskModel> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        load_model(&tx, provider_id, model_id)?;
        tx.execute(
            "INSERT INTO task_model (task, provider_id, model_id, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (task) DO UPDATE SET
                 provider_id = excluded.provider_id,
                 model_id = excluded.model_id,
                 updated_at = excluded.updated_at",
            params![task.as_str(), provider_id.to_string(), model_id, now],
        )?;
        tx.commit()?;
        Ok(TaskModel {
            task,
            provider_id,
            model_id: model_id.to_owned(),
            updated_at: now,
        })
    }

    /// The model chosen for a task, if any.
    pub fn task_model(&self, task: ModelTask) -> Result<Option<TaskModel>> {
        Ok(self
            .conn
            .query_row(
                "SELECT task, provider_id, model_id, updated_at FROM task_model WHERE task = ?1",
                [task.as_str()],
                task_model_row,
            )
            .optional()?)
    }

    /// Every task that has a model.
    pub fn task_models(&self) -> Result<Vec<TaskModel>> {
        let mut stmt = self.conn.prepare(
            "SELECT task, provider_id, model_id, updated_at FROM task_model ORDER BY task",
        )?;
        let rows = stmt.query_map([], task_model_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Leaves a task without a model.
    pub fn clear_task_model(&mut self, task: ModelTask) -> Result<()> {
        self.conn
            .execute("DELETE FROM task_model WHERE task = ?1", [task.as_str()])?;
        Ok(())
    }

    /// The provider and model chosen for a task, if any.
    pub fn resolve_task_model(&self, task: ModelTask) -> Result<Option<(Provider, ProviderModel)>> {
        let Some(choice) = self.task_model(task)? else {
            return Ok(None);
        };
        let provider = load_provider(&self.conn, choice.provider_id)?;
        let model = load_model(&self.conn, choice.provider_id, &choice.model_id)?;
        Ok(Some((provider, model)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_urls_are_checked_and_trimmed() {
        assert_eq!(
            clean_base_url(" https://api.example.com/v1/ ").unwrap(),
            "https://api.example.com/v1"
        );
        assert_eq!(
            clean_base_url("http://127.0.0.1:1234/v1").unwrap(),
            "http://127.0.0.1:1234/v1"
        );
        assert!(clean_base_url("ftp://example.com").is_err());
        assert!(clean_base_url("https://").is_err());
        assert!(clean_base_url("https://exa mple.com").is_err());
        assert!(clean_base_url("example.com").is_err());
        assert!(clean_base_url("https://user@example.com").is_err());
    }

    #[test]
    fn plain_http_is_only_for_local_servers() {
        for url in [
            "http://localhost:11434/v1",
            "http://LOCALHOST/v1",
            "http://127.0.0.1:1234/v1",
            "http://[::1]:8080/v1",
            "http://192.168.1.20:8000/v1",
            "http://10.0.0.5/v1",
            "http://172.16.3.4/v1",
            "http://169.254.1.1/v1",
            "http://[fd00::1]/v1",
            "http://studio.local:1234/v1",
        ] {
            assert!(clean_base_url(url).is_ok(), "{url}");
        }
        for url in [
            "http://api.example.com/v1",
            "http://8.8.8.8/v1",
            "http://172.32.0.1/v1",
            "http://[2001:db8::1]/v1",
            "http://localhost.example.com/v1",
        ] {
            assert!(
                matches!(clean_base_url(url), Err(StorageError::Invalid(_))),
                "{url}"
            );
        }
        assert!(clean_base_url("https://api.example.com/v1").is_ok());
    }

    #[test]
    fn overrides_win_over_the_catalogue() {
        let model = ProviderModel {
            provider_id: new_id(),
            model_id: "m".into(),
            listed: true,
            catalog: ModelLimits {
                context_tokens: Some(100),
                max_output_tokens: Some(10),
            },
            overrides: ModelLimits {
                context_tokens: None,
                max_output_tokens: Some(20),
            },
            updated_at: 0,
        };
        assert_eq!(
            model.limits(),
            ModelLimits {
                context_tokens: Some(100),
                max_output_tokens: Some(20),
            }
        );
    }

    #[test]
    fn task_names_round_trip() {
        for task in ModelTask::ALL {
            assert_eq!(task.as_str().parse::<ModelTask>().unwrap(), *task);
        }
    }
}
