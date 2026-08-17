//! Executing CLI commands and rendering their results.

use crate::cli::{Cli, Command};
use crate::error::{Error, Result};
use crate::ops::add::AddSession;
use crate::ops::manage::{self, AccountListing};
use crate::ops::switch::{SwitchOutcome, Switcher, SyncOutcome};
use crate::output;
use crate::paths::RealPaths;
use crate::store::secrets::KeyringStore;

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
        Some(Command::Add { timeout }) => cmd_add(&switcher, timeout),
        Some(Command::Remove { name }) => cmd_remove(&switcher, &name),
        Some(Command::Rename { name, label }) => cmd_rename(&switcher, &name, &label),
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

fn cmd_list(sw: &Switcher<&RealPaths, KeyringStore>, json: bool) -> Result<()> {
    let listing = manage::list(sw)?;

    if json {
        let payload: Vec<_> = listing.iter().map(listing_json).collect();
        output::data(&serde_json::to_string_pretty(&payload).unwrap_or_default());
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

fn cmd_current(sw: &Switcher<&RealPaths, KeyringStore>, json: bool) -> Result<()> {
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

fn cmd_switch(sw: &Switcher<&RealPaths, KeyringStore>, name: &str, json: bool) -> Result<()> {
    let SwitchOutcome {
        switched_to,
        sync,
        already_active,
    } = sw.switch_to(name)?;

    if json {
        output::data(
            &serde_json::json!({
                "switched_to": switched_to.label,
                "uuid": switched_to.uuid,
                "already_active": already_active,
            })
            .to_string(),
        );
        return Ok(());
    }

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

fn cmd_capture(sw: &Switcher<&RealPaths, KeyringStore>, json: bool) -> Result<()> {
    let meta = sw.capture_current()?;
    if json {
        output::data(&serde_json::json!({"captured": meta.label}).to_string());
    } else {
        output::status(&format!("Saved {}", meta.label));
    }
    Ok(())
}

fn cmd_add(sw: &Switcher<&RealPaths, KeyringStore>, timeout: u64) -> Result<()> {
    let session = AddSession::begin(sw)?;

    output::status("Claude Code is now logged out.");
    output::info("Run `claude` in another terminal and log in as the account you want to add.");
    output::info(&format!("Waiting up to {timeout} seconds..."));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    while std::time::Instant::now() < deadline {
        if let Some(meta) = session.poll_once(sw)? {
            output::status(&format!("Added {}", meta.label));
            return Ok(());
        }
        std::thread::sleep(POLL_INTERVAL);
    }

    output::warn("Timed out. Restoring the previous account.");
    session.abort(sw)?;
    Err(Error::LoginTimeout(timeout))
}

fn cmd_remove(sw: &Switcher<&RealPaths, KeyringStore>, name: &str) -> Result<()> {
    let meta = manage::remove(sw, name)?;
    output::status(&format!("Removed {}", meta.label));
    Ok(())
}

fn cmd_rename(sw: &Switcher<&RealPaths, KeyringStore>, name: &str, label: &str) -> Result<()> {
    let meta = manage::rename(sw, name, label)?;
    output::status(&format!("Renamed to {}", meta.label));
    Ok(())
}
