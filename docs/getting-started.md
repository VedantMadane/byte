# Getting started with byte

> claude account switcher

## Install

```sh
cargo install --path .
```

This builds `byte` from source and installs it to `~/.cargo/bin` (make sure
that directory is on your `PATH`). Rust 1.88 or later is required — see
[`rust-toolchain.toml`](../rust-toolchain.toml).

byte does not perform the OAuth login itself. It reads and writes the same
files Claude Code already uses, so you need Claude Code installed and logged
in as at least one account before you start.

## First run

This walkthrough takes you from a single logged-in Claude Code session to
switching between two accounts.

### 1. Save the account you're currently logged in as

Log in to Claude Code as usual (`claude`), then run:

```sh
byte capture
```

This reads the live credentials from `~/.claude/.credentials.json` and
`~/.claude.json`, saves them into your OS credential store, and records the
account's metadata in byte's own `accounts.json`. Nothing about your Claude
Code login changes — `capture` only takes a copy.

Confirm it landed:

```sh
byte list
```

You should see one account, marked active with `*`.

### 2. Add a second account

```sh
byte add
```

This logs Claude Code out and waits for you to log in again. In another
terminal, run `claude` and complete the login flow for your second account —
personal, work, or whichever one you want to add. As soon as byte detects a
new, different account has finished logging in, it saves it automatically and
you're done. If nothing logs in within the timeout (5 minutes by default,
override with `--timeout <seconds>`), byte restores the account you started
with and exits with an error, so you're never left logged out.

Run `byte list` again — you should now see two accounts.

### 3. Switch between them

```sh
byte switch <name>
```

`<name>` matches a label, an email address, or an account UUID prefix, so
`byte switch work` and `byte switch you@work.example.com` both work once
you've captured that account. Switching writes the target account's
credentials into Claude Code's live files; the account you switch away from
is saved first, so any token Claude Code rotated behind byte's back isn't
lost.

Restart any already-running `claude` session after switching — Claude Code
only reads its credentials at startup, so a session that's already running
keeps using the previous account until you restart it.

### Optional: give an account a friendlier name

Accounts are labeled from their email address by default. To rename one:

```sh
byte rename <name> <new-label>
```

## Next steps

- [Configuration reference](configuration.md)
- [Architecture overview](architecture.md)
- [Troubleshooting](troubleshooting.md)
