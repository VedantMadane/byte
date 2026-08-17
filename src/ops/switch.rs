//! Capturing, syncing back, and switching accounts (spec §7).

use serde_json::Value;

use crate::claude::files::ClaudeFiles;
use crate::error::{Error, Result};
use crate::paths::HostPaths;
use crate::store::metadata::{AccountMeta, AccountsFile};
use crate::store::secrets::SecretStore;

/// What sync-back did before a switch proceeded.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncOutcome {
    /// A known account's stored credentials were refreshed from disk.
    Updated(AccountMeta),
    /// A live account byte had never seen was saved (spec §7.2).
    Captured(AccountMeta),
    /// Nothing was logged in.
    LoggedOut,
}

/// The result of a completed switch.
#[derive(Debug, Clone)]
pub struct SwitchOutcome {
    pub switched_to: AccountMeta,
    pub sync: SyncOutcome,
    pub already_active: bool,
}

pub struct Switcher<P: HostPaths, S: SecretStore> {
    paths: P,
    secrets: S,
}

impl<P: HostPaths + Copy, S: SecretStore> Switcher<P, S> {
    pub fn new(paths: P, secrets: S) -> Self {
        Self { paths, secrets }
    }

    pub fn secrets(&self) -> &S {
        &self.secrets
    }

    fn files(&self) -> ClaudeFiles<P> {
        ClaudeFiles::new(self.paths)
    }

    fn load_accounts(&self) -> Result<AccountsFile> {
        AccountsFile::load(&self.paths.accounts_file())
    }

    fn save_accounts(&self, file: &AccountsFile) -> Result<()> {
        file.save(&self.paths.accounts_file(), &self.paths.backup_dir())
    }

    /// Save whatever account is live right now, then mark it active.
    pub fn capture_current(&self) -> Result<AccountMeta> {
        let snapshot = self.files().capture()?.ok_or(Error::NotLoggedIn)?;
        snapshot.validate()?;

        // validate() already returned Err above if identity() were None, so
        // this is guaranteed to be Some.
        let uuid = snapshot
            .identity()
            .expect("validate() guarantees an identity")
            .to_string();

        let mut accounts = self.load_accounts()?;
        let meta = accounts.upsert_from(&snapshot);
        accounts.set_active(&uuid);

        self.secrets.put(&uuid, &snapshot)?;
        self.save_accounts(&accounts)?;
        Ok(meta)
    }

    /// Copy the live credentials into the store, so a token Claude Code
    /// rotated behind byte's back is not lost.
    ///
    /// A live snapshot with no refresh token at all is genuinely logged
    /// out, which is reported as [`SyncOutcome::LoggedOut`] rather than an
    /// error. But a live snapshot that DOES carry a refresh token and still
    /// fails validation — in practice, `.claude.json` has no
    /// `oauthAccount`, so there is no identity to key it by — is not
    /// logged out: there are real credentials on disk this call cannot
    /// safely discard by mislabelling them "logged out". That case is a
    /// hard error instead, specifically so `switch_to`'s `?` aborts before
    /// reaching `apply()`, which would otherwise overwrite those
    /// credentials having never captured them anywhere (spec §7.1).
    pub fn sync_back(&self) -> Result<SyncOutcome> {
        let Some(snapshot) = self.files().capture()? else {
            return Ok(SyncOutcome::LoggedOut);
        };

        if snapshot.validate().is_err() {
            let has_refresh_token = snapshot
                .oauth
                .get("refreshToken")
                .and_then(Value::as_str)
                .is_some_and(|token| !token.is_empty());

            return if has_refresh_token {
                Err(Error::UnidentifiableLiveAccount {
                    config_path: self.paths.claude_config(),
                })
            } else {
                Ok(SyncOutcome::LoggedOut)
            };
        }

        // validate() already returned Err above if identity() were None, so
        // this is guaranteed to be Some.
        let uuid = snapshot
            .identity()
            .expect("validate() guarantees an identity")
            .to_string();

        let mut accounts = self.load_accounts()?;
        let known = accounts.accounts.iter().any(|a| a.uuid == uuid);

        let meta = accounts.upsert_from(&snapshot);
        self.secrets.put(&uuid, &snapshot)?;
        self.save_accounts(&accounts)?;

        Ok(if known {
            SyncOutcome::Updated(meta)
        } else {
            SyncOutcome::Captured(meta)
        })
    }

    /// Switch to a stored account, syncing the current one back first.
    pub fn switch_to(&self, query: &str) -> Result<SwitchOutcome> {
        // Resolve before touching anything, so an unknown name is a clean
        // no-op rather than a half-applied switch.
        let target_uuid = {
            let accounts = self.load_accounts()?;
            accounts.resolve(query)?.uuid.clone()
        };

        let sync = self.sync_back()?;

        let snapshot = self
            .secrets
            .get(&target_uuid)?
            .ok_or_else(|| Error::InvalidSnapshot {
                account: target_uuid.clone(),
                reason: "no stored credentials; re-authenticate with `byte add`".into(),
            })?;
        snapshot.validate()?;

        let mut accounts = self.load_accounts()?;
        let already_active = accounts.active.as_deref() == Some(target_uuid.as_str());

        self.files().apply(&snapshot)?;

        accounts.upsert_from(&snapshot);
        accounts.set_active(&target_uuid);
        self.save_accounts(&accounts)?;

        // An exact uuid lookup, not `resolve()`: after the switch has
        // committed, a second fuzzy lookup could in principle match more
        // than one account (e.g. one account's uuid-as-identity fallback
        // colliding with another account's email) and report
        // `AmbiguousAccount` here — masking a switch that already succeeded
        // behind the wrong error.
        let switched_to = accounts
            .accounts
            .iter()
            .find(|a| a.uuid == target_uuid)
            .cloned()
            .ok_or_else(|| Error::NoSuchAccount(target_uuid.clone()))?;

        Ok(SwitchOutcome {
            switched_to,
            sync,
            already_active,
        })
    }
}
