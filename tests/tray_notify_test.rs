//! Coverage for `switch_message`'s content -- the testable half of
//! `src/tray/notify.rs`. `send` itself is not exercised here: firing a real
//! desktop toast from a test suite is antisocial (it pops up on whoever runs
//! the suite) and unverifiable (nothing reads it back), so only the message
//! text is pinned.

use byte::ops::switch::{SwitchOutcome, SyncOutcome};
use byte::store::metadata::AccountMeta;
use byte::tray::notify::switch_message;

fn meta(label: &str) -> AccountMeta {
    AccountMeta {
        uuid: "u1".into(),
        label: label.into(),
        email: Some("a@example.com".into()),
        organization_name: Some("Indicio".into()),
        subscription_type: Some("max".into()),
        account: serde_json::json!({"accountUuid": "u1"}),
        user_id: Some("uid".into()),
        credential_schema: 1,
        added_at: "2026-01-01T00:00:00Z".into(),
        last_used_at: None,
    }
}

fn outcome(already_active: bool) -> SwitchOutcome {
    SwitchOutcome {
        switched_to: meta("work"),
        sync: SyncOutcome::LoggedOut,
        already_active,
    }
}

#[test]
fn a_switch_names_the_account_it_switched_to() {
    let (title, body) = switch_message(&outcome(false), 0);
    assert!(
        title.contains("work") || body.contains("work"),
        "{title} / {body}"
    );
}

#[test]
fn a_switch_with_running_sessions_says_to_restart_them() {
    let (_title, body) = switch_message(&outcome(false), 2);
    assert!(body.contains('2'), "should name the count: {body}");
    assert!(
        body.to_lowercase().contains("restart"),
        "should say to restart: {body}"
    );
}

#[test]
fn a_switch_with_no_running_sessions_does_not_mention_restarting() {
    let (_title, body) = switch_message(&outcome(false), 0);
    assert!(
        !body.to_lowercase().contains("restart"),
        "should not mention restarting when nothing runs: {body}"
    );
}

// The brief's own fixture set covers zero sessions and two ("many"), but
// leaves the singular boundary (exactly one) unexercised. A body built from
// e.g. `format!("{n} running Claude Code sessions ... restart them.")` for
// every n >= 1 would still pass the "many" test above and the zero test --
// it only mis-pluralizes when n is exactly 1. Pin that boundary directly.
#[test]
fn a_switch_with_one_running_session_uses_singular_language() {
    let (_title, body) = switch_message(&outcome(false), 1);
    assert!(body.contains('1'), "should name the count: {body}");
    assert!(
        body.to_lowercase().contains("restart"),
        "should say to restart: {body}"
    );
    assert!(
        !body.contains("sessions"),
        "should use singular \"session\", not plural: {body}"
    );
}

#[test]
fn an_already_active_switch_says_so_rather_than_claiming_a_change() {
    let (title, body) = switch_message(&outcome(true), 0);
    let text = format!("{title} {body}").to_lowercase();
    assert!(text.contains("already"), "{title} / {body}");
}

// Pairs with the test above: an implementation that stamps "already" into
// *every* message (regardless of `already_active`) would still pass every
// test up to this point -- the zero/one/two-session tests never check for
// its absence, so unconditional boilerplate like "Already switched -- ..."
// would slip through undetected. A genuine switch's message must not use
// the word that is reserved for the no-op case.
#[test]
fn a_fresh_switch_does_not_say_already() {
    let (title, body) = switch_message(&outcome(false), 0);
    let text = format!("{title} {body}").to_lowercase();
    assert!(!text.contains("already"), "{title} / {body}");
}
