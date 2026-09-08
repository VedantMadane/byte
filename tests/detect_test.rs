use byte::claude::detect::{FakeProbe, ProcessProbe, looks_like_claude_code};

#[test]
fn a_fake_probe_reports_what_it_was_given() {
    assert_eq!(FakeProbe::with_count(0).running_claude_sessions(), 0);
    assert_eq!(FakeProbe::with_count(3).running_claude_sessions(), 3);
}

#[test]
fn a_native_claude_binary_is_recognised() {
    assert!(looks_like_claude_code("claude.exe", &["claude.exe".into()]));
    assert!(looks_like_claude_code("claude", &["claude".into()]));
}

#[test]
fn a_node_process_running_the_claude_cli_is_recognised() {
    // npm installs appear as node with the CLI entry point in argv.
    assert!(looks_like_claude_code(
        "node.exe",
        &[
            "node.exe".into(),
            "C:\\Users\\x\\AppData\\Roaming\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js"
                .into(),
        ]
    ));
}

#[test]
fn an_unrelated_node_process_is_not_recognised() {
    assert!(!looks_like_claude_code(
        "node.exe",
        &["node.exe".into(), "server.js".into()]
    ));
}

#[test]
fn an_unrelated_binary_whose_name_merely_contains_claude_is_not_recognised() {
    // "claude-desktop" is a different product; matching it would make the
    // switch warning fire when nothing relevant is running.
    assert!(!looks_like_claude_code(
        "claude-desktop.exe",
        &["claude-desktop.exe".into()]
    ));
}

#[test]
fn byte_itself_is_never_counted() {
    // The tray is a long-running process; counting it would make every switch
    // warn about a session that is byte's own.
    assert!(!looks_like_claude_code("byte.exe", &["byte.exe".into()]));
}

#[test]
fn an_electron_helper_process_sharing_the_claude_binary_name_is_not_recognised() {
    // On Windows the Anthropic Claude desktop app -- a different product --
    // also ships an executable literally named `claude.exe`. Its internal
    // Chromium/Electron helper processes (renderer, gpu-process, utility,
    // crashpad-handler) are tagged with a `--type=` flag; a real Claude Code
    // CLI invocation never carries one. Observed on a live process table
    // where this shape accounted for 11 of 22 raw matches.
    assert!(!looks_like_claude_code(
        "claude.exe",
        &["claude.exe".into(), "--type=renderer".into()]
    ));
    assert!(!looks_like_claude_code(
        "claude.exe",
        &["claude.exe".into(), "--type=crashpad-handler".into()]
    ));
    assert!(!looks_like_claude_code(
        "claude.exe",
        &["claude.exe".into(), "--type=gpu-process".into()]
    ));
}
