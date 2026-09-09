//! Command-line surface.

pub mod run;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "byte",
    version,
    about = "Switch between Claude accounts",
    long_about = "Switch which Claude account Claude Code is authenticated as.\n\
                  Run with no arguments to start the tray icon."
)]
pub struct Cli {
    /// Emit machine-readable JSON on stdout.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List stored accounts.
    List,
    /// Show the active account.
    Current,
    /// Switch to a stored account.
    Switch {
        /// Label, email address, or account UUID prefix.
        name: String,
    },
    /// Save the currently logged-in account.
    Capture,
    /// Log out, then capture the next account you log in as.
    Add {
        /// How long to wait for a login, in seconds (1-86400).
        //
        // The upper bound keeps `Instant::now() + Duration::from_secs(timeout)`
        // in cmd_add (src/cli/run.rs) provably free of overflow -- that
        // addition would otherwise panic on an absurd value, after
        // AddSession::begin has already logged the user out. A plain `//`
        // comment rather than `///`, deliberately: clap renders doc comments
        // into --help text, and this rationale is for maintainers, not CLI
        // users.
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..=86_400))]
        timeout: u64,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Forget a stored account.
    Remove {
        /// Label, email address, or account UUID prefix.
        name: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Change an account's display label.
    Rename {
        /// The account to rename.
        name: String,
        /// The new label.
        label: String,
    },
    /// Start byte's tray automatically at login.
    Autostart {
        #[command(subcommand)]
        action: AutostartAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum AutostartAction {
    /// Register byte to start at login.
    Enable,
    /// Remove byte from login items.
    Disable,
    /// Report whether byte starts at login.
    Status,
}
