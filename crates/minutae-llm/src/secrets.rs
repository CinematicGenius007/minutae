//! API keys live only in the OS keychain (macOS Keychain, Windows Credential Manager,
//! Secret Service on Linux). Minutae never reads other apps' credentials.

use std::collections::HashMap;
use std::sync::Mutex;

use minutae_core::FailureKind;

use crate::types::LlmError;

pub const KEYCHAIN_SERVICE: &str = "com.nebula.minutae";

pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>, LlmError>;
    fn set(&self, account: &str, secret: &str) -> Result<(), LlmError>;
    fn delete(&self, account: &str) -> Result<(), LlmError>;
}

/// OS keychain via the `keyring` crate, service `com.nebula.minutae`.
#[derive(Debug, Clone)]
pub struct KeyringStore {
    service: String,
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self {
            service: KEYCHAIN_SERVICE.into(),
        }
    }
}

impl KeyringStore {
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
    fn entry(&self, account: &str) -> Result<keyring::Entry, LlmError> {
        keyring::Entry::new(&self.service, account).map_err(map_err)
    }
}

fn map_err(e: keyring::Error) -> LlmError {
    match e {
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => LlmError::new(
            FailureKind::PermissionDenied,
            "The system keychain could not be opened.",
        ),
        other => LlmError::new(FailureKind::Internal, format!("keychain: {other}")),
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<String>, LlmError> {
        match self.entry(account)?.get_password() {
            Ok(s) if s.trim().is_empty() => Ok(None),
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_err(e)),
        }
    }
    fn set(&self, account: &str, secret: &str) -> Result<(), LlmError> {
        self.entry(account)?
            .set_password(secret.trim())
            .map_err(map_err)
    }
    fn delete(&self, account: &str) -> Result<(), LlmError> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_err(e)),
        }
    }
}

/// In-memory store for tests and previews.
#[derive(Debug, Default)]
pub struct MemorySecretStore {
    map: Mutex<HashMap<String, String>>,
}

impl MemorySecretStore {
    pub fn with(entries: &[(&str, &str)]) -> Self {
        Self {
            map: Mutex::new(
                entries
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            ),
        }
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, account: &str) -> Result<Option<String>, LlmError> {
        Ok(self.map.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &str) -> Result<(), LlmError> {
        self.map
            .lock()
            .unwrap()
            .insert(account.into(), secret.into());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), LlmError> {
        self.map.lock().unwrap().remove(account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_roundtrip() {
        let s = MemorySecretStore::default();
        assert_eq!(s.get("a").unwrap(), None);
        s.set("a", "k").unwrap();
        assert_eq!(s.get("a").unwrap().as_deref(), Some("k"));
        s.delete("a").unwrap();
        assert_eq!(s.get("a").unwrap(), None);
    }
}
