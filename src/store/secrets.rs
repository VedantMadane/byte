//! Where account credentials are kept at rest.
//!
//! Only the secret `oauth` block (`claudeAiOauth`, holding `accessToken` and
//! `refreshToken`) is stored here, one keychain entry per account. The
//! non-secret rest of a snapshot -- the `oauthAccount` object and `userID`
//! -- lives in `accounts.json` (`store::metadata`) instead.
//!
//! This split exists because Windows Credential Manager will not hold the
//! whole snapshot: `CRED_MAX_CREDENTIAL_BLOB_SIZE` is 2560 *bytes*, but
//! `windows-native-keyring-store` encodes the payload as UTF-16 before
//! measuring it against that limit, which halves the real per-entry budget
//! to 1280 *characters* -- and a realistic full snapshot (a 19-field
//! `oauthAccount` plus real tokens) measured at 1281, one over. Storing only
//! `oauth` keeps every real-world payload comfortably under that ceiling
//! (see the size-guard test in `tests/secrets_test.rs`).
//!
//! Storing the two halves separately means they are no longer written
//! atomically together, so they can disagree -- a keychain entry deleted out
//! from under byte, or `accounts.json` hand-edited. That is intentionally
//! not this module's problem to catch: `AccountSnapshot::reassemble` and the
//! `validate()` call after it (see `Switcher::load_snapshot` in
//! `ops::switch`) are what refuse a half-reassembled snapshot before it ever
//! reaches `apply()`.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::Value;

use crate::error::{Error, Result};

/// Identifies byte's entries within the OS credential store.
const SERVICE: &str = "byte-claude-account-switcher";

pub trait SecretStore: Send + Sync {
    /// `oauth` is the `claudeAiOauth` object alone -- never a whole
    /// `AccountSnapshot`. See the module doc comment for why.
    fn put(&self, uuid: &str, oauth: &Value) -> Result<()>;
    fn get(&self, uuid: &str) -> Result<Option<Value>>;
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

    /// Build the exact string `put` sends to the OS credential store: the
    /// `oauth` block alone, JSON-encoded. `pub`, like `cli::run::switch_json`
    /// and `cli::run::resolve_add_failure`, specifically so
    /// `tests/secrets_test.rs` can pin its size against the real Windows
    /// character budget described in the module doc comment, without
    /// writing to a real keychain to do it.
    pub fn serialize_payload(oauth: &Value) -> Result<String> {
        serde_json::to_string(oauth).map_err(|e| Error::Secret(e.to_string()))
    }
}

impl SecretStore for KeyringStore {
    fn put(&self, uuid: &str, oauth: &Value) -> Result<()> {
        let payload = Self::serialize_payload(oauth)?;
        Self::entry(uuid)?
            .set_password(&payload)
            .map_err(|e| Error::Secret(e.to_string()))
    }

    fn get(&self, uuid: &str) -> Result<Option<Value>> {
        match Self::entry(uuid)?.get_password() {
            Ok(payload) => {
                let oauth = serde_json::from_str(&payload)
                    .map_err(|e| Error::Secret(format!("stored credentials are corrupt: {e}")))?;
                Ok(Some(oauth))
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
///
/// Deliberately has no size limit of its own -- it is a generic test double
/// for `SecretStore`, not a Windows Credential Manager simulator, and
/// hard-coding that platform's limit here would be both wrong on other
/// platforms and misleading about which type owns the real constraint. The
/// size guard that matters lives in `tests/secrets_test.rs`, against
/// `KeyringStore::serialize_payload` directly.
#[derive(Debug, Default)]
pub struct MemoryStore {
    inner: Mutex<HashMap<String, Value>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemoryStore {
    fn put(&self, uuid: &str, oauth: &Value) -> Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| Error::Secret("memory store poisoned".into()))?;
        guard.insert(uuid.to_string(), oauth.clone());
        Ok(())
    }

    fn get(&self, uuid: &str) -> Result<Option<Value>> {
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
