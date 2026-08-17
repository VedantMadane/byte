//! Listing and maintaining stored accounts.

use crate::error::Result;
use crate::ops::switch::Switcher;
use crate::paths::HostPaths;
use crate::store::metadata::AccountMeta;
use crate::store::secrets::SecretStore;

/// An account plus whether it is the active one.
#[derive(Debug, Clone)]
pub struct AccountListing {
    pub meta: AccountMeta,
    pub active: bool,
}

pub fn list<P: HostPaths + Copy, S: SecretStore>(
    sw: &Switcher<P, S>,
) -> Result<Vec<AccountListing>> {
    let file = sw.load_accounts()?;
    Ok(file
        .accounts
        .iter()
        .map(|meta| AccountListing {
            active: file.active.as_deref() == Some(meta.uuid.as_str()),
            meta: meta.clone(),
        })
        .collect())
}

pub fn current<P: HostPaths + Copy, S: SecretStore>(
    sw: &Switcher<P, S>,
) -> Result<Option<AccountMeta>> {
    Ok(sw.load_accounts()?.active_meta().cloned())
}

pub fn rename<P: HostPaths + Copy, S: SecretStore>(
    sw: &Switcher<P, S>,
    query: &str,
    label: &str,
) -> Result<AccountMeta> {
    let mut file = sw.load_accounts()?;
    let uuid = file.resolve(query)?.uuid.clone();
    let meta = file.rename(&uuid, label)?;
    sw.save_accounts(&file)?;
    Ok(meta)
}

/// Delete both the metadata entry and the stored secret. The secret is
/// deleted first: if that fails, the metadata file is left untouched so the
/// account stays visible rather than silently orphaning a live refresh
/// token in the OS keychain that `list` can no longer show and the user
/// believes is gone.
pub fn remove<P: HostPaths + Copy, S: SecretStore>(
    sw: &Switcher<P, S>,
    query: &str,
) -> Result<AccountMeta> {
    let mut file = sw.load_accounts()?;
    let uuid = file.resolve(query)?.uuid.clone();
    let meta = file.remove(&uuid)?;
    sw.secrets().delete(&uuid)?;
    sw.save_accounts(&file)?;
    Ok(meta)
}
