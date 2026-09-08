//! The tray menu's structure, independent of any GUI toolkit.
//!
//! Kept free of `tray-icon` types on purpose: menu content is the part worth
//! testing, and the event loop is the part that cannot be.

use crate::ops::manage::AccountListing;

/// One row of the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    Account {
        uuid: String,
        /// The user-facing name -- the label, never the email address.
        label: String,
        /// Secondary text, currently the organization name.
        detail: Option<String>,
        active: bool,
    },
    Separator,
    AddAccount,
    Quit,
}

/// The whole menu, in display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    pub entries: Vec<MenuEntry>,
}

impl MenuModel {
    /// Build the menu from what `byte list` would show.
    pub fn from_listing(listing: &[AccountListing]) -> Self {
        let mut entries: Vec<MenuEntry> = listing
            .iter()
            .map(|l| MenuEntry::Account {
                uuid: l.meta.uuid.clone(),
                label: l.meta.label.clone(),
                detail: l.meta.organization_name.clone(),
                active: l.active,
            })
            .collect();

        if !entries.is_empty() {
            entries.push(MenuEntry::Separator);
        }
        entries.push(MenuEntry::AddAccount);
        entries.push(MenuEntry::Separator);
        entries.push(MenuEntry::Quit);

        Self { entries }
    }
}
