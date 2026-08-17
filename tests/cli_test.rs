use std::process::{Command, Stdio};

use byte::paths::{HostPaths, TestPaths};

/// Runs the real binary against a throwaway config directory.
///
/// `CLAUDE_CONFIG_DIR` places `.claude.json` and `.credentials.json` directly
/// in the given directory — a flatter layout than a real home directory.
/// stdin is explicitly `null` (never a terminal) so every test here is
/// deterministic regardless of how the test harness's own stdin happens to
/// be connected -- this matters in particular for the `remove` confirmation
/// tests below, which assert on non-interactive behavior specifically.
fn byte(tp: &TestPaths, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_byte"))
        .args(args)
        .env("CLAUDE_CONFIG_DIR", tp.root())
        .env("BYTE_CONFIG_DIR", tp.byte_config_dir())
        .stdin(Stdio::null())
        .output()
        .expect("failed to run byte")
}

/// Seed `accounts.json` directly, bypassing `capture`/`add` entirely, so a
/// test can exercise `byte remove` against a resolvable account without
/// ever touching the OS keychain -- `KeyringStore` is real for the compiled
/// binary, and tests must never write to it.
fn seed_one_account(tp: &TestPaths) {
    std::fs::write(
        tp.accounts_file(),
        serde_json::json!({
            "schema": 1,
            "active": null,
            "accounts": [{
                "uuid": "u1",
                "label": "work",
                "email": "w@example.com",
                "organization_name": null,
                "subscription_type": null,
                "added_at": "2026-01-01T00:00:00Z",
                "last_used_at": null
            }]
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn list_on_an_empty_store_succeeds() {
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["list"]);
    assert!(out.status.success());
}

#[test]
fn list_json_on_an_empty_store_is_an_empty_array() {
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["list", "--json"]);

    assert!(out.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(parsed, serde_json::json!([]));
}

#[test]
fn json_output_goes_to_stdout_and_status_text_does_not() {
    // Guards the output split: --json must be pipeable.
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["list", "--json"]);

    assert!(serde_json::from_slice::<serde_json::Value>(&out.stdout).is_ok());
}

#[test]
fn current_json_on_an_empty_store_is_null() {
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["current", "--json"]);

    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "null");
}

#[test]
fn switching_to_an_unknown_account_exits_non_zero() {
    // resolve() fails before anything reaches the keychain.
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["switch", "nobody"]);

    assert!(!out.status.success());
}

#[test]
fn an_unknown_subcommand_exits_non_zero() {
    let tp = TestPaths::new().unwrap();
    assert!(!byte(&tp, &["frobnicate"]).status.success());
}

#[test]
fn help_lists_every_command() {
    // Anchored to the start of a (trimmed) line rather than `text.contains`:
    // clap's per-command help text can itself contain another command's
    // name as a plain substring -- e.g. "current"'s own description reads
    // "Show the active account", but "capture"'s reads "Save the currently
    // logged-in account", which contains "current" -- so a bare `contains`
    // check would still pass with the `Current` subcommand deleted
    // entirely. Every subcommand's own listing line starts with its name.
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["--help"]);
    let text = String::from_utf8_lossy(&out.stdout);

    for cmd in [
        "list", "switch", "add", "capture", "remove", "rename", "current",
    ] {
        assert!(
            text.lines().any(|l| l.trim_start().starts_with(cmd)),
            "help is missing a line starting with '{cmd}':\n{text}"
        );
    }
}

#[test]
fn version_prints_the_crate_version() {
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["--version"]);

    assert!(String::from_utf8_lossy(&out.stdout).contains(env!("CARGO_PKG_VERSION")));
}

// The tests below add coverage beyond the brief, staying inside the same
// scope boundary as the ones above (no secrets, no keychain). Each closes a
// gap where an existing test only checks success()/!success() without
// checking *why* — checking a weaker property than the one it's named for is
// the failure mode this project's reviews flag most often.

#[test]
fn non_json_status_text_never_reaches_stdout() {
    // The mirror image of `json_output_goes_to_stdout_and_status_text_does_not`.
    // That test's own scenario (`list --json` on an empty store) returns
    // before calling any status helper, so it can't actually catch one
    // wired to the wrong stream. This scenario does call one --
    // `output::info` for the empty-store message -- so it can.
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["list"]);

    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "status text leaked onto stdout: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn an_unknown_subcommand_is_a_usage_error_not_an_application_error() {
    // Distinguishes clap's own usage-error exit code (2) from the exit code
    // `main` assigns to an `Err` returned from `run()` (1) -- see
    // `switching_to_an_unknown_account_names_it_in_the_error` below. Pins
    // the exit codes documented in `man/byte.md`.
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["frobnicate"]);

    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn switching_to_an_unknown_account_names_it_in_the_error() {
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["switch", "nobody"]);

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("nobody"),
        "expected the unresolved name in stderr, got:\n{stderr}"
    );
}

#[test]
fn an_absurd_timeout_is_rejected_before_anything_else_runs() {
    // Regression test for task-11 review Finding 7: an unvalidated u64
    // `--timeout` let `Instant::now() + Duration::from_secs(timeout)` panic
    // in cmd_add -- reachable only after AddSession::begin had already
    // logged the user out. The value below comfortably fits in a u64 (so
    // this exercises the new value_parser *range* specifically, not just
    // u64::from_str's own overflow rejection) but is far outside
    // Command::Add's 1..=86_400. clap now rejects it during argument
    // parsing, before Cli::parse() even returns, so this is safe to
    // exercise through the real binary: it never reaches
    // RealPaths::discover(), let alone the keychain. A usage error (exit 2)
    // and a clean stderr message, not a panic (which Command::output()
    // would still report as a non-zero, non-2 exit and a backtrace on
    // stderr), is what proves the fix, so both are checked explicitly.
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["add", "--timeout", "100000000000"]);

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("panicked"),
        "an absurd --timeout must be a clean usage error, not a panic:\n{stderr}"
    );
}

// The tests below cover finding I6 (`byte remove` needing confirmation).
// They seed accounts.json directly rather than going through `capture`/
// `add`, and stop at the confirmation gate rather than passing `--yes` --
// both deliberately, so none of them ever reach manage::remove and its
// secrets().delete() call, which for this compiled binary is a REAL OS
// keychain. `--yes` actually proceeding to a full removal is exactly the
// kind of thing this suite must not exercise end to end (same boundary as
// cmd_add's success path, noted in tests/cli_run_test.rs).

#[test]
fn remove_without_yes_requires_confirmation_on_non_interactive_stdin() {
    let tp = TestPaths::new().unwrap();
    seed_one_account(&tp);

    let out = byte(&tp, &["remove", "work"]);

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--yes"),
        "expected a hint to pass --yes on non-interactive stdin, got:\n{stderr}"
    );
    // The account must still be listed -- proves manage::remove (and so
    // secrets().delete()) never ran.
    let list_out = byte(&tp, &["list", "--json"]);
    let listed: serde_json::Value = serde_json::from_slice(&list_out.stdout).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
}

#[test]
fn remove_json_without_yes_requires_confirmation_and_emits_no_stdout() {
    // --json must never prompt (it would corrupt machine-readable stdout)
    // -- it requires --yes outright, same as non-interactive stdin.
    let tp = TestPaths::new().unwrap();
    seed_one_account(&tp);

    let out = byte(&tp, &["remove", "work", "--json"]);

    assert!(!out.status.success());
    assert!(
        out.stdout.is_empty(),
        "a rejected --json remove must not write partial output to stdout: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn remove_of_an_unknown_account_reports_no_such_account_not_a_confirmation_prompt() {
    // Resolving the name happens before the confirmation gate, so a typo'd
    // name is reported accurately instead of asking the user to confirm
    // removing something that was never going to be removed.
    let tp = TestPaths::new().unwrap();
    seed_one_account(&tp);

    let out = byte(&tp, &["remove", "nobody"]);

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("nobody"),
        "expected the unresolved name in stderr, got:\n{stderr}"
    );
    assert!(
        !stderr.contains("--yes"),
        "an unknown account should not prompt for confirmation:\n{stderr}"
    );
}

#[test]
fn add_timeout_from_a_logged_out_start_does_not_claim_a_false_restoration() {
    // Finding M7: resolve_add_failure (and cmd_add's own pre-abort warning)
    // unconditionally claimed "Restor(ed/ing) the previous account" whenever
    // the abort attempt returned Ok(()) -- but AddSession::abort() ALSO
    // returns Ok(()) when there was no previous account to restore in the
    // first place (previous == None), which is exactly this scenario: never
    // logged in, so `byte add` times out with nothing to put back.
    //
    // Entirely keychain-free: both files are seeded as empty objects (no
    // claudeAiOauth), so capture_current() short-circuits to NotLoggedIn
    // before ever touching the secret store, and abort()'s None branch
    // never calls switch_to. --timeout 1 keeps this fast (one or two
    // 500ms polls) instead of waiting out the 300s default.
    //
    // Written directly under tp.root(), NOT via tp.claude_credentials()/
    // tp.claude_config() -- those give TestPaths's own nested layout
    // (<root>/.claude/.credentials.json), but the byte() helper below runs
    // the compiled binary with CLAUDE_CONFIG_DIR=tp.root(), which resolves
    // to the FLAT layout instead (<root>/.credentials.json,
    // <root>/.claude.json). Using the wrong one here seeds a file the
    // subprocess never reads, and it fails with ClaudeFileMissing instead.
    let tp = TestPaths::new().unwrap();
    std::fs::write(tp.root().join(".credentials.json"), "{}").unwrap();
    std::fs::write(tp.root().join(".claude.json"), "{}").unwrap();

    let out = byte(&tp, &["add", "--timeout", "1"]);

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("Restored the previous account")
            && !stderr.contains("Restoring the previous account"),
        "claimed a restoration that never happened (no previous account existed):\n{stderr}"
    );
    assert!(
        stderr.contains("Nothing to restore"),
        "expected an accurate 'nothing to restore' message, got:\n{stderr}"
    );
}
