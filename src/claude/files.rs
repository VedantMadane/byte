//! Capturing and applying snapshots against the live Claude Code files.

use std::path::Path;

use serde_json::Value;

use crate::claude::document::JsonDocument;
use crate::claude::snapshot::AccountSnapshot;
use crate::error::{Error, Result};
use crate::paths::HostPaths;

const OAUTH_KEY: &str = "claudeAiOauth";
const ACCOUNT_KEY: &str = "oauthAccount";
const USER_ID_KEY: &str = "userID";

/// Reads and writes the two files that hold Claude Code's account identity.
pub struct ClaudeFiles<P: HostPaths> {
    paths: P,
}

impl<P: HostPaths> ClaudeFiles<P> {
    pub fn new(paths: P) -> Self {
        Self { paths }
    }

    /// Snapshot whichever account is logged in right now.
    ///
    /// Returns `Ok(None)` when no account is present — a logged-out state is
    /// normal, not an error.
    pub fn capture(&self) -> Result<Option<AccountSnapshot>> {
        let creds = JsonDocument::load_or_empty(&self.paths.claude_credentials())?;
        let cfg = JsonDocument::load_or_empty(&self.paths.claude_config())?;

        let oauth = match creds.get(OAUTH_KEY) {
            Some(v) if !v.is_null() => v.clone(),
            _ => return Ok(None),
        };

        let account = cfg.get(ACCOUNT_KEY).cloned().unwrap_or(Value::Null);
        let user_id = cfg
            .get(USER_ID_KEY)
            .and_then(Value::as_str)
            .map(str::to_string);

        Ok(Some(AccountSnapshot::new(oauth, account, user_id)))
    }

    /// Make `snapshot` the logged-in account, leaving all other keys alone.
    ///
    /// The credentials file is written first. If anything past that point
    /// fails -- the credentials write itself (e.g. a housekeeping-adjacent
    /// I/O error), or the config-file half (it cannot be loaded, or it
    /// cannot be saved) -- the credentials file is rolled back to what it
    /// held before this call, so the two files never disagree about which
    /// account is active. Left unhandled, that disagreement is not just
    /// cosmetic: a later capture would pair the *new* account's tokens with
    /// the *old* account's identity, and applying that hybrid to the account
    /// store would overwrite the old account's stored refresh token
    /// permanently. Every fallible step after the credentials write commits
    /// therefore goes through `rollback_credentials`, not just the
    /// config-file half.
    pub fn apply(&self, snapshot: &AccountSnapshot) -> Result<()> {
        snapshot.validate()?;
        let backups = self.paths.backup_dir();

        let creds_path = self.paths.claude_credentials();
        let mut creds = JsonDocument::load_or_empty(&creds_path)?;
        let original_creds = creds.clone();
        creds.set(OAUTH_KEY, snapshot.oauth.clone());

        if let Err(creds_error) = creds.save(&creds_path, &backups) {
            return Err(rollback_credentials(
                &original_creds,
                &creds_path,
                &backups,
                creds_error,
            ));
        }

        if let Err(config_error) = self.apply_config(snapshot, &backups) {
            return Err(rollback_credentials(
                &original_creds,
                &creds_path,
                &backups,
                config_error,
            ));
        }

        Ok(())
    }

    /// The config-file half of `apply()`, split out so its error can be
    /// caught and turned into a credentials rollback.
    fn apply_config(&self, snapshot: &AccountSnapshot, backups: &Path) -> Result<()> {
        let cfg_path = self.paths.claude_config();
        let mut cfg = JsonDocument::load_or_empty(&cfg_path)?;
        cfg.set(ACCOUNT_KEY, snapshot.account.clone());
        match &snapshot.user_id {
            Some(id) => cfg.set(USER_ID_KEY, Value::String(id.clone())),
            None => cfg.remove(USER_ID_KEY),
        }
        cfg.save(&cfg_path, backups)
    }

    /// Put Claude Code into a logged-out state, used by the add flow.
    pub fn clear(&self) -> Result<()> {
        let backups = self.paths.backup_dir();

        let creds_path = self.paths.claude_credentials();
        let mut creds = JsonDocument::load_or_empty(&creds_path)?;
        creds.remove(OAUTH_KEY);
        creds.save(&creds_path, &backups)?;

        let cfg_path = self.paths.claude_config();
        let mut cfg = JsonDocument::load_or_empty(&cfg_path)?;
        cfg.remove(ACCOUNT_KEY);
        cfg.remove(USER_ID_KEY);
        cfg.save(&cfg_path, &backups)?;

        Ok(())
    }
}

/// Restore `creds_path` to `original`'s content after either half of
/// `apply()` failed -- the credentials write itself, or the config-file half
/// -- so the two files never end up disagreeing about which account is
/// active.
///
/// Returns `cause` unchanged when the rollback succeeds. If the rollback
/// itself fails, that is not silently swallowed: the caller gets
/// `Error::ApplyRollbackFailed`, which carries both failures, because at that
/// point the files really are left inconsistent and the user must be told.
fn rollback_credentials(
    original: &JsonDocument,
    creds_path: &Path,
    backups: &Path,
    cause: Error,
) -> Error {
    match original.save(creds_path, backups) {
        Ok(()) => cause,
        Err(rollback_source) => Error::ApplyRollbackFailed {
            creds_path: creds_path.to_path_buf(),
            apply_error: cause.to_string(),
            rollback_source: Box::new(rollback_source),
        },
    }
}
