use std::process::Command;

use byte::paths::{HostPaths, TestPaths};

/// Runs the real binary against a throwaway config directory.
///
/// `CLAUDE_CONFIG_DIR` places `.claude.json` and `.credentials.json` directly
/// in the given directory — a flatter layout than a real home directory.
fn byte(tp: &TestPaths, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_byte"))
        .args(args)
        .env("CLAUDE_CONFIG_DIR", tp.root())
        .env("BYTE_CONFIG_DIR", tp.byte_config_dir())
        .output()
        .expect("failed to run byte")
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
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["--help"]);
    let text = String::from_utf8_lossy(&out.stdout);

    for cmd in [
        "list", "switch", "add", "capture", "remove", "rename", "current",
    ] {
        assert!(text.contains(cmd), "help is missing '{cmd}':\n{text}");
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
