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

use byte::Error;
use byte::cli::run::resolve_add_failure;

#[test]
fn reports_the_original_cause_when_the_restore_succeeds() {
    let result = resolve_add_failure(Error::LoginTimeout(5), Ok(()));

    assert!(matches!(result, Err(Error::LoginTimeout(5))));
}

#[test]
fn reports_the_restore_failure_rather_than_the_original_cause_when_both_fail() {
    // This is the failure mode the fix exists to prevent: if only the
    // *original* cause were returned here, the caller (and the user) would
    // never learn that the restore attempt -- their one path back to being
    // logged in -- also failed.
    let result = resolve_add_failure(Error::LoginTimeout(5), Err(Error::NotLoggedIn));

    assert!(matches!(result, Err(Error::NotLoggedIn)));
}

#[test]
fn distinguishes_a_poll_error_cause_from_a_timeout_cause() {
    // Not just "some Err comes back" -- confirms the specific cause passed
    // in is the one that surfaces when the restore succeeds, for the other
    // shape of failure cmd_add can report (a poll_once error, not just a
    // timeout).
    let result = resolve_add_failure(Error::NotLoggedIn, Ok(()));

    assert!(matches!(result, Err(Error::NotLoggedIn)));
}
