% BYTE(1) | User Commands

# NAME

byte - switch between Claude accounts

# SYNOPSIS

**byte** \[**--json**] \[*COMMAND*]

# DESCRIPTION

**byte** switches which Claude account Claude Code is authenticated as. It
stores each account's OAuth credentials in the operating system's credential
store and swaps them into Claude Code's configuration on demand.

Only the authentication identity is swapped. Settings, project history,
plugins, and MCP server tokens are shared across all accounts.

Run with no *COMMAND* to start byte's tray icon, which shows the stored
accounts, marks the active one, and switches on a click. The tray is
available on **Windows and macOS only**; on other platforms, running **byte**
with no arguments prints a message directing you to the commands below
instead of starting anything.

# COMMANDS

**list**
: List stored accounts. The active account is marked with an asterisk.

**current**
: Print the active account's label.

**switch** *NAME*
: Switch to a stored account. *NAME* matches a label, an email address, or an
  account UUID prefix.

**capture**
: Save the currently logged-in account.

**add** \[**--timeout** *SECONDS*]
: Log Claude Code out, then wait for you to log in as a different account and
  save it automatically. Defaults to 300 seconds.

**remove** *NAME* \[**--yes**]
: Forget a stored account, deleting both its metadata and its stored
  credentials. Unlike every other write byte performs, this has no backup and
  cannot be undone. Prompts for confirmation when standard input is a
  terminal and **--json** is not set; otherwise **--yes** is required.

**rename** *NAME* *LABEL*
: Change an account's display label.

**autostart** *ACTION*
: Manage whether byte's tray starts automatically at login. *ACTION* is one
  of **enable**, **disable**, or **status**. Opt-in: byte never registers
  itself at login unless you run **autostart enable**.

# OPTIONS

**--json**
: Emit machine-readable JSON on standard output.

# ENVIRONMENT

**CLAUDE_CONFIG_DIR**
: Directory holding `.claude.json` and `.credentials.json`.

**BYTE_CONFIG_DIR**
: byte's own configuration directory.

# FILES

*accounts.json*
: Account metadata, in byte's configuration directory.

*backups/*
: Timestamped copies made before every write. The ten most recent per file are
  kept.

*mutation.lock*
: Advisory lock held for the duration of a **switch**, **capture**, **add**,
  **remove**, or **rename**. See NOTES.

*tray.lock*
: Advisory lock held for a running tray's entire lifetime. See NOTES.

# EXIT STATUS

**0**
: Success.

**1**
: An operation failed — for example, an unknown account name, a locked or
  unavailable keychain, or a Claude Code file that could not be parsed. The
  cause is printed to standard error.

**2**
: A usage error: an unrecognized command or flag, or a missing required
  argument.

# NOTES

Claude Code reads its credentials at startup, so sessions that are already
running keep the previous account until they are restarted. When a switch
completes, byte reports this only if it actually detects a running Claude
Code session; with none running, it says nothing.

**switch**, **capture**, **add**, **remove**, and **rename** each take a
short-lived advisory lock (`mutation.lock` in byte's configuration directory)
for the duration of the write, so a CLI invocation and a tray-driven switch
can never interleave their writes to the same files. **list**, **current**,
and **autostart** do not take this lock; every file byte writes is replaced
atomically, so a concurrent read is always safe. If another byte process
already holds the lock, the command fails immediately with "another byte
process is currently changing accounts" rather than waiting or retrying;
simply run it again once the other process finishes.

The tray holds a second, longer-lived lock (`tray.lock`) for as long as it
runs, so a second **byte** started with no arguments while one is already
running fails with "byte is already running" instead of opening a duplicate
icon. On a platform other than Windows or macOS, **byte** with no arguments
fails with "the tray is only available on Windows and macOS" and suggests
the CLI commands above instead.
