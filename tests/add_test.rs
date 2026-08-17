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

#[test]
fn poll_failure_does_not_prevent_recovering_the_previous_account() {
    // Teeth-check for task-11 review Finding 1. byte polls both live files
    // every 500ms for up to five minutes -- exactly the window in which
    // Claude Code might be mid-write to one of them -- so a poll can catch
    // a partial write and fail with Error::Parse. begin() has already
    // logged u1 out by the time any poll runs (proven below by u1 no
    // longer being live), so src/cli/run.rs's cmd_add now always calls
    // abort() on a poll_once error rather than propagating it bare -- see
    // resolve_add_failure and its unit tests in tests/cli_run_test.rs,
    // which cover that decision on its own. This test proves the
    // ingredient that fix depends on: a real poll_once failure, of the
    // exact shape the review described, does not prevent abort() from
    // still restoring the previous account afterward.
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    let session = AddSession::begin(&sw).unwrap();
    assert!(
        ClaudeFiles::new(&tp).capture().unwrap().is_none(),
        "begin() should have already logged u1 out before any poll runs"
    );

    // Simulate a poll catching Claude Code mid-write to .claude.json. This
    // models the write of a *second* login being interrupted mid-poll, not
    // u1's own data -- that's already safely in the store, untouched by
    // this corruption.
    std::fs::write(tp.claude_config(), "{ not valid json").unwrap();
    let err = session.poll_once(&sw).unwrap_err();
    assert!(matches!(err, byte::Error::Parse { .. }));

    // The write completes a moment later, same as it would in reality --
    // this models the interrupted write finishing, not byte fixing
    // anything.
    std::fs::write(tp.claude_config(), "{}").unwrap();

    session.abort(&sw).unwrap();

    let restored = ClaudeFiles::new(&tp).capture().unwrap().unwrap();
    assert_eq!(restored.email(), Some("a@example.com"));
}
