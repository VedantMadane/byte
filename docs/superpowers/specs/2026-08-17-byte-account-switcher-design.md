# byte — Claude account switcher

**Status:** approved design
**Date:** 2026-08-17

## 1. Purpose

`byte` lets a user who legitimately holds more than one Claude account —
typically a personal account and a work/organization account — switch which one
Claude Code is authenticated as, from a system tray menu or a headless CLI.

Today that requires logging out and logging back in through the browser. `byte`
reduces it to one click by storing each account's OAuth material and swapping it
in and out of the two files Claude Code reads at startup.

### Non-goals

- Switching the Claude desktop app or browser sessions. Claude Code only.
- Implementing the OAuth flow. `byte` never talks to Anthropic's auth servers;
  it only moves credentials that Claude Code itself obtained.
- Isolating settings, project history, plugins, or MCP tokens per account.
  Those stay shared. See §4.
- Concurrent use of two accounts at once. One account is active at a time.

## 2. Background: where Claude Code keeps account state

Two files, both under the user's home directory.

### `~/.claude/.credentials.json`

```json
{
  "claudeAiOauth": {
    "accessToken": "...",
    "refreshToken": "...",
    "expiresAt": 1234567890,
    "scopes": ["...", "..."],
    "subscriptionType": "...",
    "rateLimitTier": "..."
  },
  "mcpOAuth": { "<server-key>": { ... } }
}
```

`claudeAiOauth` is the Claude account itself. `mcpOAuth` holds per-MCP-server
tokens and is **not** account-scoped for our purposes (§4).

On macOS this data lives in the login Keychain as a generic-password item rather
than in this file. That is the one place the platforms genuinely diverge, and it
is why credential access sits behind a trait (§6.2).

### `~/.claude.json`

A large document — on the author's machine, 109 KB and ~80 top-level keys — of
which exactly two are account-scoped:

- `oauthAccount` — a 19-field object: `accountUuid`, `emailAddress`,
  `organizationUuid`, `organizationName`, `displayName`, `billingType`,
  `organizationRole`, rate-limit tiers, and related profile fields.
- `userID`

Everything else in that file is machine- or user-scoped: onboarding flags,
feature caches, per-project history (86 entries), plugin usage, tip counters.
**Preserving it byte-for-byte is a hard requirement** (§5).

## 3. Surfaces

One binary, two front ends, one shared core library.

- `byte` with no arguments → tray icon.
- `byte <subcommand>` → headless CLI:

  | Command | Behavior |
  |---|---|
  | `byte list` | Stored accounts, active one marked |
  | `byte current` | The active account |
  | `byte switch <name>` | Switch to a stored account |
  | `byte add` | Log out and capture the next login (§8) |
  | `byte capture` | Snapshot the currently logged-in account |
  | `byte remove <name>` | Forget a stored account |
  | `byte rename <name> <label>` | Change an account's display label |

  `<name>` matches against label, then email, then account UUID prefix.
  All commands accept `--json` for scripting.

The tray is menu-only: the account list with a checkmark on the active entry,
then Add account, a Remove submenu, and Quit. There is no settings window.

Accounts are labeled automatically from `oauthAccount.emailAddress`, which is
already the natural identifier. `byte rename` covers the case where a user wants
something friendlier. This deliberately avoids a native text-input dialog, which
has no good cross-platform answer.

## 4. What a switch swaps

**In scope** — the auth identity, and only that:

- `claudeAiOauth` in `.credentials.json`
- `oauthAccount` and `userID` in `.claude.json`

**Out of scope, explicitly shared across accounts:** `mcpOAuth`, settings,
project history, plugins, and every other key in either file.

Rationale: the smallest blast radius that still produces a correct switch. A
user who wants full profile isolation is better served by `CLAUDE_CONFIG_DIR`,
which Claude Code already supports; `byte` does not attempt to duplicate it.

## 5. Data-preservation requirements

Rewriting `~/.claude.json` incorrectly would destroy a large amount of unrelated
user state. These requirements are not optional and each has a corresponding
test (§10).

1. **No typed round-trip.** Reads produce a `serde_json::Value`. `byte` patches
   the specific keys it owns and re-serializes the original document. It must
   never deserialize into a struct that could drop unknown fields.
2. **Atomic replacement.** Write to a temporary file in the same directory,
   flush and fsync, then atomically replace the target (`MoveFileExW` with
   `MOVEFILE_REPLACE_EXISTING` on Windows, `rename(2)` elsewhere). Never open
   the real file with truncate.
3. **Backup before first write.** On the first modification of each file,
   copy it to `<config-dir>/backups/<name>.<timestamp>.json`. Retain the ten
   most recent per file.
4. **Fail closed on unparseable input.** If either file fails to parse, abort
   the operation and report it. Never write over a file we could not read.
5. **Post-write verification.** Re-read the written file, confirm it parses and
   that the patched keys hold the intended values. On mismatch, restore from the
   backup taken in (3) and report failure.
6. **Preserve each file's formatting.** The two files are written differently by
   Claude Code, as verified on 2026-08-17:

   | File | Style |
   |---|---|
   | `~/.claude.json` | Pretty-printed, two-space indent |
   | `~/.claude/.credentials.json` | Minified, single line |

   Writing either in the other's style would produce a whole-file diff on every
   switch. `byte` detects the existing style on read — whether a newline follows
   the opening brace — and reproduces it on write, rather than hard-coding a
   style per path, so an upstream change in either direction is absorbed
   automatically.

## 6. Architecture

### 6.1 Module layout

```
src/
  main.rs              arg dispatch: no args → tray, else CLI
  lib.rs               public re-exports
  error.rs             Error enum (thiserror)
  paths.rs             locate Claude files + byte config dir; env overrides
  atomic.rs            atomic replace + backup helper
  claude/
    credentials.rs     read/patch claudeAiOauth
    config.rs          read/patch oauthAccount + userID
    snapshot.rs        AccountSnapshot; capture() / apply()
    detect.rs          running-process probe
  store/
    metadata.rs        accounts.json
    secrets.rs         SecretStore trait + keyring impl
  ops/
    add.rs  switch.rs  login.rs  remove.rs  list.rs
  cli/
    mod.rs  run.rs
  tray/
    menu.rs  events.rs  notify.rs
```

Dependency direction is strictly downward: `tray` and `cli` depend on `ops`;
`ops` depends on `claude` and `store`; those depend on `paths`, `atomic`, and
`error`. Nothing depends upward.

Per `AGENTS.md`, non-test sources stay under 1000 lines; the layout above targets
200–400 lines per file.

### 6.2 Trait seams

Four traits make everything except the tray event loop testable with no GUI, no
keychain, and no Claude installation:

| Trait | Production | Test |
|---|---|---|
| `HostPaths` | Real home directory | `tempfile::TempDir` |
| `CredentialBackend` | JSON file (Win/Linux), Keychain (macOS) | In-memory |
| `SecretStore` | `keyring` v3 | In-memory `HashMap` |
| `ProcessProbe` | `sysinfo` | Scripted fake |

`ProcessProbe` must handle both install methods, since `.claude.json` records an
`installMethod` that varies per machine: a native install appears as a `claude`
/ `claude.exe` process, while an npm install appears as a `node` process whose
command line references the Claude Code entry point. The probe is a heuristic
and is never allowed to fail a switch — it only drives the warning in §9.

### 6.3 Storage

Secrets — the `claudeAiOauth` object — go to the OS keychain via `keyring` v3:
Windows Credential Manager, macOS Keychain, Linux Secret Service. One entry per
account, keyed by `accountUuid`.

Non-secret metadata goes to `<config-dir>/accounts.json`:

```json
{
  "schema": 1,
  "active": "<account-uuid>",
  "accounts": [
    {
      "uuid": "...",
      "label": "hello@example.com",
      "email": "hello@example.com",
      "organization_name": "...",
      "organization_uuid": "...",
      "subscription_type": "...",
      "added_at": "2026-08-17T10:00:00Z",
      "last_used_at": "2026-08-17T12:00:00Z"
    }
  ]
}
```

`<config-dir>` is the platform config directory via `directories::ProjectDirs`,
overridable with `BYTE_CONFIG_DIR` (required for tests). The resolved path is
documented in `docs/configuration.md`.

### 6.4 Dependencies

`tray-icon` + `muda` + `winit` (tray and menus), `keyring` v3 (secrets), `clap`
v4 derive (CLI), `serde`/`serde_json`, `sysinfo` (process probe), `notify-rust`
(notifications), `directories` (paths), `tempfile` (atomic writes, test
fixtures), `thiserror` (library errors), `anyhow` (binary errors), `time` or
`jiff` (timestamps).

`tray-icon` requires an event loop and, on macOS, main-thread execution; on Linux
it requires GTK. `winit` normalizes this.

## 7. The switch algorithm

```
switch(target):
  1. read the live snapshot from disk
  2. sync-back:
       resolve live snapshot to a stored account by accountUuid,
         falling back to emailAddress
       found   → overwrite the stored copy
       unknown → auto-capture it as a new stored account
  3. load the target snapshot (metadata + keychain secret)
  4. validate: refreshToken non-empty, expiresAt parses, schema version known
  5. atomic-patch .credentials.json  → claudeAiOauth only
  6. atomic-patch .claude.json       → oauthAccount + userID only
  7. update metadata: active, last_used_at
  8. probe for running `claude` processes; notify if any exist
```

### 7.1 Why sync-back exists

Claude Code refreshes its access token in the background using the refresh
token. If Anthropic rotates refresh tokens on use, a stored copy goes stale as
soon as Claude Code refreshes it — and writing a rotated-out refresh token back
later would silently log the user out.

Step 2 therefore always re-reads live credentials into the store *before*
overwriting them. Sync-back also runs when the tray menu is opened, so the store
stays fresh during a long session without a background watcher.

Whether Anthropic actually rotates refresh tokens is unverified. Sync-back is
correct either way; Phase 0 confirms it empirically.

### 7.2 Why unknown accounts are auto-captured

If a user logs in fresh without capturing, and then switches, the un-captured
login would be destroyed permanently. Auto-capture makes that impossible.

The trade-off is that `byte` may write a refresh token to the keychain without an
explicit user action. This is accepted: it is strictly safer than the
alternative, and it is the expected behavior of a tool the user installed
specifically to manage account credentials.

## 8. The add flow

`/login` is a REPL slash command, not a CLI subcommand, so there is no headless
way to invoke it. `byte add` therefore:

1. Syncs back the current account **and verifies it reads back out of the store
   intact**. If that verification fails, abort — never clear credentials that
   are not provably saved.
2. Clears `claudeAiOauth` and `oauthAccount`, putting Claude Code in a
   logged-out state.
3. Attempts to spawn a terminal running `claude`. If that fails for any reason,
   degrade to a notification instructing the user to log in manually.
4. Polls `.credentials.json` every 500 ms for up to 5 minutes, waiting for a
   `claudeAiOauth` whose `accountUuid` differs from the one just cleared.
5. On detection, captures it into the store, marks it active, and notifies.

If the timeout expires, `byte` restores the previously active account so the
user is never left logged out by an abandoned add.

## 9. Errors and edge cases

| Condition | Behavior |
|---|---|
| Claude Code not installed / files absent | Clear message naming the expected paths; exit non-zero. No files created. |
| Either file unparseable | Abort before any write (§5.4). |
| Keychain locked or unavailable | Report the platform-specific cause; no partial switch. |
| Stored snapshot fails validation | Refuse to apply; suggest `byte add` to re-authenticate that account. |
| Unrecognized credential schema version | Refuse to apply; report the version mismatch. |
| Switch to the already-active account | No-op with a message; still performs sync-back. |
| Removing the active account | Allowed, with confirmation; Claude Code is left logged in until the next switch. |
| Running `claude` processes detected | Switch proceeds; notification states that N running sessions still use the previous account. |
| Post-write verification fails | Restore from backup, report failure. |

Errors are a `thiserror` enum in the library, surfaced with `anyhow` context in
the binary. No error path silently swallows a failure.

## 10. Testing

Target: 80% coverage across everything except `tray/`.

**Unit** — path resolution, atomic replace (including the crash-midway case),
snapshot capture/apply, metadata serialization, name resolution.

**The preservation test is the most important test in the repo.** Take a
realistic `.claude.json` fixture with many unrelated keys, apply a snapshot, and
assert the output is byte-identical except `oauthAccount` and `userID`. The same
test exists for `.credentials.json` with respect to `mcpOAuth`.

**Integration** — a full add → switch → switch-back cycle against a `TempDir`
home with an in-memory `SecretStore`, asserting the live files end in exactly
the expected state and that sync-back captured a mutated refresh token.

**Manual** — tray behavior per platform, recorded in the PR description. Windows
is verified before release; macOS and Linux are marked unverified until someone
runs them.

Per `AGENTS.md`, all tests live in `tests/` in files with a `_test` suffix. No
inline `#[cfg(test)]` modules.

## 11. Phasing

| Phase | Deliverable |
|---|---|
| 0 | Research and reuse: survey prior art, verify `tray-icon` and `keyring` behavior on Windows, confirm refresh-token rotation empirically |
| 1 | Core: paths, atomic writes, preserving read/patch, snapshot capture/apply |
| 2 | Store: metadata + keychain, in-memory test implementation |
| 3 | Ops: add, switch, remove, list, sync-back |
| 4 | CLI, man page, docs — **fully working headless switcher** |
| 5 | Tray: icon, menu, events, notifications |
| 6 | Process detection and warnings |
| 7 | Packaging, release artifacts, README, website |

Phases 1–4 produce a usable product before any GUI code exists. That is
deliberate: it keeps the risky, data-touching work under test and away from the
untestable parts.

## 12. Risks

1. **macOS is unverified.** Credentials live in the Keychain there, not in a
   file, so `CredentialBackend` has a genuinely different implementation on that
   platform. It ships written but untested until someone runs it.
2. **Schema drift.** Claude Code can change its credential format at any time.
   Snapshots carry a schema version and `byte` refuses to apply one it does not
   recognize, rather than writing something malformed.
3. **Rotation semantics are assumed.** See §7.1.
4. **No lock protocol** exists between `byte` and Claude Code. Both can write
   `.credentials.json`. Atomic replacement plus post-write verification narrows
   the window; it cannot close it. The running-session warning is the
   user-facing acknowledgment of this limit.
5. **Refresh tokens at rest.** The OS keychain is a meaningful improvement over
   the plaintext file Claude Code already uses, but these are live credentials.
   `SECURITY.md` gets an honest threat-model note: anyone who can run code as
   the user can read them, and `byte` does not change that.
6. **Undocumented surface.** Both files are internal to Claude Code. This design
   depends on their current shape and accepts that upstream may change it
   without notice; risk 2 is the mitigation.

## 13. Documentation

Per the sync points in `AGENTS.md`:

- `README.md` — value propositions, install, quick start
- `docs/getting-started.md` — first run, adding a second account
- `docs/configuration.md` — config paths, `BYTE_CONFIG_DIR`, keychain entries
- `docs/architecture.md` — module layout and dependency direction from §6
- `docs/troubleshooting.md` — the table in §9, plus recovering from a backup
- `man/byte.md` — the command surface from §3
- `SECURITY.md` — the threat-model note from risk 5
