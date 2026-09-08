//! Desktop notifications.
//!
//! Strictly best-effort. By the time there is something to notify about, the
//! switch it describes has already committed -- so a toast that fails to
//! display must never be turned into an error: doing so would report
//! failure for an operation that, in fact, succeeded. `send` below swallows
//! its error deliberately, logging it instead. This is the one place in
//! byte where that is the correct thing to do; everywhere else, an error
//! must propagate (see the crate root's `Error` enum).

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

/// Show a desktop notification, or log why it could not be shown.
///
/// Returns nothing -- not even a `Result` -- on purpose. See the module
/// comment: a failed toast is never the caller's problem.
pub fn send(title: &str, body: &str) {
    use notify_rust::Notification;

    if let Err(e) = Notification::new().summary(title).body(body).show() {
        output::warn(&format!("could not show a notification: {e}"));
    }
}
