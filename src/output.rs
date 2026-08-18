//! Central output module for byte (§19.4).
//!
//! All user-facing messages route through the semantic helpers below so that
//! formatting and routing can change in one place. Raw `println!`/`eprintln!`
//! must not appear outside this module.

use std::io::Write as _;

/// Success message.
pub fn status(msg: &str) {
    let _ = writeln!(std::io::stderr(), "✓  {msg}");
}

/// Warning message.
pub fn warn(msg: &str) {
    let _ = writeln!(std::io::stderr(), "!  {msg}");
}

/// Informational message.
pub fn info(msg: &str) {
    let _ = writeln!(std::io::stderr(), "{msg}");
}

/// Bold section header.
pub fn header(msg: &str) {
    let _ = writeln!(std::io::stderr(), "== {msg} ==");
}

/// Error message.
pub fn error(msg: &str) {
    let _ = writeln!(std::io::stderr(), "✗  {msg}");
}

/// Machine-readable output. This is the only helper that writes to stdout,
/// so `--json` can be piped without status messages contaminating it.
pub fn data(text: &str) {
    let _ = writeln!(std::io::stdout(), "{text}");
}

/// Ask a yes/no question on stderr and read the answer from stdin.
///
/// Generic so any command that adds a destructive or irreversible action can
/// reuse it, rather than each one rolling its own prompt. Returns `false`
/// for anything other than an explicit "y" or "yes" (case insensitive) --
/// including a read failure -- so a broken, closed, or non-interactive
/// stdin defaults to declining rather than proceeding. Callers that cannot
/// tell whether stdin is actually interactive (see `std::io::IsTerminal`)
/// should not call this at all; an unattended prompt would hang forever.
pub fn confirm(prompt: &str) -> bool {
    let _ = write!(std::io::stderr(), "{prompt} [y/N] ");
    let _ = std::io::stderr().flush();

    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}
