# Configuration

byte has no config file of its own to edit. It reads two files that belong to
Claude Code, writes one small metadata file of its own, and stores secrets in
the OS credential store. Every path below can be overridden with an
environment variable for testing or non-standard setups.

## Environment variables

| Variable | Type | Default | Effect |
|---|---|---|---|
| `CLAUDE_CONFIG_DIR` | path | unset | Directory containing Claude Code's `.claude.json` and `.credentials.json`. When set, both files are read directly from this directory (`<dir>/.claude.json`, `<dir>/.credentials.json`) instead of the default locations below. |
| `BYTE_CONFIG_DIR` | path | unset | byte's own configuration directory, holding `accounts.json` and `backups/`. When unset, falls back to the platform default below. |

## Claude Code's files (read and patched, never fully rewritten)

When `CLAUDE_CONFIG_DIR` is not set, byte looks in the same places Claude
Code itself uses:

| File | Default location | What byte reads/writes |
|---|---|---|
| `.claude.json` | `~/.claude.json` | The `oauthAccount` object and the `userID` field. Every other key (settings, project history, plugin usage, onboarding flags — roughly 80 top-level keys on a typical install) is preserved byte-for-byte. |
| `.credentials.json` | `~/.claude/.credentials.json` | The `claudeAiOauth` object. `mcpOAuth` (per-MCP-server tokens) is preserved untouched. |

byte never deserializes either file into a typed struct — it patches specific
keys in a parsed JSON document and re-serializes the rest unchanged,
preserving key order and the original pretty-printed or minified formatting.

## byte's own configuration directory

`accounts.json` (account metadata — label, email, organization, UUID; no
secrets) and `backups/` (see below) live in `BYTE_CONFIG_DIR` if set,
otherwise in the platform default:

| Platform | Default `BYTE_CONFIG_DIR` |
|---|---|
| Windows | `%APPDATA%\byte` |
| macOS | `~/Library/Application Support/byte` |
| Linux / other Unix | `$XDG_CONFIG_HOME/byte`, or `~/.config/byte` if `XDG_CONFIG_HOME` is unset |

Within that directory:

| Path | Contents |
|---|---|
| `accounts.json` | Every stored account's metadata and which one is active. Safe to read directly; contains no secrets. |
| `backups/` | Timestamped copies of `.claude.json`, `.credentials.json`, and `accounts.json`, made before every write. The ten most recent per file are kept. See [Troubleshooting](troubleshooting.md) for how to restore one. |

## Where credentials are stored

Each account's OAuth material (the full `claudeAiOauth` snapshot, refresh
token included) is stored as one entry in the operating system's credential
store, under the service name `byte-claude-account-switcher`:

| Platform | Backend |
|---|---|
| Windows | Windows Credential Manager — look for a generic credential named `byte-claude-account-switcher`. |
| macOS | The login Keychain — search for the service name `byte-claude-account-switcher`. |
| Linux | A Secret Service provider (e.g. GNOME Keyring or KWallet) via `zbus`. |

`accounts.json` never contains a refresh token or access token. The
credential store does — and so, in plaintext, do the `.credentials.json`
backups in `backups/` (above): once a refresh token has been live on this
machine, it persists in one or more backup generations for a while
afterward, independent of the credential store entry. Removing an account
with `byte remove` deletes its metadata entry and its credential store
entry, but does **not** delete its past backups. See
[Security](../SECURITY.md#threat-model) for the full picture of where
credentials rest and what removal does and doesn't clear.
