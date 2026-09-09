//! Coverage for the command `Add account…` builds -- the testable half of
//! `src/tray/launch.rs`. `spawn_add` itself is not exercised: it opens a
//! real terminal window on whoever runs the suite. Both platform builders
//! are pure and compile everywhere, so both are asserted here whichever
//! platform the tests run on.

use std::path::Path;

use byte::tray::launch::{macos_script, windows_command_line};

#[test]
fn the_windows_command_runs_byte_add() {
    let line = windows_command_line(Path::new(r"C:\tools\byte.exe"));
    assert!(line.contains(r"C:\tools\byte.exe"), "{line}");
    assert!(line.contains("add"), "{line}");
}

#[test]
fn the_windows_command_survives_a_path_with_spaces() {
    // `cmd /k "..."` strips the outer quote pair, so a quoted program path
    // needs the whole command wrapped in a second pair -- otherwise
    // everything after the first space is parsed as a separate argument and
    // cmd reports that it cannot find `C:\Program`.
    let line = windows_command_line(Path::new(r"C:\Program Files\byte\byte.exe"));
    assert!(
        line.contains(r#"""C:\Program Files\byte\byte.exe" add""#),
        "expected the doubled-quote form cmd /k requires, got: {line}"
    );
}

#[test]
fn the_windows_command_keeps_the_terminal_open() {
    // `/c` would close the window the instant `byte add` returned, taking
    // its result -- "Added work", or an error -- with it.
    let line = windows_command_line(Path::new(r"C:\tools\byte.exe"));
    assert!(line.starts_with("/k"), "expected /k, not /c: {line}");
}

#[test]
fn the_macos_script_opens_terminal_and_runs_byte_add() {
    let script = macos_script(Path::new("/usr/local/bin/byte"));
    assert!(script.contains("Terminal"), "{script}");
    assert!(script.contains("do script"), "{script}");
    assert!(script.contains("/usr/local/bin/byte"), "{script}");
    assert!(script.contains("add"), "{script}");
}

#[test]
fn the_macos_script_quotes_a_path_with_spaces() {
    let script = macos_script(Path::new("/Applications/My Tools/byte"));
    assert!(
        script.contains("'/Applications/My Tools/byte'"),
        "the path must be shell-quoted or the space splits it: {script}"
    );
}

// The safety-critical one. `byte add` logs Claude Code out, and its
// confirmation prompt is the only thing standing between a stray tray click
// and being logged out mid-session. A builder that passed `--yes` would
// skip that prompt and make the click immediately destructive -- which is
// the entire reason the tray spawns a terminal instead of running `add`
// itself.
#[test]
fn neither_platform_skips_the_confirmation_prompt() {
    for command in [
        windows_command_line(Path::new(r"C:\tools\byte.exe")),
        macos_script(Path::new("/usr/local/bin/byte")),
    ] {
        assert!(
            !command.contains("--yes"),
            "the tray must never pass --yes: a stray click would log the user out: {command}"
        );
    }
}
