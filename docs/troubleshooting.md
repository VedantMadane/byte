# Troubleshooting

Every error byte can report is listed below, along with what actually
happened and how to recover. All of these come from a single `Error` enum
(`src/error.rs`); the message shown on your terminal is close to verbatim
what's quoted here.

| Symptom | Cause | Fix |
|---|---|---|
| `Claude Code file not found: <path>` | Claude Code isn't installed, or has never logged in, so `.claude.json` or `.credentials.json` doesn't exist yet. Nothing is created or changed. | Install Claude Code and log in at least once, then retry. If you're using `CLAUDE_CONFIG_DIR`, confirm it points at the right directory. |
| `failed to parse <path>: ...` | The file exists but isn't valid JSON — most likely something else wrote to it mid-edit. byte aborts before writing anything rather than overwriting a file it can't fully understand. | Fix or restore the file by hand (see "Recovering from a backup" below), then retry. |
| `no account matching '<name>'` | `<name>` didn't match any stored account's label, email, or UUID prefix. | Run `byte list` to see exact labels, or `byte capture` / `byte add` first if the account isn't stored yet. |
| `'<name>' is ambiguous; it matches N accounts` | `<name>` matched more than one account as a prefix. | Use a longer prefix, or the account's exact label or email. |
| `no Claude account is currently logged in` | `byte capture` (or the first half of `byte add`) ran while Claude Code had no live credentials. | Log in with `claude` first, then retry. |
| the live Claude Code login has credentials but `<path>` has no identifiable account... | `.credentials.json` has real tokens, but `.claude.json` has no `oauthAccount` — typically a login that didn't fully complete. byte refuses to guess and changes nothing. | Relaunch Claude Code and complete the login again, then retry. |
| `unsupported snapshot schema version N; this build expects M` | A stored account was saved by a different, incompatible version of byte. | Update byte, or re-authenticate the account with `byte add` to save it under the current schema. |
| `stored credentials for '<name>' are unusable: ...` | The stored snapshot has no refresh token, or no identity to key it by — it can't be applied safely. | Re-authenticate with `byte add`. |
| `secret store unavailable: ...` | The OS credential store is locked, unreachable, or (on Linux) no Secret Service provider is running. | Unlock your keychain / login session, or start a Secret Service provider (e.g. `gnome-keyring-daemon`), then retry. |
| `write verification failed for <path>; the original was restored from backup` | byte wrote a file, then read it back and it didn't match what was written. byte already restored the pre-write backup automatically — this is reported so you know a write was rejected, not so you have to fix it. | Retry the command. If it fails repeatedly, check disk space and file permissions on the config directory. |
| applying the account snapshot failed, and rolling back `<path>` afterwards also failed | The rarest failure: the config-file half of a switch failed, and restoring the credentials file to its pre-switch state *also* failed. The two files may now disagree about which account is active. | Follow "Recovering from a backup" below for both `.claude.json` and `.credentials.json`, then confirm with `claude` and `byte current` that they agree. |
| `timed out after N seconds waiting for a new login` (`byte add`) | Nobody logged in as a different account within the timeout. byte already restored your previous account before reporting this. | Retry `byte add`, optionally with a longer `--timeout`, and log in via `claude` promptly. |

A few behaviors worth calling out even though they aren't errors:

- **Switching to the already-active account** is a no-op — byte still syncs
  the live credentials back to the store first (in case Claude Code rotated
  the token), then reports that the account was already active.
- **Removing the active account** is allowed. `byte` stops tracking it as
  active, but Claude Code itself is left logged in as that account's
  credentials until you run `byte switch` to something else.
- **Sessions already running.** Claude Code only reads its credentials at
  startup. After every switch, byte prints a reminder that already-running
  `claude` sessions keep using the previous account until restarted — it
  currently prints this unconditionally rather than detecting whether a
  session is actually running.

## Recovering from a backup

byte backs up every file before it writes to it, in
`<byte-config-dir>/backups/` (see [Configuration](configuration.md) for
where that is on your platform). Filenames are `<original-name>.<timestamp>.bak`
— for example `.claude.json.1755400000000.bak` — so sorting the directory
also sorts them chronologically. The ten most recent backups per file are
kept.

Most failures (parse errors, verification failures) are handled
automatically and don't need a manual restore. If you do need one — for
example after an `ApplyRollbackFailed` error, or if a switch just looks
wrong — copy the most recent relevant backup back over the live file. The
examples below assume `BYTE_CONFIG_DIR` is set; if you rely on the platform
default instead, substitute the path from [Configuration](configuration.md).

```sh
# Example: restore .claude.json on Linux/macOS
cp "$BYTE_CONFIG_DIR/backups/.claude.json.<timestamp>.bak" ~/.claude.json
```

```powershell
# Example: restore .claude.json on Windows
Copy-Item "$env:BYTE_CONFIG_DIR\backups\.claude.json.<timestamp>.bak" "$HOME\.claude.json"
```

After restoring, confirm the file still parses (`claude` should start
normally) and that `byte current` reports the account you expect.
