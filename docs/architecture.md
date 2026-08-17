# Architecture of byte

byte moves a small, self-contained `AccountSnapshot` — one account's OAuth
credentials plus its identity — between Claude Code's live config files and a
per-account store. Everything else in those files is opaque to byte and
passes through untouched.

## Module layout

```
src/
  main.rs          entry point: parse argv, dispatch, map errors to an exit code
  lib.rs           public module tree
  error.rs         Error enum (thiserror) + Result alias, used everywhere
  output.rs        the only place allowed to call println!/eprintln!
  paths.rs         HostPaths trait; RealPaths (env + home dir) and TestPaths (tempdir)
  atomic.rs        atomic file replace, timestamped backup, backup pruning
  claude/
    document.rs    JsonDocument — load/patch/save JSON preserving key order and style
    snapshot.rs    AccountSnapshot type, field accessors, validation
    files.rs       ClaudeFiles — capture/apply/clear a snapshot against the live files
  store/
    metadata.rs    AccountsFile, AccountMeta — accounts.json, name resolution
    secrets.rs     SecretStore trait, KeyringStore (OS keychain), MemoryStore (tests)
  ops/
    switch.rs      Switcher — capture, sync-back, switch (the core algorithm)
    add.rs         AddSession — logout-and-watch add flow
    manage.rs      list / current / rename / remove
  cli/
    mod.rs         clap command definitions (the Cli and Command types)
    run.rs         dispatches a parsed Cli to an op and renders the result
```

`src/tray/*` and `src/claude/detect.rs` (running-process detection) are
deferred to a later plan; nothing in the current tree depends on them.

## Dependency direction

Dependencies flow one way, from the CLI down to the primitives. Nothing in a
lower layer imports from a higher one:

```
main.rs
  └── cli::run
        └── ops (switch, add, manage)
              ├── claude (document, snapshot, files)
              ├── store (metadata, secrets)
              └── paths
error, output, paths, atomic
  are used from every layer above them
```

- **`cli/`** is the only layer that knows about argument parsing, `--json`,
  and human-readable formatting. It calls into `ops` and renders whatever
  comes back; it contains no file I/O or business logic of its own.
- **`ops/`** composes `claude/` and `store/` into the operations the CLI
  exposes (switch, add, list, ...). It is generic over the `HostPaths` and
  `SecretStore` traits rather than depending on their concrete
  implementations, which is what lets the test suite exercise the full
  algorithm against a `TempDir` and an in-memory store with no keychain and
  no real Claude Code installation.
- **`claude/`** knows the shape of Claude Code's two files and how to
  capture and apply an `AccountSnapshot` against them, but nothing about
  where those files live or how accounts are stored between switches.
- **`store/`** knows how to persist account metadata and secrets, but
  nothing about Claude Code's file formats.
- **`error.rs`**, **`output.rs`**, **`paths.rs`**, and **`atomic.rs`** are
  primitives with no dependency on anything above them.

## Major data types

- **`AccountSnapshot`** (`claude/snapshot.rs`) — the unit of account identity
  byte moves around: the `claudeAiOauth` object, the `oauthAccount` object,
  and `userID`, held as opaque `serde_json::Value` so unknown upstream fields
  survive a capture/apply cycle untouched. Short-lived — constructed by a
  capture, consumed by an apply or a store write.
- **`JsonDocument`** (`claude/document.rs`) — a parsed JSON object plus the
  formatting details (pretty vs. compact, trailing newline) needed to write
  it back without producing a spurious diff.
- **`AccountsFile` / `AccountMeta`** (`store/metadata.rs`) — the persisted,
  non-secret half of an account: label, email, organization, UUID, and which
  account is currently active. This is what `accounts.json` serializes to
  and what `byte list` reads; it never touches the keychain.
- **`Switcher<P, S>`** (`ops/switch.rs`) — generic over `P: HostPaths` and
  `S: SecretStore`, constructed once per CLI invocation and passed by
  reference into every op. This is the seam that makes the algorithm
  testable: production code instantiates `Switcher<&RealPaths,
  KeyringStore>`, tests instantiate `Switcher<&TestPaths, MemoryStore>`.

## Command flow, end to end

`byte switch work`:

1. `main.rs` parses argv into a `Cli` via `clap::Parser`.
2. `cli::run::run` builds a `Switcher<&RealPaths, KeyringStore>` from
   `RealPaths::discover()` (honoring `CLAUDE_CONFIG_DIR` / `BYTE_CONFIG_DIR`)
   and dispatches on the parsed `Command`.
3. `cmd_switch` calls `Switcher::switch_to("work")`, which: resolves `"work"`
   against `accounts.json` before touching anything; syncs the currently
   live account back to the store so a token Claude Code rotated isn't lost;
   reads the target account's snapshot from `SecretStore`; applies it to the
   live files via `ClaudeFiles::apply` (credentials written first, config
   second, with a credentials rollback if the config write fails); and
   updates `accounts.json` to mark the new account active.
4. `cmd_switch` renders the `SwitchOutcome` — either as JSON on stdout
   (`--json`) or as status/warning lines on stderr via `output.rs`.
5. `main` maps `Ok`/`Err` to a process exit code, printing any error through
   `output::error`.

## Cross-cutting concerns

- **Errors.** A single `thiserror` enum (`error.rs`) covers every fallible
  path in the crate. Ops functions return `Result<T, Error>`; `main.rs` is
  the only place an `Error` is turned into user-facing text and an exit
  code.
- **Output.** All user-facing text goes through `output.rs`'s semantic
  helpers (`status`, `warn`, `info`, `header`, `error`, `confirm` on stderr;
  `data` on stdout). No other module calls `println!`/`eprintln!` directly,
  which is what keeps `--json` output pipeable — machine-readable data never
  shares a stream with a status message.
- **Configuration.** Two environment variables, both resolved once in
  `paths::RealPaths::discover()`; see [Configuration](configuration.md).
- **Safety.** Every write to a Claude Code file goes through
  `atomic.rs` (temp file in the same directory, fsync, atomic rename) and is
  preceded by a timestamped backup and followed by a read-back verification,
  regardless of which op triggered it.
