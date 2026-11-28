//! API keys, kept in the macOS Keychain and never in the database or in logs.
//!
//! Each provider that needs a key has one generic password item: service
//! `trama-provider-<provider id>` ([`crate::storage::PROVIDER_KEY_SERVICE_PREFIX`]),
//! account [`KEY_ACCOUNT`]. The database stores only the service name.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, PoisonError};

use super::error::{ErrorKind, ProviderError};

/// Account of the Keychain items that hold API keys.
pub const KEY_ACCOUNT: &str = "api-key";

/// An API key. Its `Debug` form hides the value, so it never reaches a log by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The key, to put in a request header. Never log it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Where API keys are kept. The app uses [`Keychain`]; tests use [`MemorySecrets`].
pub trait SecretStore: Send + Sync {
    /// The key stored under `service`, if any.
    fn get(&self, service: &str) -> Result<Option<Secret>, ProviderError>;
    /// Stores or replaces the key under `service`.
    fn set(&self, service: &str, secret: &Secret) -> Result<(), ProviderError>;
    /// Removes the key under `service`; removing a missing key is not an error.
    fn delete(&self, service: &str) -> Result<(), ProviderError>;
}

/// The macOS Keychain (login keychain of the current user).
#[derive(Debug, Clone, Copy, Default)]
pub struct Keychain;

#[cfg(target_os = "macos")]
mod keychain {
    use security_framework::passwords;

    use super::*;

    /// `errSecItemNotFound`.
    const ITEM_NOT_FOUND: i32 = -25300;

    fn keychain_error(action: &str, err: security_framework::base::Error) -> ProviderError {
        // The OS message names the failure, never the secret.
        ProviderError::new(
            ErrorKind::Keychain,
            format!("{action} failed: {} ({})", err, err.code()),
        )
    }

    impl SecretStore for Keychain {
        fn get(&self, service: &str) -> Result<Option<Secret>, ProviderError> {
            match passwords::get_generic_password(service, KEY_ACCOUNT) {
                Ok(bytes) => String::from_utf8(bytes)
                    .map(|s| Some(Secret(s)))
                    .map_err(|_| {
                        ProviderError::new(ErrorKind::Keychain, "stored key is not valid UTF-8")
                    }),
                Err(err) if err.code() == ITEM_NOT_FOUND => Ok(None),
                Err(err) => Err(keychain_error("reading the key", err)),
            }
        }

        fn set(&self, service: &str, secret: &Secret) -> Result<(), ProviderError> {
            passwords::set_generic_password(service, KEY_ACCOUNT, secret.expose().as_bytes())
                .map_err(|err| keychain_error("storing the key", err))
        }

        fn delete(&self, service: &str) -> Result<(), ProviderError> {
            match passwords::delete_generic_password(service, KEY_ACCOUNT) {
                Ok(()) => Ok(()),
                Err(err) if err.code() == ITEM_NOT_FOUND => Ok(()),
                Err(err) => Err(keychain_error("removing the key", err)),
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl SecretStore for Keychain {
    fn get(&self, _service: &str) -> Result<Option<Secret>, ProviderError> {
        Err(unsupported())
    }

    fn set(&self, _service: &str, _secret: &Secret) -> Result<(), ProviderError> {
        Err(unsupported())
    }

    fn delete(&self, _service: &str) -> Result<(), ProviderError> {
        Err(unsupported())
    }
}

#[cfg(not(target_os = "macos"))]
fn unsupported() -> ProviderError {
    ProviderError::new(
        ErrorKind::Keychain,
        "no credential store on this platform yet",
    )
}

/// Keys kept in memory, for tests.
#[derive(Debug, Default)]
pub struct MemorySecrets(Mutex<HashMap<String, Secret>>);

impl MemorySecrets {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of keys stored.
    pub fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl SecretStore for MemorySecrets {
    fn get(&self, service: &str) -> Result<Option<Secret>, ProviderError> {
        let keys = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(keys.get(service).cloned())
    }

    fn set(&self, service: &str, secret: &Secret) -> Result<(), ProviderError> {
        let mut keys = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        keys.insert(service.to_owned(), secret.clone());
        Ok(())
    }

    fn delete(&self, service: &str) -> Result<(), ProviderError> {
        let mut keys = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        keys.remove(service);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_the_key() {
        let secret = Secret::new("sk-very-secret");
        assert!(!format!("{secret:?}").contains("very-secret"));
    }

    #[test]
    fn memory_store_round_trips() {
        let store = MemorySecrets::new();
        assert_eq!(store.get("a").unwrap(), None);
        store.set("a", &Secret::new("one")).unwrap();
        store.set("a", &Secret::new("two")).unwrap();
        assert_eq!(store.get("a").unwrap(), Some(Secret::new("two")));
        store.delete("a").unwrap();
        store.delete("a").unwrap();
        assert!(store.is_empty());
    }

    /// Writes, reads and removes a throwaway item in the real Keychain. Ignored by
    /// default: it may show a Keychain prompt. Run with
    /// `cargo test -p trama-core keychain_round_trip -- --ignored`.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "touches the real Keychain"]
    fn keychain_round_trip() {
        let service = format!("trama-provider-test-{}", uuid::Uuid::now_v7());
        let keychain = Keychain;
        assert_eq!(keychain.get(&service).unwrap(), None);
        keychain.set(&service, &Secret::new("first")).unwrap();
        keychain.set(&service, &Secret::new("second")).unwrap();
        assert_eq!(keychain.get(&service).unwrap(), Some(Secret::new("second")));
        keychain.delete(&service).unwrap();
        assert_eq!(keychain.get(&service).unwrap(), None);
        keychain.delete(&service).unwrap();
    }
}
