use byte::tray::events::{Action, TrayEventKind, action_for_menu_id, is_actionable_tray_event};
use byte::tray::menu::{MenuEntry, MenuModel};

fn model() -> MenuModel {
    MenuModel {
        entries: vec![
            MenuEntry::Account {
                uuid: "u1".into(),
                label: "personal".into(),
                detail: None,
                active: true,
            },
            MenuEntry::Account {
                uuid: "u2".into(),
                label: "work".into(),
                detail: None,
                active: false,
            },
            MenuEntry::Separator,
            MenuEntry::AddAccount,
            MenuEntry::Separator,
            MenuEntry::Quit,
        ],
    }
}

/// Menu item ids assigned in the same order as `model()`'s entries.
fn ids() -> Vec<String> {
    vec![
        "id-u1".into(),
        "id-u2".into(),
        "id-sep1".into(),
        "id-add".into(),
        "id-sep2".into(),
        "id-quit".into(),
    ]
}

#[test]
fn clicking_an_account_switches_to_that_account() {
    assert_eq!(
        action_for_menu_id(&model(), &ids(), "id-u2"),
        Action::SwitchTo("u2".into())
    );
}

#[test]
fn clicking_the_already_active_account_still_maps_to_a_switch() {
    // switch_to is idempotent and still performs sync-back, so the tray does
    // not need to special-case it.
    assert_eq!(
        action_for_menu_id(&model(), &ids(), "id-u1"),
        Action::SwitchTo("u1".into())
    );
}

#[test]
fn clicking_add_maps_to_add() {
    assert_eq!(
        action_for_menu_id(&model(), &ids(), "id-add"),
        Action::AddAccount
    );
}

#[test]
fn clicking_quit_maps_to_quit() {
    assert_eq!(
        action_for_menu_id(&model(), &ids(), "id-quit"),
        Action::Quit
    );
}

#[test]
fn an_unknown_id_is_ignored_rather_than_guessed() {
    // A stale id can arrive after the menu is rebuilt. Guessing would switch
    // the user to an arbitrary account.
    assert_eq!(
        action_for_menu_id(&model(), &ids(), "id-gone"),
        Action::Ignore
    );
}

#[test]
fn a_separator_id_is_ignored() {
    assert_eq!(
        action_for_menu_id(&model(), &ids(), "id-sep1"),
        Action::Ignore
    );
}

#[test]
fn pointer_motion_is_not_actionable() {
    // A single hover produced roughly a hundred Move events in the spike.
    // Treating them as actionable would trigger a hundred file reads.
    assert!(!is_actionable_tray_event(TrayEventKind::Move));
    assert!(!is_actionable_tray_event(TrayEventKind::Enter));
    assert!(!is_actionable_tray_event(TrayEventKind::Leave));
}

#[test]
fn clicks_are_actionable() {
    assert!(is_actionable_tray_event(TrayEventKind::Click));
    assert!(is_actionable_tray_event(TrayEventKind::DoubleClick));
}
