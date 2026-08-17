//! Where account credentials are kept at rest.
//!
//! The whole snapshot is stored as one keychain entry per account. That keeps
//! the secret and the identity it belongs to together, so a half-written
//! account cannot occur.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::claude::snapshot::AccountSnapshot;
use crate::error::{Error, Result};

/// Identifies byte's entries within the OS credential store.
const SERVICE: &str = "byte-claude-account-switcher";

pub trait SecretStore: Send + Sync {
    fn put(&self, uuid: &str, snapshot: &AccountSnapshot) -> Result<()>;
    fn get(&self, uuid: &str) -> Result<Option<AccountSnapshot>>;
    fn delete(&self, uuid: &str) -> Result<()>;
}

/// The OS credential store: Windows Credential Manager, macOS Keychain, or
/// the Secret Service on Linux.
#[derive(Debug, Default)]
pub struct KeyringStore;

impl KeyringStore {
    pub fn new() -> Self {
        Self
    }

    fn entry(uuid: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, uuid).map_err(|e| Error::Secret(e.to_string()))
    }
}

impl SecretStore for KeyringStore {
    fn put(&self, uuid: &str, snapshot: &AccountSnapshot) -> Result<()> {
        let payload = serde_json::to_string(snapshot).map_err(|e| Error::Secret(e.to_string()))?;
        Self::entry(uuid)?
            .set_password(&payload)
            .map_err(|e| Error::Secret(e.to_string()))
    }

    fn get(&self, uuid: &str) -> Result<Option<AccountSnapshot>> {
        match Self::entry(uuid)?.get_password() {
            Ok(payload) => {
                let snap = serde_json::from_str(&payload)
                    .map_err(|e| Error::Secret(format!("stored snapshot is corrupt: {e}")))?;
                Ok(Some(snap))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::Secret(e.to_string())),
        }
    }

    fn delete(&self, uuid: &str) -> Result<()> {
        match Self::entry(uuid)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Error::Secret(e.to_string())),
        }
    }
}

/// An in-process store used by tests, so the suite never touches a real
/// keychain or blocks on an unlock prompt.
#[derive(Debug, Default)]
pub struct MemoryStore {
    inner: Mutex<HashMap<String, AccountSnapshot>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemoryStore {
    fn put(&self, uuid: &str, snapshot: &AccountSnapshot) -> Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| Error::Secret("memory store poisoned".into()))?;
        guard.insert(uuid.to_string(), snapshot.clone());
        Ok(())
    }

    fn get(&self, uuid: &str) -> Result<Option<AccountSnapshot>> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| Error::Secret("memory store poisoned".into()))?;
        Ok(guard.get(uuid).cloned())
    }

    fn delete(&self, uuid: &str) -> Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| Error::Secret("memory store poisoned".into()))?;
        guard.remove(uuid);
        Ok(())
    }
}
