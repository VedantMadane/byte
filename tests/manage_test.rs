use byte::ops::manage;
use byte::ops::switch::Switcher;
use byte::paths::{HostPaths, TestPaths};
use byte::store::secrets::{MemoryStore, SecretStore};
use serde_json::json;

fn login_as(tp: &TestPaths, uuid: &str, email: &str) {
    std::fs::write(
        tp.claude_credentials(),
        serde_json::to_string(&json!({
            "claudeAiOauth": {"refreshToken": "r", "expiresAt": 1i64}
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        tp.claude_config(),
        serde_json::to_string_pretty(&json!({
            "oauthAccount": {"accountUuid": uuid, "emailAddress": email},
            "userID": "uid"
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn list_is_empty_before_anything_is_captured() {
    let tp = TestPaths::new().unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());
    assert!(manage::list(&sw).unwrap().is_empty());
}

#[test]
fn list_marks_exactly_one_account_active() {
    let tp = TestPaths::new().unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());
    login_as(&tp, "u1", "a@example.com");
    sw.capture_current().unwrap();
    login_as(&tp, "u2", "b@example.com");
    sw.capture_current().unwrap();

    // Switch back to u1 so the active account is neither the most recently
    // captured account nor the last entry in the accounts vector. That is
    // what actually distinguishes "active is derived from the accounts
    // file's `active` pointer" from a broken implementation that just
    // guesses (e.g. always reporting the last-inserted account as active).
    sw.switch_to("a@example.com").unwrap();

    let listing = manage::list(&sw).unwrap();

    assert_eq!(listing.len(), 2);
    assert_eq!(listing.iter().filter(|l| l.active).count(), 1);
    assert!(listing.iter().find(|l| l.active).unwrap().meta.uuid == "u1");
}

#[test]
fn current_returns_the_active_account() {
    let tp = TestPaths::new().unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());
    login_as(&tp, "u1", "a@example.com");
    sw.capture_current().unwrap();

    assert_eq!(manage::current(&sw).unwrap().unwrap().uuid, "u1");
}

#[test]
fn rename_changes_the_label_and_it_is_then_resolvable() {
    let tp = TestPaths::new().unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());
    login_as(&tp, "u1", "a@example.com");
    sw.capture_current().unwrap();

    manage::rename(&sw, "a@example.com", "work").unwrap();

    assert_eq!(manage::current(&sw).unwrap().unwrap().label, "work");
    assert!(sw.switch_to("work").is_ok());
}

#[test]
fn remove_deletes_both_metadata_and_secret() {
    let tp = TestPaths::new().unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());
    login_as(&tp, "u1", "a@example.com");
    sw.capture_current().unwrap();

    manage::remove(&sw, "a@example.com").unwrap();

    assert!(manage::list(&sw).unwrap().is_empty());
    assert!(sw.secrets().get("u1").unwrap().is_none());
}

#[test]
fn removing_an_unknown_account_errors() {
    let tp = TestPaths::new().unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());
    assert!(matches!(
        manage::remove(&sw, "nobody"),
        Err(byte::Error::NoSuchAccount(_))
    ));
}
