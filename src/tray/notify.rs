//! Desktop notifications.
//!
//! Strictly best-effort. By the time there is something to notify about, the
//! switch it describes has already committed -- so a toast that fails to
//! display must never be turned into an error: doing so would report
//! failure for an operation that, in fact, succeeded. `send` below swallows
//! its error deliberately, logging it instead. This is the one place in
//! byte where that is the correct thing to do; everywhere else, an error
//! must propagate (see the crate root's `Error` enum).
//!
//! "Best-effort" is weaker than a returned `Ok` makes it look: `.show()`
//! succeeding means the OS *accepted* the toast, not that anyone saw it.
//! Measured on Windows 11, byte's toasts were accepted, written to the
//! notification database, and displayed to nobody -- with no error to log.
//! A toast is therefore never the only channel a tray action speaks on; see
//! [`send`].

use crate::cli::run::running_sessions_warning;
use crate::ops::switch::SwitchOutcome;
use crate::output;

/// Title and body for a completed switch.
///
/// Delegates the running-sessions wording to
/// [`running_sessions_warning`](crate::cli::run::running_sessions_warning)
/// rather than restating its zero/one/many boundary here, so the tray
/// notification and `byte switch`'s terminal warning can never drift apart.
pub fn switch_message(outcome: &SwitchOutcome, running_sessions: usize) -> (String, String) {
    let label = &outcome.switched_to.label;

    if outcome.already_active {
        return (
            format!("Already on {label}"),
            "No change was needed.".to_string(),
        );
    }

    let body = running_sessions_warning(running_sessions)
        .unwrap_or_else(|| "Claude Code will use this account from now on.".to_string());

    (format!("Switched to {label}"), body)
}

/// The one line a notification mirrors to stderr.
///
/// Split out from [`send`] so it can be asserted: `send` writes to a real
/// stderr and fires a real toast, and a test can read back neither.
pub fn console_line(title: &str, body: &str) -> String {
    format!("{title}: {body}")
}

/// Show a desktop notification, and always mirror it to stderr.
///
/// Returns nothing -- not even a `Result` -- on purpose. See the module
/// comment: a failed toast is never the caller's problem.
///
/// The stderr mirror is unconditional, and comes first. A toast the OS
/// accepts is not a toast the user sees, and that gap reports as success:
/// on Windows every one of byte's toasts was accepted and shown to nobody,
/// so `send` returned having logged nothing at all. Without this line a
/// tray action -- a failed switch included -- is indistinguishable from a
/// dead menu item, which is exactly how it was first reported. byte is a
/// console-subsystem binary, so the line lands in the terminal the tray was
/// launched from; started from autostart there is no console attached and
/// it costs nothing.
pub fn send(title: &str, body: &str) {
    use notify_rust::Notification;

    output::info(&console_line(title, body));

    if let Err(e) = Notification::new().summary(title).body(body).show() {
        output::warn(&format!("could not show a notification: {e}"));
    }
}
