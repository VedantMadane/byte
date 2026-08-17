//! Adding an account by logging Claude Code out and watching for a new login.

use crate::error::{Error, Result};
use crate::ops::switch::Switcher;
use crate::paths::HostPaths;
use crate::store::metadata::AccountMeta;
use crate::store::secrets::SecretStore;

/// An in-progress add. Created by [`AddSession::begin`], driven by
/// [`AddSession::poll_once`], and cancelled by [`AddSession::abort`].
#[derive(Debug, Clone)]
pub struct AddSession {
    previous: Option<AccountMeta>,
}

impl AddSession {
    /// Save the current account, verify it is recoverable, then log out.
    ///
    /// The verification step is what makes this safe: byte only clears
    /// credentials it has already read back out of the store.
    pub fn begin<P: HostPaths + Copy, S: SecretStore>(sw: &Switcher<P, S>) -> Result<Self> {
        let previous = match sw.capture_current() {
            Ok(meta) => {
                // Prove recoverability before destroying the live copy.
                if sw.secrets().get(&meta.uuid)?.is_none() {
                    return Err(Error::InvalidSnapshot {
                        account: meta.label.clone(),
                        reason: "could not be read back from the store; refusing to log out".into(),
                    });
                }
                Some(meta)
            }
            Err(Error::NotLoggedIn) => None,
            Err(e) => return Err(e),
        };

        sw.files_for_add().clear()?;
        Ok(Self { previous })
    }

    pub fn previous(&self) -> Option<&AccountMeta> {
        self.previous.as_ref()
    }

    /// Check once for a completed login as a *different* account.
    pub fn poll_once<P: HostPaths + Copy, S: SecretStore>(
        &self,
        sw: &Switcher<P, S>,
    ) -> Result<Option<AccountMeta>> {
        let Some(snapshot) = sw.files_for_add().capture()? else {
            return Ok(None);
        };
        if snapshot.validate().is_err() {
            return Ok(None);
        }

        let identity = snapshot.identity().unwrap_or_default();
        if let Some(prev) = &self.previous {
            if identity == prev.uuid {
                return Ok(None);
            }
        }

        Ok(Some(sw.capture_current()?))
    }

    /// Give up and put the previous account back.
    pub fn abort<P: HostPaths + Copy, S: SecretStore>(&self, sw: &Switcher<P, S>) -> Result<()> {
        match &self.previous {
            Some(prev) => {
                sw.switch_to(&prev.uuid)?;
                Ok(())
            }
            None => Ok(()),
        }
    }
}
