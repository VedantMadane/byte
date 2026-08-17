//! Unit-level coverage for `src/cli/run.rs`'s decision logic, as opposed to
//! `tests/cli_test.rs`, which drives the compiled binary as a black box.
//!
//! `resolve_add_failure` takes the outcome of an already-attempted restore
//! rather than a `Switcher` and performing the restore itself, specifically
//! so this branching is testable with no `SecretStore`, no file I/O, and no
//! keychain at all -- see the comment on the function itself for why this
//! matters: it is the exact piece of logic task-11 review Finding 1 found
//! broken (a bare `?` that skipped recovery entirely on a `poll_once`
//! error).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use byte::Error;
use byte::claude::files::ClaudeFiles;
use byte::cli::run::{cmd_add, resolve_add_failure, switch_json};
use byte::ops::switch::{SwitchOutcome, Switcher, SyncOutcome};
use byte::paths::{HostPaths, TestPaths};
use byte::store::metadata::AccountMeta;
use byte::store::secrets::MemoryStore;
use serde_json::json;

fn meta(uuid: &str, label: &str) -> AccountMeta {
    AccountMeta {
        uuid: uuid.to_string(),
        label: label.to_string(),
        email: None,
        organization_name: None,
        subscription_type: None,
        added_at: "2026-01-01T00:00:00Z".to_string(),
        last_used_at: None,
    }
}

#[test]
fn switch_json_reports_a_captured_sync_outcome() {
    // Finding M3: `switch --json` silently dropped sync-back's outcome, so
    // a script had no way to learn that sync-back just wrote a previously
    // unknown account's refresh token to the keychain.
    let outcome = SwitchOutcome {
        switched_to: meta("target-uuid", "work"),
        sync: SyncOutcome::Captured(meta("live-uuid", "personal")),
        already_active: false,
    };

    let value = switch_json(&outcome);

    assert_eq!(value["sync"]["outcome"], "captured");
    assert_eq!(value["sync"]["uuid"], "live-uuid");
    assert_eq!(value["sync"]["label"], "personal");
    assert_eq!(value["switched_to"], "work");
    assert_eq!(value["already_active"], false);
}

#[test]
fn switch_json_reports_an_updated_sync_outcome() {
    let outcome = SwitchOutcome {
        switched_to: meta("target-uuid", "work"),
        sync: SyncOutcome::Updated(meta("target-uuid", "work")),
        already_active: true,
    };

    let value = switch_json(&outcome);

    assert_eq!(value["sync"]["outcome"], "updated");
    assert_eq!(value["already_active"], true);
}

#[test]
fn switch_json_reports_a_logged_out_sync_outcome_without_an_account() {
    let outcome = SwitchOutcome {
        switched_to: meta("target-uuid", "work"),
        sync: SyncOutcome::LoggedOut,
        already_active: false,
    };

    let value = switch_json(&outcome);

    assert_eq!(value["sync"]["outcome"], "logged_out");
    // LoggedOut carries no account -- confirm sync_json doesn't fabricate a
    // uuid/label field for it the way the other two variants have.
    assert!(value["sync"].get("uuid").is_none());
}

#[test]
fn reports_the_original_cause_when_the_restore_succeeds() {
    let result = resolve_add_failure(Error::LoginTimeout(5), true, Ok(()));

    assert!(matches!(result, Err(Error::LoginTimeout(5))));
}

#[test]
fn reports_the_restore_failure_rather_than_the_original_cause_when_both_fail() {
    // This is the failure mode the fix exists to prevent: if only the
    // *original* cause were returned here, the caller (and the user) would
    // never learn that the restore attempt -- their one path back to being
    // logged in -- also failed.
    let result = resolve_add_failure(Error::LoginTimeout(5), true, Err(Error::NotLoggedIn));

    assert!(matches!(result, Err(Error::NotLoggedIn)));
}

#[test]
fn distinguishes_a_poll_error_cause_from_a_timeout_cause() {
    // Not just "some Err comes back" -- confirms the specific cause passed
    // in is the one that surfaces when the restore succeeds, for the other
    // shape of failure cmd_add can report (a poll_once error, not just a
    // timeout).
    let result = resolve_add_failure(Error::NotLoggedIn, true, Ok(()));

    assert!(matches!(result, Err(Error::NotLoggedIn)));
}

#[test]
fn had_previous_does_not_change_which_error_propagates() {
    // Finding M7: resolve_add_failure claimed "Restored the previous
    // account" whenever restore_result was Ok(()), even when there was no
    // previous account to restore -- AddSession::abort()'s None branch also
    // returns Ok(()) unconditionally. had_previous exists to fix the STATUS
    // TEXT for that case (see tests/cli_test.rs for an end-to-end
    // assertion on the actual stderr wording, which this module-level
    // function can't observe on its own -- see the file header). What this
    // test pins is the property `#[test]` code CAN observe here: had_previous
    // must only change the message, never which Err propagates.
    let with_previous = resolve_add_failure(Error::LoginTimeout(5), true, Ok(()));
    let without_previous = resolve_add_failure(Error::LoginTimeout(5), false, Ok(()));

    assert!(matches!(with_previous, Err(Error::LoginTimeout(5))));
    assert!(matches!(without_previous, Err(Error::LoginTimeout(5))));
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

/// A `HostPaths` that corrupts `.claude.json` the moment it is asked for
/// that path for the third time, then "finishes the write" (heals it) on
/// the fourth -- timed to land on `cmd_add`'s first `poll_once` call (calls
/// 1 and 2 are `AddSession::begin`'s own `capture_current` and `clear`),
/// simulating catching Claude Code mid-write without needing real
/// concurrency. Mirrors the corrupt-then-fix sequence
/// `tests/add_test.rs::poll_failure_does_not_prevent_recovering_the_previous_account`
/// drives by hand; this drives it through the real `cmd_add`, end to end.
struct FlakyPaths<'a> {
    inner: &'a TestPaths,
    claude_config_calls: AtomicU32,
}

impl HostPaths for FlakyPaths<'_> {
    fn claude_config(&self) -> PathBuf {
        let n = self.claude_config_calls.fetch_add(1, Ordering::SeqCst);
        let path = self.inner.claude_config();
        if n == 2 {
            std::fs::write(&path, "{ not valid json").unwrap();
        } else if n == 3 {
            // The interrupted write completes a moment later, same as it
            // would in reality -- this models Claude Code's own write
            // finishing, not byte fixing anything.
            std::fs::write(&path, "{}").unwrap();
        }
        path
    }

    fn claude_credentials(&self) -> PathBuf {
        self.inner.claude_credentials()
    }

    fn byte_config_dir(&self) -> PathBuf {
        self.inner.byte_config_dir()
    }
}

#[test]
fn cmd_add_restores_the_previous_account_when_poll_once_fails() {
    // Finding I8: the CLI layer's wiring around cmd_add -- does it call
    // abort() before reporting a poll_once failure, does the poll loop
    // terminate -- had zero automated coverage, because every cmd_*
    // function was concretely typed over KeyringStore, forcing a real
    // keychain write to reach it at all. Now that they're generic over
    // `S: SecretStore`, this drives the real `cmd_add` end to end against a
    // `MemoryStore`, reproducing the shape of the original task-11 Finding
    // F26 bug it guards against: a `poll_once` failure must not leave the
    // user logged out with no attempt to recover.
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let paths = FlakyPaths {
        inner: &tp,
        claude_config_calls: AtomicU32::new(0),
    };
    let sw = Switcher::new(&paths, MemoryStore::new());

    let result = cmd_add(&sw, 300, false);

    // The poll_once error (Error::Parse, from the corrupted .claude.json)
    // is what must be reported -- proving cmd_add actually observed the
    // failure, rather than looping past it or hanging until the 300s
    // timeout this test would otherwise be at the mercy of.
    assert!(
        matches!(result, Err(Error::Parse { .. })),
        "expected the poll_once Parse error to propagate, got: {result:?}"
    );

    // The property that actually matters: abort() ran and put u1 back as
    // the live account, so the user is not left logged out.
    let restored = ClaudeFiles::new(&paths).capture().unwrap().unwrap();
    assert_eq!(restored.email(), Some("a@example.com"));
}
