use byte::claude::files::ClaudeFiles;
use byte::claude::snapshot::AccountSnapshot;
use byte::ops::add::AddSession;
use byte::ops::switch::Switcher;
use byte::paths::{HostPaths, TestPaths};
use byte::store::secrets::{MemoryStore, SecretStore};
use serde_json::json;

/// A store that accepts every write but can never read one back --
/// simulates `begin`'s recoverability check failing even though
/// `capture_current`'s own `put` reported success. Exists to prove that
/// check actually gates `clear()`, not just that the end state looks right:
/// `MemoryStore` can never diverge between a `put` and the following `get`,
/// so no test built on it alone can tell the check apart from its absence.
struct UnreadableStore;

impl SecretStore for UnreadableStore {
    fn put(&self, _uuid: &str, _snapshot: &AccountSnapshot) -> byte::Result<()> {
        Ok(())
    }

    fn get(&self, _uuid: &str) -> byte::Result<Option<AccountSnapshot>> {
        Ok(None)
    }

    fn delete(&self, _uuid: &str) -> byte::Result<()> {
        Ok(())
    }
}

fn login_as(tp: &TestPaths, uuid: &str, email: &str, refresh: &str) {
    std::fs::write(
        tp.claude_credentials(),
        serde_json::to_string(&json!({
            "claudeAiOauth": {
                "accessToken": "a", "refreshToken": refresh, "expiresAt": 1i64
            }
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
fn begin_saves_the_outgoing_account_then_logs_out() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());

    let session = AddSession::begin(&sw).unwrap();

    assert_eq!(session.previous().unwrap().uuid, "u1");
    assert!(ClaudeFiles::new(&tp).capture().unwrap().is_none());
    // Recoverable: the outgoing credentials are in the store.
    assert!(sw.secrets().get("u1").unwrap().is_some());
}

#[test]
fn begin_works_from_a_logged_out_state() {
    let tp = TestPaths::new().unwrap();
    std::fs::write(tp.claude_credentials(), "{}").unwrap();
    std::fs::write(tp.claude_config(), "{}").unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());

    let session = AddSession::begin(&sw).unwrap();

    assert!(session.previous().is_none());
}

#[test]
fn poll_returns_none_while_still_logged_out() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    let session = AddSession::begin(&sw).unwrap();

    assert!(session.poll_once(&sw).unwrap().is_none());
}

#[test]
fn poll_captures_a_new_login() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    let session = AddSession::begin(&sw).unwrap();

    login_as(&tp, "u2", "b@example.com", "r2");
    let found = session.poll_once(&sw).unwrap().unwrap();

    assert_eq!(found.uuid, "u2");
    assert!(sw.secrets().get("u2").unwrap().is_some());
}

#[test]
fn poll_ignores_a_re_login_as_the_same_account() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    let session = AddSession::begin(&sw).unwrap();

    login_as(&tp, "u1", "a@example.com", "r1-again");

    assert!(session.poll_once(&sw).unwrap().is_none());
}

#[test]
fn begin_aborts_rather_than_logging_out_an_unidentifiable_live_account() {
    // Real, non-empty credentials, but .claude.json has no oauthAccount at
    // all (otherwise valid JSON) - e.g. a partially-completed login.
    // capture_current fails validate()'s identity check (InvalidSnapshot,
    // not NotLoggedIn), and that must NOT be swallowed into previous=None:
    // doing so would let begin() clear credentials it never managed to save
    // anywhere.
    let tp = TestPaths::new().unwrap();
    std::fs::write(
        tp.claude_credentials(),
        serde_json::to_string(&json!({
            "claudeAiOauth": {
                "accessToken": "a", "refreshToken": "irreplaceable", "expiresAt": 1i64
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        tp.claude_config(),
        serde_json::to_string(&json!({"numStartups": 1})).unwrap(),
    )
    .unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());

    let err = AddSession::begin(&sw).unwrap_err();

    assert!(matches!(err, byte::Error::InvalidSnapshot { .. }));
    // The unidentifiable live credentials must survive untouched - clear()
    // must never have run.
    let creds: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_credentials()).unwrap()).unwrap();
    assert_eq!(
        creds["claudeAiOauth"]["refreshToken"],
        json!("irreplaceable")
    );
}

#[test]
fn begin_refuses_to_clear_when_the_store_cannot_read_back_what_it_wrote() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, UnreadableStore);

    let err = AddSession::begin(&sw).unwrap_err();

    assert!(matches!(err, byte::Error::InvalidSnapshot { .. }));
    // The one assertion that actually matters: clear() must never have run.
    // An implementation that errored AFTER clearing would still satisfy the
    // assertion above, so this checks the live files directly rather than
    // just the returned error.
    let live = ClaudeFiles::new(&tp).capture().unwrap().unwrap();
    assert_eq!(live.email(), Some("a@example.com"));
}

#[test]
fn abort_restores_the_previous_account() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    let session = AddSession::begin(&sw).unwrap();

    session.abort(&sw).unwrap();

    let restored = ClaudeFiles::new(&tp).capture().unwrap().unwrap();
    assert_eq!(restored.email(), Some("a@example.com"));
}
