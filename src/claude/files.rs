//! Capturing and applying snapshots against the live Claude Code files.

use serde_json::Value;

use crate::claude::document::JsonDocument;
use crate::claude::snapshot::AccountSnapshot;
use crate::error::Result;
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
    pub fn apply(&self, snapshot: &AccountSnapshot) -> Result<()> {
        snapshot.validate()?;
        let backups = self.paths.backup_dir();

        let creds_path = self.paths.claude_credentials();
        let mut creds = JsonDocument::load_or_empty(&creds_path)?;
        creds.set(OAUTH_KEY, snapshot.oauth.clone());
        creds.save(&creds_path, &backups)?;

        let cfg_path = self.paths.claude_config();
        let mut cfg = JsonDocument::load_or_empty(&cfg_path)?;
        cfg.set(ACCOUNT_KEY, snapshot.account.clone());
        match &snapshot.user_id {
            Some(id) => cfg.set(USER_ID_KEY, Value::String(id.clone())),
            None => cfg.remove(USER_ID_KEY),
        }
        cfg.save(&cfg_path, &backups)?;

        Ok(())
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
