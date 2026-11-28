//! Errors of provider calls, as stable codes the interface translates.

use std::time::Duration;

use crate::jobs::JobError;
use crate::storage::StorageError;

/// Stable error codes, translated in `locales/<locale>/providers.ftl` as
/// `provider-error-<code>` (with `_` written as `-`).
pub mod error_code {
    /// No reply within the timeout.
    pub const TIMEOUT: &str = "timeout";
    /// The provider could not be reached (DNS, connection, TLS).
    pub const NETWORK: &str = "network";
    /// Too many requests (HTTP 429) and the provider asked to wait longer than allowed.
    pub const RATE_LIMITED: &str = "rate_limited";
    /// The API key was refused (HTTP 401 or 403).
    pub const AUTH_FAILED: &str = "auth_failed";
    /// The endpoint or the model does not exist (HTTP 404).
    pub const NOT_FOUND: &str = "not_found";
    /// The provider refused the request as invalid (HTTP 400, 413, 422).
    pub const BAD_REQUEST: &str = "bad_request";
    /// The provider failed (HTTP 5xx) on every attempt.
    pub const SERVER_ERROR: &str = "server_error";
    /// The reply was not the JSON the API promises.
    pub const INVALID_RESPONSE: &str = "invalid_response";
    /// The reply stopped at the output limit (`finish_reason = length`).
    pub const TRUNCATED: &str = "truncated";
    /// The provider's content filter stopped the reply.
    pub const CONTENT_FILTERED: &str = "content_filtered";
    /// The reply had no text.
    pub const EMPTY_REPLY: &str = "empty_reply";
    /// A structured reply was not valid JSON.
    pub const INVALID_JSON: &str = "invalid_json";
    /// A structured reply did not match its schema.
    pub const SCHEMA_MISMATCH: &str = "schema_mismatch";
    /// A structured reply matched its schema but was empty or placeholder text.
    pub const DEGENERATE_OUTPUT: &str = "degenerate_output";
    /// The prompt does not fit the model's context window.
    pub const CONTEXT_EXCEEDED: &str = "context_exceeded";
    /// The model's context window is unknown: the catalogue doesn't say and the user set
    /// no override.
    pub const LIMITS_UNKNOWN: &str = "limits_unknown";
    /// No model is chosen for the task.
    pub const NO_MODEL_FOR_TASK: &str = "no_model_for_task";
    /// The provider needs an API key and none is stored.
    pub const MISSING_KEY: &str = "missing_key";
    /// The Keychain refused to read or write the key.
    pub const KEYCHAIN: &str = "keychain";
    /// The library could not be read or written.
    pub const STORAGE: &str = "storage";
    /// The month's spend reached the budget and the user chose to refuse calls past it.
    pub const BUDGET_EXCEEDED: &str = "budget_exceeded";
    /// The provider's URL moved to another origin: its stored key is never sent there, so
    /// a new key must come with the new URL.
    pub const KEY_REQUIRED_FOR_NEW_URL: &str = "key_required_for_new_url";
    /// The user enforces a monthly budget and the model has no price, so its cost could
    /// not be counted against the budget.
    pub const PRICE_MISSING: &str = "price_missing";

    /// Every code, for the translation tests.
    pub const ALL: &[&str] = &[
        TIMEOUT,
        NETWORK,
        RATE_LIMITED,
        AUTH_FAILED,
        NOT_FOUND,
        BAD_REQUEST,
        SERVER_ERROR,
        INVALID_RESPONSE,
        TRUNCATED,
        CONTENT_FILTERED,
        EMPTY_REPLY,
        INVALID_JSON,
        SCHEMA_MISMATCH,
        DEGENERATE_OUTPUT,
        CONTEXT_EXCEEDED,
        LIMITS_UNKNOWN,
        NO_MODEL_FOR_TASK,
        MISSING_KEY,
        KEYCHAIN,
        STORAGE,
        BUDGET_EXCEEDED,
        KEY_REQUIRED_FOR_NEW_URL,
        PRICE_MISSING,
    ];
}

/// What went wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    Timeout,
    Network,
    /// HTTP 429; `retry_after` is what the provider asked for, if it said.
    RateLimited {
        retry_after: Option<Duration>,
    },
    AuthFailed,
    NotFound,
    BadRequest,
    Server {
        status: u16,
    },
    InvalidResponse,
    Truncated,
    ContentFiltered,
    EmptyReply,
    InvalidJson,
    SchemaMismatch,
    /// Paths of the fields that were empty or placeholders, for example `items[0].text`.
    Degenerate {
        fields: Vec<String>,
    },
    ContextExceeded,
    LimitsUnknown,
    NoModelForTask,
    MissingKey,
    Keychain,
    Storage,
    BudgetExceeded,
    KeyRequiredForNewUrl,
    PriceMissing,
}

/// A failed provider call (principle A4: surfaced with a code, never silent).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {detail}", code = self.code())]
pub struct ProviderError {
    pub kind: ErrorKind,
    /// Technical detail for diagnostics; never contains the API key.
    pub detail: String,
}

impl ProviderError {
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    /// The stable code, see [`error_code`].
    pub fn code(&self) -> &'static str {
        use error_code::*;
        match self.kind {
            ErrorKind::Timeout => TIMEOUT,
            ErrorKind::Network => NETWORK,
            ErrorKind::RateLimited { .. } => RATE_LIMITED,
            ErrorKind::AuthFailed => AUTH_FAILED,
            ErrorKind::NotFound => NOT_FOUND,
            ErrorKind::BadRequest => BAD_REQUEST,
            ErrorKind::Server { .. } => SERVER_ERROR,
            ErrorKind::InvalidResponse => INVALID_RESPONSE,
            ErrorKind::Truncated => TRUNCATED,
            ErrorKind::ContentFiltered => CONTENT_FILTERED,
            ErrorKind::EmptyReply => EMPTY_REPLY,
            ErrorKind::InvalidJson => INVALID_JSON,
            ErrorKind::SchemaMismatch => SCHEMA_MISMATCH,
            ErrorKind::Degenerate { .. } => DEGENERATE_OUTPUT,
            ErrorKind::ContextExceeded => CONTEXT_EXCEEDED,
            ErrorKind::LimitsUnknown => LIMITS_UNKNOWN,
            ErrorKind::NoModelForTask => NO_MODEL_FOR_TASK,
            ErrorKind::MissingKey => MISSING_KEY,
            ErrorKind::Keychain => KEYCHAIN,
            ErrorKind::Storage => STORAGE,
            ErrorKind::BudgetExceeded => BUDGET_EXCEEDED,
            ErrorKind::KeyRequiredForNewUrl => KEY_REQUIRED_FOR_NEW_URL,
            ErrorKind::PriceMissing => PRICE_MISSING,
        }
    }

    /// Whether trying again later may succeed. Errors that need the user (a wrong key, a
    /// missing model, a prompt too long) are not.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::Timeout
                | ErrorKind::Network
                | ErrorKind::RateLimited { .. }
                | ErrorKind::Server { .. }
                | ErrorKind::InvalidResponse
                | ErrorKind::EmptyReply
                | ErrorKind::InvalidJson
                | ErrorKind::SchemaMismatch
                | ErrorKind::Degenerate { .. }
                | ErrorKind::Storage
        )
    }

    /// Whether the client retries the same request at once (after a backoff).
    pub(crate) fn is_transient(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::Timeout
                | ErrorKind::Network
                | ErrorKind::RateLimited { .. }
                | ErrorKind::Server { .. }
        )
    }

    /// Whether a structured call asks again for a better reply.
    pub(crate) fn is_bad_content(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::EmptyReply
                | ErrorKind::InvalidJson
                | ErrorKind::SchemaMismatch
                | ErrorKind::Degenerate { .. }
        )
    }
}

impl From<StorageError> for ProviderError {
    fn from(err: StorageError) -> Self {
        Self::new(ErrorKind::Storage, err.to_string())
    }
}

impl From<ProviderError> for JobError {
    fn from(err: ProviderError) -> Self {
        if err.is_retryable() {
            JobError::retryable(err.code(), err.detail)
        } else {
            JobError::permanent(err.code(), err.detail)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_listed() {
        let mut codes = error_code::ALL.to_vec();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), error_code::ALL.len());
        let degenerate = ProviderError::new(ErrorKind::Degenerate { fields: vec![] }, "");
        assert!(error_code::ALL.contains(&degenerate.code()));
    }

    #[test]
    fn job_errors_keep_the_code_and_retryability() {
        let job: JobError = ProviderError::new(ErrorKind::Timeout, "slow").into();
        assert_eq!(job.code, "timeout");
        assert!(job.retryable);
        let job: JobError = ProviderError::new(ErrorKind::AuthFailed, "401").into();
        assert_eq!(job.code, "auth_failed");
        assert!(!job.retryable);
    }
}
