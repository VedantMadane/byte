//! Executing CLI commands and rendering their results.

use std::io::IsTerminal as _;

use crate::cli::{Cli, Command};
use crate::error::{Error, Result};
use crate::ops::add::AddSession;
use crate::ops::manage::{self, AccountListing};
use crate::ops::switch::{SwitchOutcome, Switcher, SyncOutcome};
use crate::output;
use crate::paths::{HostPaths, RealPaths};
use crate::store::secrets::{KeyringStore, SecretStore};

/// How often the add flow checks for a completed login.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

pub fn run(cli: Cli) -> Result<()> {
    let paths = RealPaths::discover()?;
    let switcher = Switcher::new(&paths, KeyringStore::new());

    match cli.command {
        None | Some(Command::List) => cmd_list(&switcher, cli.json),
        Some(Command::Current) => cmd_current(&switcher, cli.json),
        Some(Command::Switch { name }) => cmd_switch(&switcher, &name, cli.json),
        Some(Command::Capture) => cmd_capture(&switcher, cli.json),
        Some(Command::Add { timeout }) => cmd_add(&switcher, timeout, cli.json),
        Some(Command::Remove { name, yes }) => cmd_remove(&switcher, &name, yes, cli.json),
        Some(Command::Rename { name, label }) => cmd_rename(&switcher, &name, &label, cli.json),
    }
}

fn listing_json(l: &AccountListing) -> serde_json::Value {
    serde_json::json!({
        "label": l.meta.label,
        "email": l.meta.email,
        "organization": l.meta.organization_name,
        "subscription": l.meta.subscription_type,
        "uuid": l.meta.uuid,
        "active": l.active,
        "last_used_at": l.meta.last_used_at,
    })
}

fn cmd_list<P: HostPaths + Copy, S: SecretStore>(sw: &Switcher<P, S>, json: bool) -> Result<()> {
    let listing = manage::list(sw)?;

    if json {
        let payload: Vec<_> = listing.iter().map(listing_json).collect();
        let text =
            serde_json::to_string_pretty(&payload).map_err(|e| Error::Render(e.to_string()))?;
        output::data(&text);
        return Ok(());
    }

    if listing.is_empty() {
        output::info("No accounts stored yet. Run `byte capture` to save the current one.");
        return Ok(());
    }

    output::header("Accounts");
    for l in &listing {
        let mark = if l.active { "*" } else { " " };
        let org = l.meta.organization_name.as_deref().unwrap_or("-");
        output::info(&format!("{mark} {}  ({org})", l.meta.label));
    }
    Ok(())
}

fn cmd_current<P: HostPaths + Copy, S: SecretStore>(sw: &Switcher<P, S>, json: bool) -> Result<()> {
    match manage::current(sw)? {
        Some(meta) if json => {
            output::data(&serde_json::json!({"label": meta.label, "uuid": meta.uuid}).to_string());
        }
        Some(meta) => output::data(&meta.label),
        None if json => output::data("null"),
        None => output::info("No active account."),
    }
    Ok(())
}

fn report_sync(sync: &SyncOutcome) {
    if let SyncOutcome::Captured(meta) = sync {
        output::status(&format!("Saved previously unknown account {}", meta.label));
    }
}

/// Build `switch --json`'s payload. `pub` (like `resolve_add_failure`)
/// specifically so its shape is directly testable without a keychain --
/// see `tests/cli_run_test.rs`.
///
/// Includes `sync`: without it, a script has no way to learn that
/// sync-back just wrote a previously unknown account's refresh token to
/// the keychain (finding M3) -- the non-JSON path already reports this via
/// `report_sync`, but `--json` skipped it entirely.
pub fn switch_json(outcome: &SwitchOutcome) -> serde_json::Value {
    serde_json::json!({
        "switched_to": outcome.switched_to.label,
        "uuid": outcome.switched_to.uuid,
        "already_active": outcome.already_active,
        "sync": sync_json(&outcome.sync),
    })
}

fn sync_json(sync: &SyncOutcome) -> serde_json::Value {
    match sync {
        SyncOutcome::Updated(meta) => {
            serde_json::json!({"outcome": "updated", "label": meta.label, "uuid": meta.uuid})
        }
        SyncOutcome::Captured(meta) => {
            serde_json::json!({"outcome": "captured", "label": meta.label, "uuid": meta.uuid})
        }
        SyncOutcome::LoggedOut => serde_json::json!({"outcome": "logged_out"}),
    }
}

fn cmd_switch<P: HostPaths + Copy, S: SecretStore>(
    sw: &Switcher<P, S>,
    name: &str,
    json: bool,
) -> Result<()> {
    let outcome = sw.switch_to(name)?;

    if json {
        output::data(&switch_json(&outcome).to_string());
        return Ok(());
    }

    let SwitchOutcome {
        switched_to,
        sync,
        already_active,
    } = outcome;

    report_sync(&sync);
    if already_active {
        output::info(&format!("{} is already active.", switched_to.label));
    } else {
        output::status(&format!("Switched to {}", switched_to.label));
        output::warn(
            "Claude Code sessions already running keep the previous account until restarted.",
        );
    }
    Ok(())
}

fn cmd_capture<P: HostPaths + Copy, S: SecretStore>(sw: &Switcher<P, S>, json: bool) -> Result<()> {
    let meta = sw.capture_current()?;
    if json {
        output::data(&serde_json::json!({"captured": meta.label}).to_string());
    } else {
        output::status(&format!("Saved {}", meta.label));
    }
    Ok(())
}

/// `pub` (like `resolve_add_failure`) specifically so its wiring -- does it
/// call `abort()` before reporting a `poll_once` failure, does the poll loop
/// terminate and reach the timeout path -- is directly testable against a
/// `MemoryStore` rather than only through the compiled binary, which would
/// require a real keychain (`capture_current` unconditionally calls
/// `secrets.put`). See `tests/cli_run_test.rs`.
pub fn cmd_add<P: HostPaths + Copy, S: SecretStore>(
    sw: &Switcher<P, S>,
    timeout: u64,
    json: bool,
) -> Result<()> {
    let session = AddSession::begin(sw)?;

    output::status("Claude Code is now logged out.");
    output::info("Run `claude` in another terminal and log in as the account you want to add.");
    output::info(&format!("Waiting up to {timeout} seconds..."));

    // Safe from overflow: clap's value_parser restricts `timeout` to
    // 1..=86_400 (see Command::Add in src/cli/mod.rs), nowhere near a
    // Duration that could push this addition past Instant's range.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    while std::time::Instant::now() < deadline {
        match session.poll_once(sw) {
            Ok(Some(meta)) => {
                if json {
                    output::data(&serde_json::json!({"added": meta.label}).to_string());
                } else {
                    output::status(&format!("Added {}", meta.label));
                }
                return Ok(());
            }
            Ok(None) => {}
            // AddSession::begin has already cleared the live credentials by
            // this point, so this failure must not simply propagate: that
            // would leave the user logged out with no attempt to recover.
            // Mirror the timeout path below -- try to restore first, then
            // decide what to report.
            Err(e) => {
                let restore_result = session.abort(sw);
                return resolve_add_failure(e, restore_result);
            }
        }
        std::thread::sleep(POLL_INTERVAL);
    }

    output::warn("Timed out. Restoring the previous account.");
    let restore_result = session.abort(sw);
    resolve_add_failure(Error::LoginTimeout(timeout), restore_result)
}

/// Decide what `cmd_add` reports after a failure, given the outcome of
/// already having tried to restore the previous account.
///
/// Neither failure may go unreported: if the restore succeeded, the
/// *original* cause (a `poll_once` error or a timeout) is still what the
/// user needs to see, so it becomes the returned `Err`. If the restore also
/// failed, the original cause is printed directly here (it would otherwise
/// vanish -- only one `Err` can be returned) and the restore failure -- the
/// more urgent of the two, since it means the account may not actually have
/// been put back -- becomes the returned `Err`, which `main` prints last.
///
/// Takes the restore attempt's `Result` rather than a `Switcher` and
/// performing it itself, so this decision is unit-testable on its own
/// without a `SecretStore` or any file/keychain I/O -- see
/// `tests/cli_run_test.rs`. `pub` (rather than the other `cmd_*` helpers'
/// default privacy) specifically so those tests can reach it.
pub fn resolve_add_failure(cause: Error, restore_result: Result<()>) -> Result<()> {
    match restore_result {
        Ok(()) => {
            output::status("Restored the previous account.");
            Err(cause)
        }
        Err(abort_err) => {
            output::error(&cause.to_string());
            Err(abort_err)
        }
    }
}

/// `byte remove` is unlike every other write byte performs: the OS keychain
/// entry it deletes has no backup, so a removal is genuinely unrecoverable
/// except by re-authenticating with `byte add`. It must not proceed without
/// explicit confirmation.
fn cmd_remove<P: HostPaths + Copy, S: SecretStore>(
    sw: &Switcher<P, S>,
    name: &str,
    yes: bool,
    json: bool,
) -> Result<()> {
    if !yes {
        // Resolve first, so an unknown name still reports NoSuchAccount
        // rather than demanding confirmation for an account that was never
        // going to be removed anyway.
        let label = sw.load_accounts()?.resolve(name)?.label.clone();

        // --json is for scripts: a prompt would corrupt machine-readable
        // stdout, and would block forever on stdin nobody is watching, so
        // it requires --yes outright instead of prompting. The same applies
        // to any other non-interactive stdin (piped input, cron, CI) even
        // without --json -- prompting there would just hang.
        if json || !std::io::stdin().is_terminal() {
            return Err(Error::ConfirmationRequired {
                action: "byte remove".into(),
            });
        }

        if !output::confirm(&format!(
            "Remove '{label}'? Its stored credentials cannot be recovered afterward."
        )) {
            output::info("Aborted; nothing was removed.");
            return Ok(());
        }
    }

    let meta = manage::remove(sw, name)?;
    if json {
        output::data(&serde_json::json!({"removed": meta.label}).to_string());
    } else {
        output::status(&format!("Removed {}", meta.label));
    }
    Ok(())
}

fn cmd_rename<P: HostPaths + Copy, S: SecretStore>(
    sw: &Switcher<P, S>,
    name: &str,
    label: &str,
    json: bool,
) -> Result<()> {
    let meta = manage::rename(sw, name, label)?;
    if json {
        output::data(&serde_json::json!({"renamed": meta.label}).to_string());
    } else {
        output::status(&format!("Renamed to {}", meta.label));
    }
    Ok(())
}
