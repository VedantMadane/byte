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

/// One rendered row, ready for the toolkit to turn into a widget.
///
/// Deliberately produced one-per-`MenuEntry`, separators included: the event
/// loop resolves a click by looking its menu id up by *position* and reading
/// `MenuModel::entries` at that same index, so the two sequences must stay
/// the same length and order. Skipping a non-clickable row while building
/// would shift every later index and resolve a click to the wrong account --
/// with one account stored, a click on "Quit" would resolve to
/// "Add account...". Returning a row for every entry makes that invariant
/// structural instead of a rule a comment asks the next reader to keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuRow {
    /// A clickable row, with the exact text to display.
    Item(String),
    /// A non-clickable divider, which still occupies an index.
    Separator,
}

/// Render every entry to its display text, one row per entry.
pub fn menu_rows(model: &MenuModel) -> Vec<MenuRow> {
    model
        .entries
        .iter()
        .map(|entry| match entry {
            MenuEntry::Account {
                label,
                detail,
                active,
                ..
            } => {
                let text = match detail {
                    Some(d) => format!("{label}  ({d})"),
                    None => label.clone(),
                };
                // A leading marker either way, so the rows stay aligned and
                // the active one is distinguishable without relying on
                // position.
                MenuRow::Item(if *active {
                    format!("● {text}")
                } else {
                    format!("   {text}")
                })
            }
            MenuEntry::Separator => MenuRow::Separator,
            MenuEntry::AddAccount => MenuRow::Item("Add account…".to_string()),
            MenuEntry::Quit => MenuRow::Item("Quit".to_string()),
        })
        .collect()
}
