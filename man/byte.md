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
running keep the previous account until they are restarted.
