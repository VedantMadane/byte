//! Non-secret account metadata, stored as plain JSON.
//!
//! Kept separate from the secret store so that listing accounts — including
//! rendering the tray menu — never needs to unlock the OS keychain.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::atomic;
use crate::claude::snapshot::AccountSnapshot;
use crate::error::{Error, Result};

const METADATA_SCHEMA: u32 = 1;

/// Everything shown about an account without touching its secrets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountMeta {
    pub uuid: String,
    pub label: String,
    pub email: Option<String>,
    pub organization_name: Option<String>,
    pub subscription_type: Option<String>,
    pub added_at: String,
    pub last_used_at: Option<String>,
}

/// The contents of `accounts.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountsFile {
    pub schema: u32,
    pub active: Option<String>,
    pub accounts: Vec<AccountMeta>,
}

impl Default for AccountsFile {
    fn default() -> Self {
        Self {
            schema: METADATA_SCHEMA,
            active: None,
            accounts: Vec::new(),
        }
    }
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| String::from("unknown"))
}

impl AccountsFile {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => {
                return Err(Error::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };

        serde_json::from_str(&raw).map_err(|source| Error::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn save(&self, path: &Path, backup_dir: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(|source| Error::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        atomic::backup(path, backup_dir)?;
        atomic::write(path, format!("{text}\n").as_bytes())?;

        // Without this, accounts.json accumulates one backup per capture,
        // switch, rename, and remove, unbounded -- unlike .claude.json and
        // .credentials.json, which JsonDocument::save already prunes to the
        // same limit.
        if let Some(name) = path.file_name() {
            atomic::prune(backup_dir, &name.to_string_lossy(), 10)?;
        }
        Ok(())
    }

    /// Insert or refresh the entry for a snapshot's account, returning the
    /// stored metadata. A label the user set with `rename` is preserved.
    pub fn upsert_from(&mut self, snapshot: &AccountSnapshot) -> AccountMeta {
        let uuid = snapshot.identity().unwrap_or("unknown").to_string();

        let existing = self.accounts.iter().position(|a| a.uuid == uuid);

        let meta = AccountMeta {
            uuid: uuid.clone(),
            label: match existing {
                Some(i) => self.accounts[i].label.clone(),
                None => snapshot.default_label(),
            },
            email: snapshot.email().map(str::to_string),
            organization_name: snapshot.organization_name().map(str::to_string),
            subscription_type: snapshot.subscription_type().map(str::to_string),
            added_at: match existing {
                Some(i) => self.accounts[i].added_at.clone(),
                None => now_rfc3339(),
            },
            last_used_at: existing.and_then(|i| self.accounts[i].last_used_at.clone()),
        };

        match existing {
            Some(i) => self.accounts[i] = meta.clone(),
            None => self.accounts.push(meta.clone()),
        }
        meta
    }

    /// Find an account by label, email, or UUID prefix.
    pub fn resolve(&self, query: &str) -> Result<&AccountMeta> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Err(Error::NoSuchAccount(query.to_string()));
        }

        let exact: Vec<&AccountMeta> = self
            .accounts
            .iter()
            .filter(|a| {
                a.label.to_lowercase() == q
                    || a.email.as_deref().map(str::to_lowercase) == Some(q.clone())
                    || a.uuid.to_lowercase() == q
            })
            .collect();

        if exact.len() == 1 {
            return Ok(exact[0]);
        }
        if exact.len() > 1 {
            return Err(Error::AmbiguousAccount {
                query: query.to_string(),
                count: exact.len(),
            });
        }

        let prefixed: Vec<&AccountMeta> = self
            .accounts
            .iter()
            .filter(|a| {
                a.uuid.to_lowercase().starts_with(&q) || a.label.to_lowercase().starts_with(&q)
            })
            .collect();

        match prefixed.len() {
            1 => Ok(prefixed[0]),
            0 => Err(Error::NoSuchAccount(query.to_string())),
            n => Err(Error::AmbiguousAccount {
                query: query.to_string(),
                count: n,
            }),
        }
    }

    pub fn rename(&mut self, uuid: &str, label: &str) -> Result<AccountMeta> {
        let idx = self
            .accounts
            .iter()
            .position(|a| a.uuid == uuid)
            .ok_or_else(|| Error::NoSuchAccount(uuid.to_string()))?;
        self.accounts[idx].label = label.to_string();
        Ok(self.accounts[idx].clone())
    }

    pub fn remove(&mut self, uuid: &str) -> Result<AccountMeta> {
        let idx = self
            .accounts
            .iter()
            .position(|a| a.uuid == uuid)
            .ok_or_else(|| Error::NoSuchAccount(uuid.to_string()))?;
        let removed = self.accounts.remove(idx);
        if self.active.as_deref() == Some(uuid) {
            self.active = None;
        }
        Ok(removed)
    }

    pub fn set_active(&mut self, uuid: &str) {
        self.active = Some(uuid.to_string());
        if let Some(a) = self.accounts.iter_mut().find(|a| a.uuid == uuid) {
            a.last_used_at = Some(now_rfc3339());
        }
    }

    pub fn active_meta(&self) -> Option<&AccountMeta> {
        let active = self.active.as_deref()?;
        self.accounts.iter().find(|a| a.uuid == active)
    }
}
