use byte::claude::files::ClaudeFiles;
use byte::ops::switch::{SwitchOutcome, Switcher, SyncOutcome};
use byte::paths::{HostPaths, TestPaths};
use byte::store::metadata::AccountsFile;
use byte::store::secrets::{MemoryStore, SecretStore};
use serde_json::json;

fn login_as(tp: &TestPaths, uuid: &str, email: &str, refresh: &str) {
    std::fs::write(
        tp.claude_credentials(),
        serde_json::to_string(&json!({
            "mcpOAuth": {"srv": {"accessToken": "keep"}},
            "claudeAiOauth": {
                "accessToken": "a", "refreshToken": refresh,
                "expiresAt": 1i64, "subscriptionType": "max"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        tp.claude_config(),
        serde_json::to_string_pretty(&json!({
            "numStartups": 7,
            "oauthAccount": {"accountUuid": uuid, "emailAddress": email},
            "userID": format!("uid-{uuid}")
        }))
        .unwrap(),
    )
    .unwrap();
}

fn live_refresh(tp: &TestPaths) -> String {
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_credentials()).unwrap()).unwrap();
    v["claudeAiOauth"]["refreshToken"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn capture_current_stores_the_live_account() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());

    let meta = sw.capture_current().unwrap();

    assert_eq!(meta.uuid, "u1");
    let file = AccountsFile::load(&tp.accounts_file()).unwrap();
    assert_eq!(file.accounts.len(), 1);
    assert_eq!(file.active.as_deref(), Some("u1"));
}

#[test]
fn sync_back_captures_an_account_byte_has_never_seen() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());

    match sw.sync_back().unwrap() {
        SyncOutcome::Captured(m) => assert_eq!(m.uuid, "u1"),
        other => panic!("expected Captured, got {other:?}"),
    }
}

#[test]
fn sync_back_refreshes_a_rotated_token() {
    // The scenario spec section 7.1 exists to defend against.
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "original");
    let sw = Switcher::new(&tp, MemoryStore::new());
    sw.capture_current().unwrap();

    // Claude Code refreshes and rotates the token behind byte's back.
    login_as(&tp, "u1", "a@example.com", "rotated");

    match sw.sync_back().unwrap() {
        SyncOutcome::Updated(m) => assert_eq!(m.uuid, "u1"),
        other => panic!("expected Updated, got {other:?}"),
    }
    assert_eq!(
        sw.secrets().get("u1").unwrap().unwrap().oauth["refreshToken"],
        json!("rotated")
    );
}

#[test]
fn sync_back_reports_logged_out_when_no_account_is_present() {
    let tp = TestPaths::new().unwrap();
    std::fs::write(tp.claude_credentials(), "{}").unwrap();
    std::fs::write(tp.claude_config(), "{}").unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());

    assert!(matches!(sw.sync_back().unwrap(), SyncOutcome::LoggedOut));
}

#[test]
fn switching_writes_the_target_account_to_the_live_files() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    sw.capture_current().unwrap();

    login_as(&tp, "u2", "b@example.com", "r2");
    sw.capture_current().unwrap();

    let out: SwitchOutcome = sw.switch_to("a@example.com").unwrap();

    assert_eq!(out.switched_to.uuid, "u1");
    assert_eq!(live_refresh(&tp), "r1");
    let snap = ClaudeFiles::new(&tp).capture().unwrap().unwrap();
    assert_eq!(snap.email(), Some("a@example.com"));
}

#[test]
fn switching_away_saves_the_current_account_first() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    sw.capture_current().unwrap();
    login_as(&tp, "u2", "b@example.com", "r2");
    sw.capture_current().unwrap();

    // u2 is live; rotate it, then switch away without capturing.
    login_as(&tp, "u2", "b@example.com", "r2-rotated");
    sw.switch_to("a@example.com").unwrap();

    assert_eq!(
        sw.secrets().get("u2").unwrap().unwrap().oauth["refreshToken"],
        json!("r2-rotated")
    );
}

#[test]
fn switching_back_and_forth_round_trips_cleanly() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    sw.capture_current().unwrap();
    login_as(&tp, "u2", "b@example.com", "r2");
    sw.capture_current().unwrap();

    sw.switch_to("a@example.com").unwrap();
    assert_eq!(live_refresh(&tp), "r1");
    sw.switch_to("b@example.com").unwrap();
    assert_eq!(live_refresh(&tp), "r2");
    sw.switch_to("a@example.com").unwrap();
    assert_eq!(live_refresh(&tp), "r1");
}

#[test]
fn switching_preserves_unrelated_keys_in_both_files() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    sw.capture_current().unwrap();
    login_as(&tp, "u2", "b@example.com", "r2");
    sw.capture_current().unwrap();

    sw.switch_to("a@example.com").unwrap();

    let creds: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_credentials()).unwrap()).unwrap();
    let cfg: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_config()).unwrap()).unwrap();
    assert_eq!(creds["mcpOAuth"]["srv"]["accessToken"], json!("keep"));
    assert_eq!(cfg["numStartups"], json!(7));
}

#[test]
fn switching_to_the_active_account_is_a_no_op_that_still_syncs() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    sw.capture_current().unwrap();

    let out = sw.switch_to("a@example.com").unwrap();

    assert!(out.already_active);
    assert_eq!(live_refresh(&tp), "r1");
}

#[test]
fn switching_to_an_unknown_account_fails_without_touching_the_files() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    sw.capture_current().unwrap();

    assert!(sw.switch_to("nobody@example.com").is_err());
    assert_eq!(live_refresh(&tp), "r1");
}
