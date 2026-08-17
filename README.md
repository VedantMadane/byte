# byte

claude account switcher

[![CI](https://github.com/lindstrm/byte/actions/workflows/ci.yml/badge.svg)](https://github.com/lindstrm/byte/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Why?

- Switch between a personal and a work Claude account without logging out
  through the browser and back in again every time.
- Credentials live in your OS's credential store (Windows Credential Manager,
  macOS Keychain, or a Linux Secret Service provider). Pre-write backups are
  plaintext, though — see [Security](SECURITY.md) for exactly where and for
  how long.
- Only the account identity is swapped. Settings, project history, plugins,
  and MCP server tokens are shared across accounts and never touched.
- Every write is backed up first, replaced atomically, and verified
  afterward, so a crash mid-switch can't corrupt `~/.claude.json`.
- Scriptable: every command accepts `--json` for machine-readable output.

## Prerequisites

- Rust 1.88 or later (edition 2024) to build from source — see
  [`rust-toolchain.toml`](rust-toolchain.toml).
- [Claude Code](https://claude.com/claude-code) installed and already logged
  in as at least one account. `byte` reads Claude Code's own credential
  files; it does not perform the OAuth login itself.
- An OS credential store: Windows Credential Manager, the macOS login
  Keychain, or a Secret Service provider on Linux (e.g. GNOME Keyring or
  KWallet).

## Install

```sh
cargo install --path .
```

## Quick start

```sh
# Save the account you're currently logged in as
byte capture

# Log out, log in as a second account, and save it too
byte add

# Optional: accounts are labeled by email by default — give them names you'll
# actually type
byte rename you@personal.example.com personal
byte rename you@work.example.com work

# Switch back and forth by label, email, or UUID prefix
byte switch work
byte switch personal
```

## Usage

| Command | Description |
|---|---|
| `byte` / `byte list` | List stored accounts; the active one is marked with `*` |
| `byte current` | Print the active account's label |
| `byte switch <name>` | Switch to a stored account |
| `byte capture` | Save the currently logged-in account |
| `byte add [--timeout <secs>]` | Log out, then save the next account you log in as (default 300s) |
| `byte remove <name> [--yes]` | Forget a stored account (irreversible; prompts for confirmation unless `--yes` is given) |
| `byte rename <name> <label>` | Change an account's display label |

`<name>` matches a label, an email address, or an account UUID prefix. Every
command accepts `--json` for machine-readable output on stdout; status
messages always go to stderr, so `--json` output can be piped safely.
`byte remove` requires `--yes` under `--json` or when standard input isn't a
terminal, since it can't prompt in either case.

See [`man/byte.md`](man/byte.md) for the full reference, or run `byte --help`.

## Configuration

byte reads two environment variables:

| Variable | Effect |
|---|---|
| `CLAUDE_CONFIG_DIR` | Overrides where Claude Code's `.claude.json` and `.credentials.json` are read from |
| `BYTE_CONFIG_DIR` | Overrides byte's own config directory (`accounts.json`, `backups/`) |

See [Configuration](docs/configuration.md) for default paths per platform and
where credentials are stored in each OS keychain.

## Examples

See [`examples/`](examples/) for runnable demos, including
`roundtrip_check`, which round-trips a real `.claude.json` through byte's
JSON writer and diffs the result — useful for confirming byte preserves a
file it hasn't seen before.

## Troubleshooting

byte never writes over a file it could not parse, and every write is backed
up first — if a switch leaves things looking wrong, your previous file is in
`<config-dir>/backups/`. See [Troubleshooting](docs/troubleshooting.md) for
the full error reference.

## Documentation

- [Getting started](docs/getting-started.md)
- [Configuration](docs/configuration.md)
- [Architecture](docs/architecture.md)
- [Troubleshooting](docs/troubleshooting.md)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under [MIT](LICENSE).