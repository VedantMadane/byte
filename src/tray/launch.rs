//! Opening a terminal that runs `byte add`.
//!
//! The tray cannot run `byte add` itself: it logs Claude Code out and then
//! waits for an interactive login, which needs a console to prompt in and a
//! human to answer. So the menu item hands off to a terminal that has both.
//!
//! Nothing here passes `--yes`, and that omission is load-bearing. `byte
//! add`'s confirmation prompt is the only thing between a mis-aimed click
//! in the notification area and being logged out mid-session, so the
//! spawned terminal must arrive at that prompt and stop. The click opens a
//! window; the human in front of it decides whether anything happens.
//!
//! The two command builders below are pure and compile on every platform so
//! both stay under test everywhere (see `tests/tray_launch_test.rs`); only
//! `spawn_add` is gated to the platforms that can actually run them.

use std::path::Path;

use crate::error::{Error, Result};

/// Wrap a string so a POSIX shell sees it as one argument.
///
/// Single quotes rather than double: inside single quotes a shell expands
/// nothing, so a path containing `$`, backticks, or spaces cannot turn into
/// something else on the way to `do script`. The `'\''` dance is the
/// standard way to embed a literal single quote, there being no escape for
/// one inside a single-quoted string.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The `cmd.exe` command line that runs `byte add` in a new console.
///
/// `/k`, not `/c`: the window has to outlive the command so whoever
/// answered the prompt can read how it went -- `/c` would close it the
/// instant `byte add` returned, taking "Added work" or an error with it.
///
/// The doubled quotes are not a typo. `cmd /k "..."` strips the outermost
/// pair of quotes, so a program path that needs quoting (`C:\Program
/// Files\...`) must be wrapped in a second pair or cmd consumes the wrong
/// one and reports that it cannot find `C:\Program`. Windows forbids `"` in
/// paths, so there is no further quoting case to handle.
pub fn windows_command_line(exe: &Path) -> String {
    format!(r#"/k ""{}" add""#, exe.display())
}

/// The AppleScript that runs `byte add` in a new Terminal window.
///
/// Double-escaped by necessity: the path is first quoted for the shell that
/// `do script` hands it to, then the whole command is escaped for the
/// AppleScript string literal it sits inside.
pub fn macos_script(exe: &Path) -> String {
    let command = format!("{} add", shell_quote(&exe.to_string_lossy()));
    let escaped = command
        .replace('\\', r"\\")
        .replace('"', "\\\"")
        .replace('\n', r"\n")
        .replace('\r', r"\r");
    format!(
        "tell application \"Terminal\"\n\
         activate\n\
         do script \"{escaped}\"\n\
         end tell"
    )
}

/// byte's own executable, which is what the spawned terminal must run.
///
/// Resolved rather than assumed: `byte` is not necessarily on the spawned
/// shell's `PATH` (it commonly isn't, when the tray was started from a
/// build directory), and the terminal must run *this* build in any case.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn current_exe() -> Result<std::path::PathBuf> {
    std::env::current_exe()
        .map_err(|e| Error::Tray(format!("could not locate byte's own executable: {e}")))
}

/// Open a terminal running `byte add`.
///
/// Returns as soon as the terminal is launched -- it is not waited on. The
/// account it adds reaches the tray's menu the same way a CLI `byte add`
/// already does: `accounts.json` changes and the watcher rebuilds.
#[cfg(target_os = "windows")]
pub fn spawn_add() -> Result<()> {
    use std::os::windows::process::CommandExt as _;

    // The tray is a console-subsystem binary and already owns a console, so
    // a plain spawn would write into the terminal the tray was started from
    // rather than a window of its own -- and `byte add`'s prompt would be
    // read from a stdin nobody is typing at.
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

    let exe = current_exe()?;
    // `raw_arg`, not `arg`: Rust would quote the whole thing as a single
    // argument, defeating the deliberate quoting `windows_command_line`
    // builds for cmd's own parser.
    // `%COMSPEC%` rather than a bare "cmd", which would be resolved
    // through `PATH`. This process guards credentials; it should not launch
    // whatever `cmd` a caller-controlled `PATH` happens to find first.
    let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
    std::process::Command::new(shell)
        .raw_arg(windows_command_line(&exe))
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map_err(|e| Error::Tray(format!("could not open a terminal: {e}")))?;
    Ok(())
}

/// Open a terminal running `byte add`. See the Windows version above.
#[cfg(target_os = "macos")]
pub fn spawn_add() -> Result<()> {
    let exe = current_exe()?;
    // `status()`, not `spawn()`. `osascript` returns as soon as Terminal
    // has been told to run the script -- it does not wait for `byte add` --
    // so waiting costs nothing here and is the only way to find out it
    // failed. The first "Add account…" click triggers a TCC Automation
    // consent prompt for controlling Terminal; deny it and osascript exits
    // non-zero with nothing on screen, and returning `Ok` would have the
    // tray cheerfully tell the user to go answer a prompt that does not
    // exist. Waiting also reaps the child, which `spawn` alone left as a
    // zombie per click for the life of the tray. Absolute path so the
    // binary cannot be resolved through a caller-controlled `PATH`.
    let status = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(macos_script(&exe))
        .status()
        .map_err(|e| Error::Tray(format!("could not open a terminal: {e}")))?;

    if !status.success() {
        return Err(Error::Tray(format!(
            "could not open a terminal: osascript exited with {status}. If macOS asked              for permission to control Terminal and it was denied, re-enable it under              System Settings > Privacy & Security > Automation."
        )));
    }
    Ok(())
}

/// Unreachable in practice -- only `tray::app` calls this and it does not
/// build here -- but present so this module compiles everywhere its pure
/// halves are tested, and fails honestly rather than by absence.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn spawn_add() -> Result<()> {
    Err(Error::Tray(
        "opening a terminal is only supported on Windows and macOS; run `byte add` yourself."
            .to_string(),
    ))
}
