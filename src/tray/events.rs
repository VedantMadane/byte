//! Turning raw tray and menu events into intentions.
//!
//! Pure on purpose: the mapping is where the mistakes are (a stale menu id
//! must not resolve to the wrong account), and it is testable without a GUI.

use crate::tray::menu::{MenuEntry, MenuModel};

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    SwitchTo(String),
    AddAccount,
    Quit,
    /// Nothing to do -- an unknown id, a separator, or pointer motion.
    Ignore,
}

/// The kinds of tray-icon event byte distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEventKind {
    Click,
    DoubleClick,
    Move,
    Enter,
    Leave,
}

/// Whether an event is worth doing any work for.
///
/// `TrayIconEvent` is a pointer stream, not a click stream: hovering the icon
/// emits `Move` continuously. Everything except a click is dropped here,
/// before any I/O happens.
pub fn is_actionable_tray_event(kind: TrayEventKind) -> bool {
    matches!(kind, TrayEventKind::Click | TrayEventKind::DoubleClick)
}

/// Resolve a clicked menu id to an action.
///
/// `ids` is parallel to `model.entries` -- element *i* is the id assigned to
/// entry *i* when the menu was built. An id that is not in `ids` is
/// [`Action::Ignore`]: menu ids change every rebuild, and a stale click must
/// never be resolved positionally to whatever account now sits there.
pub fn action_for_menu_id(model: &MenuModel, ids: &[String], clicked: &str) -> Action {
    let Some(index) = ids.iter().position(|id| id == clicked) else {
        return Action::Ignore;
    };
    match model.entries.get(index) {
        Some(MenuEntry::Account { uuid, .. }) => Action::SwitchTo(uuid.clone()),
        Some(MenuEntry::AddAccount) => Action::AddAccount,
        Some(MenuEntry::Quit) => Action::Quit,
        _ => Action::Ignore,
    }
}
