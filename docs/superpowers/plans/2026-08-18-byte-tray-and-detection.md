# byte Tray and Process Detection Implementation Plan (Phases 5–6)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the working headless switcher into a tray application — an icon with an account menu, one-click switching, real running-session detection, and opt-in autostart.

**Architecture:** `byte` with no arguments launches a tray icon; every existing subcommand stays headless and unchanged. The tray and the CLI cooperate through the filesystem rather than IPC: a short-lived **mutation lock** serialises writes between the two processes, and the tray **watches `accounts.json`** so a CLI-driven switch refreshes its menu. A separate long-held lock permits only one tray at a time. Menu construction and event→action mapping are pure functions, so the untestable part is confined to the event loop itself.

**Tech Stack:** `tray-icon` 0.24, `winit` 0.30, `fs4` (advisory file locks), `notify` (filesystem watching), `notify-rust` (desktop notifications), `sysinfo` (process detection).

**Spec:** [`docs/superpowers/specs/2026-08-17-byte-account-switcher-design.md`](../specs/2026-08-17-byte-account-switcher-design.md) — phases 5–6 of §11, plus §6.1's `tray/` and `detect.rs` module layout and §9's running-session row.

## Global Constraints

Every task's requirements implicitly include this section.

- **Rust 1.88.0, edition 2024.** Pinned by `rust-toolchain.toml`. Do not change.
- **`serde_json` keeps both `preserve_order` and `float_roundtrip`.** Removing either silently corrupts `~/.claude.json`.
- **No raw `println!` / `eprintln!` outside `src/output.rs`** (OSS_SPEC §19.4). Machine-readable output goes to stdout via `output::data`; everything else to stderr.
- **All tests live in `tests/`** in files whose stem matches `_?[Tt]ests?$`. **Never** inline `#[cfg(test)]` modules.
- **Assert identity, not cardinality.** `assert_eq!(remaining.len(), 3)` passes whichever three survive. Assert *which*.
- **Pin the error variant, not `.is_err()`.** Use `assert!(matches!(err, Error::TheVariant { .. }))`.
- **No test writes to a real home directory or a real credential store.** Use `TestPaths` and `MemoryStore`.
- **Non-test source files stay under 1000 physical lines.**
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --all -- --check` must pass before every commit.
- **Conventional Commits.**
- `.map(Clone::clone)` trips clippy's `map_clone` under `-D warnings` — use `.cloned()`.
- `make` may not be on PATH; run the underlying cargo commands directly.

### Constraints specific to this plan

These four came out of a throwaway spike that built a working tray on Windows 11. Each is invisible until you hit it:

1. **Build the tray inside `ApplicationHandler::resumed()`**, not before `run_app`. Required on macOS, harmless on Windows. Building it early ships a Windows-only tray.
2. **`event_loop.exit()` does not stop callbacks immediately** — `about_to_wait` fires again before the loop terminates. Every handler must be idempotent.
3. **Menu and tray events do not arrive through winit's event enum.** They come from global receivers (`MenuEvent::receiver()`, `TrayIconEvent::receiver()`). Wire them to an `EventLoopProxy` via `set_event_handler` so the loop stays idle, rather than polling on a timer.
4. **`TrayIconEvent` is a pointer stream, not a click stream.** A single hover produced ~100 `Move` events in the spike. Filter to `Click`/`DoubleClick` *before* doing any work, or one hover triggers a hundred file reads.

Two more, from the product:

5. **The tray renders account labels, not emails.** Users rename accounts (`personal`, `work`); the menu must show what `byte list` shows, with the organization as secondary text.
6. **Notifications are best-effort and must never fail an operation.** A failed toast is logged, never propagated.

---

## File Structure

| File | Responsibility |
|---|---|
| `src/lock.rs` | `MutationGuard` (short-lived, shared by tray and CLI) and `InstanceGuard` (long-held, one tray only). |
| `src/claude/detect.rs` | `ProcessProbe` trait, `SysinfoProbe`, `FakeProbe`. Answers "is Claude Code running?" |
| `src/tray/mod.rs` | `run()` — event loop setup, `ApplicationHandler`, lifecycle. The one untestable file. |
| `src/tray/menu.rs` | `MenuModel` — pure construction of menu structure from `Vec<AccountListing>`. |
| `src/tray/events.rs` | `Action` enum and pure `event → Action` mapping, including the `Move` filter. |
| `src/tray/notify.rs` | Best-effort desktop notifications. |
| `src/tray/watch.rs` | Watches `accounts.json`, debounced, signals the event loop. |
| `src/autostart.rs` | `enable` / `disable` / `status`, per platform. |

**Modified:** `src/lib.rs` (module declarations), `src/cli/mod.rs` (`Autostart` subcommand), `src/cli/run.rs` (no-args → tray; lock acquisition; real detection in the switch warning), `Cargo.toml`.

**Deferred to a later plan (phase 7):** packaging, release artifacts, installers, code signing, website.

---

### Task 1: Mutation lock and single-instance guard

Foundation for tray/CLI cooperation. Nothing else in this plan is safe without it.

**Files:**
- Create: `src/lock.rs`
- Create: `tests/lock_test.rs`
- Modify: `src/lib.rs`, `Cargo.toml`

**Interfaces:**
- Consumes: `HostPaths` (`byte_config_dir()`), `Error`, `Result`.
- Produces: `MutationGuard::acquire(&impl HostPaths) -> Result<MutationGuard>`, `MutationGuard::try_acquire(&impl HostPaths) -> Result<Option<MutationGuard>>`; `InstanceGuard::acquire(&impl HostPaths) -> Result<Option<InstanceGuard>>`. Both release on drop.

- [ ] **Step 1: Add the dependency**

```bash
cargo add fs4 --features sync
```

- [ ] **Step 2: Write the failing test**

Create `tests/lock_test.rs`:

```rust
use byte::lock::{InstanceGuard, MutationGuard};
use byte::paths::TestPaths;

#[test]
fn a_mutation_guard_can_be_acquired_when_free() {
    let tp = TestPaths::new().unwrap();
    let guard = MutationGuard::try_acquire(&tp).unwrap();
    assert!(guard.is_some());
}

#[test]
fn a_second_mutation_guard_is_refused_while_the_first_is_held() {
    let tp = TestPaths::new().unwrap();
    let first = MutationGuard::try_acquire(&tp).unwrap();
    assert!(first.is_some(), "first acquisition should succeed");

    let second = MutationGuard::try_acquire(&tp).unwrap();
    assert!(
        second.is_none(),
        "a second guard must be refused while the first is alive"
    );
}

#[test]
fn dropping_a_mutation_guard_releases_it() {
    let tp = TestPaths::new().unwrap();
    {
        let _held = MutationGuard::try_acquire(&tp).unwrap();
    }
    let after = MutationGuard::try_acquire(&tp).unwrap();
    assert!(after.is_some(), "the lock must be free after the guard drops");
}

#[test]
fn an_instance_guard_admits_only_one_holder() {
    let tp = TestPaths::new().unwrap();
    let first = InstanceGuard::acquire(&tp).unwrap();
    assert!(first.is_some());

    let second = InstanceGuard::acquire(&tp).unwrap();
    assert!(second.is_none(), "only one tray instance may hold this");
}

#[test]
fn the_two_locks_are_independent() {
    // A running tray holds an InstanceGuard for its whole life. A CLI command
    // must still be able to take the mutation lock -- that is the entire point
    // of using two files rather than one.
    let tp = TestPaths::new().unwrap();
    let _instance = InstanceGuard::acquire(&tp).unwrap().expect("instance");

    let mutation = MutationGuard::try_acquire(&tp).unwrap();
    assert!(
        mutation.is_some(),
        "holding the instance lock must not block mutations"
    );
}

#[test]
fn lock_files_live_in_the_byte_config_dir() {
    use byte::paths::HostPaths;
    let tp = TestPaths::new().unwrap();
    let _guard = MutationGuard::try_acquire(&tp).unwrap().expect("guard");
    assert!(tp.byte_config_dir().join("mutation.lock").exists());
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --test lock_test`
Expected: FAIL — `unresolved import byte::lock`.

- [ ] **Step 4: Write the implementation**

Create `src/lock.rs`:

```rust
//! Advisory locks that let a tray and a CLI process share byte's state.
//!
//! Two distinct locks, deliberately in separate files:
//!
//! - [`MutationGuard`] is taken for the duration of a single write sequence
//!   (switch, capture, add, remove, rename) by whichever process is doing it.
//!   It stops a tray-driven switch interleaving with a CLI-driven one.
//! - [`InstanceGuard`] is held for a tray's entire lifetime, so a second tray
//!   refuses to start. It must not block CLI mutations, which is why it is a
//!   different file.
//!
//! Both are advisory OS locks released by the kernel if the process dies, so a
//! crash cannot leave byte permanently wedged.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use fs4::fs_std::FileExt;

use crate::error::{Error, Result};
use crate::paths::HostPaths;

const MUTATION_LOCK: &str = "mutation.lock";
const INSTANCE_LOCK: &str = "tray.lock";

fn open_lock_file(dir: &Path, name: &str) -> Result<File> {
    std::fs::create_dir_all(dir).map_err(|source| Error::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let path = dir.join(name);
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(|source| Error::Io { path, source })
}

/// Held while a single write sequence runs. Released on drop.
#[derive(Debug)]
pub struct MutationGuard {
    _file: File,
    path: PathBuf,
}

impl MutationGuard {
    /// Take the lock, or return `Ok(None)` if another process holds it.
    pub fn try_acquire(paths: &impl HostPaths) -> Result<Option<Self>> {
        let dir = paths.byte_config_dir();
        let file = open_lock_file(&dir, MUTATION_LOCK)?;
        let path = dir.join(MUTATION_LOCK);
        match file.try_lock_exclusive() {
            Ok(true) => Ok(Some(Self { _file: file, path })),
            Ok(false) => Ok(None),
            Err(source) => Err(Error::Io { path, source }),
        }
    }

    /// Take the lock, or fail with a message naming who to wait for.
    pub fn acquire(paths: &impl HostPaths) -> Result<Self> {
        match Self::try_acquire(paths)? {
            Some(guard) => Ok(guard),
            None => Err(Error::Busy),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Held for a tray's whole lifetime so only one tray runs. Released on drop.
#[derive(Debug)]
pub struct InstanceGuard {
    _file: File,
}

impl InstanceGuard {
    /// Take the lock, or return `Ok(None)` if a tray is already running.
    pub fn acquire(paths: &impl HostPaths) -> Result<Option<Self>> {
        let dir = paths.byte_config_dir();
        let file = open_lock_file(&dir, INSTANCE_LOCK)?;
        let path = dir.join(INSTANCE_LOCK);
        match file.try_lock_exclusive() {
            Ok(true) => Ok(Some(Self { _file: file })),
            Ok(false) => Ok(None),
            Err(source) => Err(Error::Io { path, source }),
        }
    }
}
```

Add to `src/error.rs`:

```rust
    #[error(
        "another byte process is currently changing accounts.\n\
         Wait for it to finish and try again."
    )]
    Busy,
```

Add `pub mod lock;` to `src/lib.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test lock_test`
Expected: 6 passed.

If `try_lock_exclusive` does not return `Result<bool>` in the resolved `fs4`, adapt to whatever it does return (older versions returned `Result<()>` with a distinguishable would-block error) and record the deviation in your report.

- [ ] **Step 6: Add the troubleshooting row**

Add a row to `docs/troubleshooting.md` for `Error::Busy` — the table claims to list every error byte can report. Cause: another byte process (often the tray) is mid-write. Fix: retry.

- [ ] **Step 7: Commit**

```bash
git add src/lock.rs src/error.rs src/lib.rs tests/lock_test.rs docs/troubleshooting.md Cargo.toml Cargo.lock
git commit -m "feat: add mutation and single-instance advisory locks"
```

---

### Task 2: Process detection

Phase 6. Spec §9's running-session row is currently satisfied by an unconditional warning; this makes it truthful.

**Files:**
- Create: `src/claude/detect.rs`
- Create: `tests/detect_test.rs`
- Modify: `src/claude/mod.rs`, `Cargo.toml`

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: `trait ProcessProbe { fn running_claude_sessions(&self) -> usize; }`, `SysinfoProbe::new()`, `FakeProbe::with_count(usize)`.

- [ ] **Step 1: Add the dependency**

```bash
cargo add sysinfo
```

- [ ] **Step 2: Write the failing test**

Create `tests/detect_test.rs`. `SysinfoProbe` is not asserted against a live machine — the count depends on what the developer happens to have open. Only its classifier is tested, through a pure helper.

```rust
use byte::claude::detect::{FakeProbe, ProcessProbe, looks_like_claude_code};

#[test]
fn a_fake_probe_reports_what_it_was_given() {
    assert_eq!(FakeProbe::with_count(0).running_claude_sessions(), 0);
    assert_eq!(FakeProbe::with_count(3).running_claude_sessions(), 3);
}

#[test]
fn a_native_claude_binary_is_recognised() {
    assert!(looks_like_claude_code("claude.exe", &["claude.exe".into()]));
    assert!(looks_like_claude_code("claude", &["claude".into()]));
}

#[test]
fn a_node_process_running_the_claude_cli_is_recognised() {
    // npm installs appear as node with the CLI entry point in argv.
    assert!(looks_like_claude_code(
        "node.exe",
        &[
            "node.exe".into(),
            "C:\\Users\\x\\AppData\\Roaming\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js".into(),
        ]
    ));
}

#[test]
fn an_unrelated_node_process_is_not_recognised() {
    assert!(!looks_like_claude_code(
        "node.exe",
        &["node.exe".into(), "server.js".into()]
    ));
}

#[test]
fn an_unrelated_binary_whose_name_merely_contains_claude_is_not_recognised() {
    // "claude-desktop" is a different product; matching it would make the
    // switch warning fire when nothing relevant is running.
    assert!(!looks_like_claude_code(
        "claude-desktop.exe",
        &["claude-desktop.exe".into()]
    ));
}

#[test]
fn byte_itself_is_never_counted() {
    // The tray is a long-running process; counting it would make every switch
    // warn about a session that is byte's own.
    assert!(!looks_like_claude_code("byte.exe", &["byte.exe".into()]));
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --test detect_test`
Expected: FAIL — `unresolved import byte::claude::detect`.

- [ ] **Step 4: Write the implementation**

Create `src/claude/detect.rs`:

```rust
//! Answering "is Claude Code running right now?"
//!
//! Claude Code reads its credentials once at startup, so a switch does not
//! affect a session that is already running. byte warns about that -- but a
//! warning shown when nothing is running is noise, so the count has to be
//! real.
//!
//! This is deliberately a heuristic and is never allowed to fail an
//! operation: a probe that errors reports zero.

/// Something that can count running Claude Code sessions.
pub trait ProcessProbe: Send + Sync {
    fn running_claude_sessions(&self) -> usize;
}

/// Decide whether one process is a Claude Code session.
///
/// Two install shapes exist: a native binary named `claude`/`claude.exe`, and
/// an npm install that runs as `node` with the CLI entry point in its
/// arguments. Exposed separately from the probe so it can be tested without a
/// live process table.
pub fn looks_like_claude_code(process_name: &str, argv: &[String]) -> bool {
    let name = process_name.to_ascii_lowercase();
    let stem = name.strip_suffix(".exe").unwrap_or(&name);

    if stem == "claude" {
        return true;
    }

    if stem == "node" || stem == "node.js" {
        return argv.iter().any(|a| {
            let a = a.replace('\\', "/").to_ascii_lowercase();
            a.contains("@anthropic-ai/claude-code") || a.ends_with("/claude-code/cli.js")
        });
    }

    false
}

/// Counts sessions from the real process table.
#[derive(Debug, Default)]
pub struct SysinfoProbe;

impl SysinfoProbe {
    pub fn new() -> Self {
        Self
    }
}

impl ProcessProbe for SysinfoProbe {
    fn running_claude_sessions(&self) -> usize {
        use sysinfo::{ProcessRefreshKind, RefreshKind, System};

        let system = System::new_with_specifics(
            RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
        );

        system
            .processes()
            .values()
            .filter(|p| {
                let name = p.name().to_string_lossy();
                let argv: Vec<String> = p
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy().into_owned())
                    .collect();
                looks_like_claude_code(&name, &argv)
            })
            .count()
    }
}

/// A probe that reports a fixed count, for tests.
#[derive(Debug, Clone, Copy)]
pub struct FakeProbe {
    count: usize,
}

impl FakeProbe {
    pub fn with_count(count: usize) -> Self {
        Self { count }
    }
}

impl ProcessProbe for FakeProbe {
    fn running_claude_sessions(&self) -> usize {
        self.count
    }
}
```

Add `pub mod detect;` to `src/claude/mod.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test detect_test`
Expected: 6 passed.

If `sysinfo`'s API differs from the calls above (it has changed shape across releases), adapt and note the deviation. The trait and `looks_like_claude_code` must keep their signatures — later tasks depend on them.

- [ ] **Step 6: Sanity-check against reality**

Not a test — a one-off check that the classifier finds something real:

```bash
cargo run --example probe_check
```

Create `examples/probe_check.rs`:

```rust
//! Prints how many Claude Code sessions the probe currently sees.
fn main() {
    use byte::claude::detect::{ProcessProbe, SysinfoProbe};
    println!("running Claude Code sessions: {}", SysinfoProbe::new().running_claude_sessions());
}
```

Run it with Claude Code open and again with it closed; the number must differ. Report both. If it reports 0 with Claude Code running, the classifier is wrong — fix it before moving on, and say what the real process looked like.

- [ ] **Step 7: Commit**

```bash
git add src/claude/detect.rs src/claude/mod.rs tests/detect_test.rs examples/probe_check.rs Cargo.toml Cargo.lock
git commit -m "feat: detect running Claude Code sessions"
```

---

### Task 3: Use real detection in the CLI switch warning

**Files:**
- Modify: `src/cli/run.rs`
- Modify: `tests/cli_test.rs`

**Interfaces:**
- Consumes: `ProcessProbe`, `SysinfoProbe` (Task 2).
- Produces: no new public API; `cmd_switch` now warns conditionally.

- [ ] **Step 1: Write the failing test**

`cmd_switch`'s success path needs a keychain, so this is asserted at the level of the message builder rather than through the binary. Add to `tests/cli_run_test.rs`:

```rust
use byte::cli::run::running_sessions_warning;

#[test]
fn no_warning_when_nothing_is_running() {
    assert_eq!(running_sessions_warning(0), None);
}

#[test]
fn one_session_is_described_in_the_singular() {
    let msg = running_sessions_warning(1).expect("a warning");
    assert!(msg.contains('1'), "should name the count: {msg}");
    assert!(!msg.contains("sessions"), "should be singular: {msg}");
}

#[test]
fn several_sessions_are_described_in_the_plural() {
    let msg = running_sessions_warning(3).expect("a warning");
    assert!(msg.contains('3'), "should name the count: {msg}");
    assert!(msg.contains("sessions"), "should be plural: {msg}");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test cli_run_test`
Expected: FAIL — `running_sessions_warning` not found.

- [ ] **Step 3: Write the implementation**

Add to `src/cli/run.rs`:

```rust
/// The warning shown after a switch, or `None` when nothing is running.
///
/// Claude Code reads credentials at startup, so a session that is already
/// running keeps the previous account until it restarts.
pub fn running_sessions_warning(count: usize) -> Option<String> {
    match count {
        0 => None,
        1 => Some(
            "1 running Claude Code session still uses the previous account. \
             Restart it to pick up the switch."
                .to_string(),
        ),
        n => Some(format!(
            "{n} running Claude Code sessions still use the previous account. \
             Restart them to pick up the switch."
        )),
    }
}
```

Then in `cmd_switch`, replace the unconditional warning with:

```rust
        if let Some(msg) = running_sessions_warning(probe.running_claude_sessions()) {
            output::warn(&msg);
        }
```

Thread a `&impl ProcessProbe` into `cmd_switch` from `run()`, constructing `SysinfoProbe::new()` there. Keep `cmd_switch` generic over the probe so a test can pass a `FakeProbe`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test cli_run_test && cargo test`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/cli/run.rs tests/cli_run_test.rs
git commit -m "feat: warn about running sessions only when some are running"
```

---

### Task 4: Menu model

Pure construction of the tray menu's structure. No `tray-icon` types — this is what makes the menu testable.

**Files:**
- Create: `src/tray/mod.rs` (declaring only `menu` for now)
- Create: `src/tray/menu.rs`
- Create: `tests/tray_menu_test.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `AccountListing` (`src/ops/manage.rs`).
- Produces: `MenuModel { entries: Vec<MenuEntry> }`, `MenuEntry::{Account { uuid, label, detail, active }, Separator, AddAccount, Quit}`, `MenuModel::from_listing(&[AccountListing]) -> MenuModel`.

- [ ] **Step 1: Write the failing test**

Create `tests/tray_menu_test.rs`:

```rust
use byte::ops::manage::AccountListing;
use byte::store::metadata::AccountMeta;
use byte::tray::menu::{MenuEntry, MenuModel};

fn listing(uuid: &str, label: &str, org: Option<&str>, active: bool) -> AccountListing {
    AccountListing {
        meta: AccountMeta {
            uuid: uuid.to_string(),
            label: label.to_string(),
            email: Some(format!("{label}@example.com")),
            organization_name: org.map(str::to_string),
            subscription_type: Some("max".to_string()),
            account: serde_json::json!({"accountUuid": uuid}),
            user_id: Some("uid".to_string()),
            credential_schema: 1,
            added_at: "2026-01-01T00:00:00Z".to_string(),
            last_used_at: None,
        },
        active,
    }
}

#[test]
fn an_empty_store_offers_only_add_and_quit() {
    let model = MenuModel::from_listing(&[]);
    assert_eq!(
        model.entries,
        vec![MenuEntry::AddAccount, MenuEntry::Separator, MenuEntry::Quit]
    );
}

#[test]
fn accounts_render_by_label_not_email() {
    // Users rename accounts; the menu must show what `byte list` shows.
    let model = MenuModel::from_listing(&[listing("u1", "work", Some("Indicio"), true)]);
    let MenuEntry::Account { label, detail, .. } = &model.entries[0] else {
        panic!("first entry should be an account, got {:?}", model.entries[0]);
    };
    assert_eq!(label, "work");
    assert_eq!(detail.as_deref(), Some("Indicio"));
}

#[test]
fn exactly_the_active_account_is_marked() {
    let model = MenuModel::from_listing(&[
        listing("u1", "personal", None, false),
        listing("u2", "work", None, true),
    ]);
    let marked: Vec<&str> = model
        .entries
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Account { uuid, active: true, .. } => Some(uuid.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(marked, vec!["u2"], "only u2 should be marked active");
}

#[test]
fn account_order_from_the_listing_is_preserved() {
    let model = MenuModel::from_listing(&[
        listing("u1", "alpha", None, false),
        listing("u2", "beta", None, false),
    ]);
    let uuids: Vec<&str> = model
        .entries
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Account { uuid, .. } => Some(uuid.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(uuids, vec!["u1", "u2"]);
}

#[test]
fn a_separator_divides_accounts_from_the_actions() {
    let model = MenuModel::from_listing(&[listing("u1", "work", None, true)]);
    assert_eq!(
        model.entries,
        vec![
            MenuEntry::Account {
                uuid: "u1".to_string(),
                label: "work".to_string(),
                detail: None,
                active: true,
            },
            MenuEntry::Separator,
            MenuEntry::AddAccount,
            MenuEntry::Separator,
            MenuEntry::Quit,
        ]
    );
}

#[test]
fn an_account_with_no_organization_has_no_detail_line() {
    let model = MenuModel::from_listing(&[listing("u1", "solo", None, false)]);
    let MenuEntry::Account { detail, .. } = &model.entries[0] else {
        panic!("expected an account entry");
    };
    assert_eq!(*detail, None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test tray_menu_test`
Expected: FAIL — `unresolved import byte::tray`.

- [ ] **Step 3: Write the implementation**

Create `src/tray/mod.rs`:

```rust
//! The tray application.

pub mod menu;
```

Create `src/tray/menu.rs`:

```rust
//! The tray menu's structure, independent of any GUI toolkit.
//!
//! Kept free of `tray-icon` types on purpose: menu content is the part worth
//! testing, and the event loop is the part that cannot be.

use crate::ops::manage::AccountListing;

/// One row of the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    Account {
        uuid: String,
        /// The user-facing name -- the label, never the email address.
        label: String,
        /// Secondary text, currently the organization name.
        detail: Option<String>,
        active: bool,
    },
    Separator,
    AddAccount,
    Quit,
}

/// The whole menu, in display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    pub entries: Vec<MenuEntry>,
}

impl MenuModel {
    /// Build the menu from what `byte list` would show.
    pub fn from_listing(listing: &[AccountListing]) -> Self {
        let mut entries: Vec<MenuEntry> = listing
            .iter()
            .map(|l| MenuEntry::Account {
                uuid: l.meta.uuid.clone(),
                label: l.meta.label.clone(),
                detail: l.meta.organization_name.clone(),
                active: l.active,
            })
            .collect();

        if !entries.is_empty() {
            entries.push(MenuEntry::Separator);
        }
        entries.push(MenuEntry::AddAccount);
        entries.push(MenuEntry::Separator);
        entries.push(MenuEntry::Quit);

        Self { entries }
    }
}
```

Add `pub mod tray;` to `src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test tray_menu_test`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add src/tray/ src/lib.rs tests/tray_menu_test.rs
git commit -m "feat: add the tray menu model"
```

---

### Task 5: Event mapping

The `Move`-flood filter (constraint 4) lives here, tested without a GUI.

**Files:**
- Create: `src/tray/events.rs`
- Create: `tests/tray_events_test.rs`
- Modify: `src/tray/mod.rs`

**Interfaces:**
- Consumes: `MenuModel`, `MenuEntry` (Task 4).
- Produces: `Action::{SwitchTo(String), AddAccount, Quit, Ignore}`, `action_for_menu_id(&MenuModel, &[String], &str) -> Action`, `is_actionable_tray_event(TrayEventKind) -> bool`, `TrayEventKind::{Click, DoubleClick, Move, Enter, Leave}`.

- [ ] **Step 1: Write the failing test**

Create `tests/tray_events_test.rs`:

```rust
use byte::tray::events::{Action, TrayEventKind, action_for_menu_id, is_actionable_tray_event};
use byte::tray::menu::{MenuEntry, MenuModel};

fn model() -> MenuModel {
    MenuModel {
        entries: vec![
            MenuEntry::Account {
                uuid: "u1".into(),
                label: "personal".into(),
                detail: None,
                active: true,
            },
            MenuEntry::Account {
                uuid: "u2".into(),
                label: "work".into(),
                detail: None,
                active: false,
            },
            MenuEntry::Separator,
            MenuEntry::AddAccount,
            MenuEntry::Separator,
            MenuEntry::Quit,
        ],
    }
}

/// Menu item ids assigned in the same order as `model()`'s entries.
fn ids() -> Vec<String> {
    vec![
        "id-u1".into(),
        "id-u2".into(),
        "id-sep1".into(),
        "id-add".into(),
        "id-sep2".into(),
        "id-quit".into(),
    ]
}

#[test]
fn clicking_an_account_switches_to_that_account() {
    assert_eq!(
        action_for_menu_id(&model(), &ids(), "id-u2"),
        Action::SwitchTo("u2".into())
    );
}

#[test]
fn clicking_the_already_active_account_still_maps_to_a_switch() {
    // switch_to is idempotent and still performs sync-back, so the tray does
    // not need to special-case it.
    assert_eq!(
        action_for_menu_id(&model(), &ids(), "id-u1"),
        Action::SwitchTo("u1".into())
    );
}

#[test]
fn clicking_add_maps_to_add() {
    assert_eq!(action_for_menu_id(&model(), &ids(), "id-add"), Action::AddAccount);
}

#[test]
fn clicking_quit_maps_to_quit() {
    assert_eq!(action_for_menu_id(&model(), &ids(), "id-quit"), Action::Quit);
}

#[test]
fn an_unknown_id_is_ignored_rather_than_guessed() {
    // A stale id can arrive after the menu is rebuilt. Guessing would switch
    // the user to an arbitrary account.
    assert_eq!(action_for_menu_id(&model(), &ids(), "id-gone"), Action::Ignore);
}

#[test]
fn a_separator_id_is_ignored() {
    assert_eq!(action_for_menu_id(&model(), &ids(), "id-sep1"), Action::Ignore);
}

#[test]
fn pointer_motion_is_not_actionable() {
    // A single hover produced roughly a hundred Move events in the spike.
    // Treating them as actionable would trigger a hundred file reads.
    assert!(!is_actionable_tray_event(TrayEventKind::Move));
    assert!(!is_actionable_tray_event(TrayEventKind::Enter));
    assert!(!is_actionable_tray_event(TrayEventKind::Leave));
}

#[test]
fn clicks_are_actionable() {
    assert!(is_actionable_tray_event(TrayEventKind::Click));
    assert!(is_actionable_tray_event(TrayEventKind::DoubleClick));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test tray_events_test`
Expected: FAIL — `unresolved import byte::tray::events`.

- [ ] **Step 3: Write the implementation**

Create `src/tray/events.rs`:

```rust
//! Turning raw tray and menu events into intentions.
//!
//! Pure on purpose: the mapping is where the mistakes are (a stale menu id
//! must not resolve to the wrong account), and it is testable without a GUI.

use crate::tray::menu::{MenuEntry, MenuModel};

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    SwitchTo(String),
    AddAccount,
    Quit,
    /// Nothing to do -- an unknown id, a separator, or pointer motion.
    Ignore,
}

/// The kinds of tray-icon event byte distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEventKind {
    Click,
    DoubleClick,
    Move,
    Enter,
    Leave,
}

/// Whether an event is worth doing any work for.
///
/// `TrayIconEvent` is a pointer stream, not a click stream: hovering the icon
/// emits `Move` continuously. Everything except a click is dropped here,
/// before any I/O happens.
pub fn is_actionable_tray_event(kind: TrayEventKind) -> bool {
    matches!(kind, TrayEventKind::Click | TrayEventKind::DoubleClick)
}

/// Resolve a clicked menu id to an action.
///
/// `ids` is parallel to `model.entries` -- element *i* is the id assigned to
/// entry *i* when the menu was built. An id that is not in `ids` is
/// [`Action::Ignore`]: menu ids change every rebuild, and a stale click must
/// never be resolved positionally to whatever account now sits there.
pub fn action_for_menu_id(model: &MenuModel, ids: &[String], clicked: &str) -> Action {
    let Some(index) = ids.iter().position(|id| id == clicked) else {
        return Action::Ignore;
    };
    match model.entries.get(index) {
        Some(MenuEntry::Account { uuid, .. }) => Action::SwitchTo(uuid.clone()),
        Some(MenuEntry::AddAccount) => Action::AddAccount,
        Some(MenuEntry::Quit) => Action::Quit,
        _ => Action::Ignore,
    }
}
```

Add `pub mod events;` to `src/tray/mod.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test tray_events_test`
Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add src/tray/events.rs src/tray/mod.rs tests/tray_events_test.rs
git commit -m "feat: map tray and menu events to actions"
```

---

### Task 6: Best-effort notifications

**Files:**
- Create: `src/tray/notify.rs`
- Create: `tests/tray_notify_test.rs`
- Modify: `src/tray/mod.rs`, `Cargo.toml`

**Interfaces:**
- Consumes: `SwitchOutcome`, `SyncOutcome` (`src/ops/switch.rs`), `running_sessions_warning` (Task 3).
- Produces: `notify::send(title: &str, body: &str)`, `notify::switch_message(&SwitchOutcome, usize) -> (String, String)`.

- [ ] **Step 1: Add the dependency**

```bash
cargo add notify-rust
```

- [ ] **Step 2: Write the failing test**

Create `tests/tray_notify_test.rs`. `send` is not asserted — firing a real desktop toast from a test suite is antisocial and unverifiable. The message *content* is what matters.

```rust
use byte::ops::switch::{SwitchOutcome, SyncOutcome};
use byte::store::metadata::AccountMeta;
use byte::tray::notify::switch_message;

fn meta(label: &str) -> AccountMeta {
    AccountMeta {
        uuid: "u1".into(),
        label: label.into(),
        email: Some("a@example.com".into()),
        organization_name: Some("Indicio".into()),
        subscription_type: Some("max".into()),
        account: serde_json::json!({"accountUuid": "u1"}),
        user_id: Some("uid".into()),
        credential_schema: 1,
        added_at: "2026-01-01T00:00:00Z".into(),
        last_used_at: None,
    }
}

fn outcome(already_active: bool) -> SwitchOutcome {
    SwitchOutcome {
        switched_to: meta("work"),
        sync: SyncOutcome::LoggedOut,
        already_active,
    }
}

#[test]
fn a_switch_names_the_account_it_switched_to() {
    let (title, body) = switch_message(&outcome(false), 0);
    assert!(title.contains("work") || body.contains("work"), "{title} / {body}");
}

#[test]
fn a_switch_with_running_sessions_says_to_restart_them() {
    let (_title, body) = switch_message(&outcome(false), 2);
    assert!(body.contains('2'), "should name the count: {body}");
    assert!(
        body.to_lowercase().contains("restart"),
        "should say to restart: {body}"
    );
}

#[test]
fn a_switch_with_no_running_sessions_does_not_mention_restarting() {
    let (_title, body) = switch_message(&outcome(false), 0);
    assert!(
        !body.to_lowercase().contains("restart"),
        "should not mention restarting when nothing runs: {body}"
    );
}

#[test]
fn an_already_active_switch_says_so_rather_than_claiming_a_change() {
    let (title, body) = switch_message(&outcome(true), 0);
    let text = format!("{title} {body}").to_lowercase();
    assert!(text.contains("already"), "{title} / {body}");
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --test tray_notify_test`
Expected: FAIL — `unresolved import byte::tray::notify`.

- [ ] **Step 4: Write the implementation**

Create `src/tray/notify.rs`:

```rust
//! Desktop notifications.
//!
//! Strictly best-effort. A notification that fails to display must never turn
//! a completed switch into an error -- the account has already changed, and
//! reporting failure would be a lie.

use crate::ops::switch::SwitchOutcome;
use crate::output;

/// Title and body for a completed switch.
pub fn switch_message(outcome: &SwitchOutcome, running_sessions: usize) -> (String, String) {
    let label = &outcome.switched_to.label;

    if outcome.already_active {
        return (
            format!("Already on {label}"),
            "No change was needed.".to_string(),
        );
    }

    let body = match running_sessions {
        0 => "Claude Code will use this account from now on.".to_string(),
        1 => "1 running Claude Code session still uses the previous account — restart it.".to_string(),
        n => format!(
            "{n} running Claude Code sessions still use the previous account — restart them."
        ),
    };

    (format!("Switched to {label}"), body)
}

/// Show a notification, or log why it could not be shown.
pub fn send(title: &str, body: &str) {
    use notify_rust::Notification;

    if let Err(e) = Notification::new().summary(title).body(body).show() {
        // Deliberately not propagated: see the module comment.
        output::warn(&format!("could not show a notification: {e}"));
    }
}
```

Add `pub mod notify;` to `src/tray/mod.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test tray_notify_test`
Expected: 4 passed.

- [ ] **Step 6: Check that a notification actually appears**

Not a test. Create `examples/notify_check.rs`:

```rust
//! Fires one desktop notification so a human can confirm it appears.
fn main() {
    byte::tray::notify::send("byte", "If you can read this, notifications work.");
    println!("notification sent — check your desktop");
}
```

```bash
cargo run --example notify_check
```

On Windows, toasts can require a registered AppUserModelID and may silently do nothing for an unpackaged binary. **If nothing appears, that is a finding, not a failure** — report it. The design already treats notifications as optional, so the fallback is to rely on the tray tooltip and menu state, and revisit toasts in phase 7 when packaging exists.

- [ ] **Step 7: Commit**

```bash
git add src/tray/notify.rs src/tray/mod.rs tests/tray_notify_test.rs examples/notify_check.rs Cargo.toml Cargo.lock
git commit -m "feat: add best-effort switch notifications"
```

---

### Task 7: Accounts file watcher

Lets a CLI-driven switch refresh a running tray's menu.

**Files:**
- Create: `src/tray/watch.rs`
- Create: `tests/tray_watch_test.rs`
- Modify: `src/tray/mod.rs`, `Cargo.toml`

**Interfaces:**
- Consumes: `HostPaths` (`accounts_file()`).
- Produces: `AccountsWatcher::start(&impl HostPaths, on_change: impl Fn() + Send + 'static) -> Result<AccountsWatcher>`; stops on drop.

- [ ] **Step 1: Add the dependency**

```bash
cargo add notify
```

- [ ] **Step 2: Write the failing test**

Create `tests/tray_watch_test.rs`:

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use byte::paths::{HostPaths, TestPaths};
use byte::tray::watch::AccountsWatcher;

/// Poll until `f` is true or the deadline passes. Filesystem events are
/// inherently asynchronous; a fixed sleep would be either flaky or slow.
fn wait_until(deadline: Duration, f: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    f()
}

#[test]
fn writing_the_accounts_file_fires_the_callback() {
    let tp = TestPaths::new().unwrap();
    std::fs::write(tp.accounts_file(), r#"{"schema":2,"active":null,"accounts":[]}"#).unwrap();

    let hits = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&hits);
    let _watcher = AccountsWatcher::start(&tp, move || {
        seen.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();

    std::fs::write(
        tp.accounts_file(),
        r#"{"schema":2,"active":"u1","accounts":[]}"#,
    )
    .unwrap();

    assert!(
        wait_until(Duration::from_secs(5), || hits.load(Ordering::SeqCst) > 0),
        "the watcher never fired"
    );
}

#[test]
fn the_callback_stops_after_the_watcher_is_dropped() {
    let tp = TestPaths::new().unwrap();
    std::fs::write(tp.accounts_file(), "{}").unwrap();

    let hits = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&hits);
    {
        let _watcher = AccountsWatcher::start(&tp, move || {
            seen.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
    }

    let before = hits.load(Ordering::SeqCst);
    std::fs::write(tp.accounts_file(), r#"{"changed":true}"#).unwrap();
    std::thread::sleep(Duration::from_millis(500));

    assert_eq!(
        hits.load(Ordering::SeqCst),
        before,
        "a dropped watcher must not keep firing"
    );
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --test tray_watch_test`
Expected: FAIL — `unresolved import byte::tray::watch`.

- [ ] **Step 4: Write the implementation**

Create `src/tray/watch.rs`:

```rust
//! Watching `accounts.json` so a CLI-driven change refreshes a running tray.
//!
//! The file is the shared state between the two processes -- no IPC. Because
//! every write goes through an atomic replace, a watcher never observes a torn
//! file; the worst case is a briefly stale menu, never a wrong one.

use std::path::Path;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::{Error, Result};
use crate::paths::HostPaths;

/// Watches the accounts file. Stops when dropped.
pub struct AccountsWatcher {
    _watcher: RecommendedWatcher,
}

impl AccountsWatcher {
    /// Begin watching, calling `on_change` when the accounts file changes.
    ///
    /// The parent directory is watched rather than the file itself: an atomic
    /// replace swaps the inode, and a file watch would follow the old one.
    pub fn start(
        paths: &impl HostPaths,
        on_change: impl Fn() + Send + 'static,
    ) -> Result<Self> {
        let file = paths.accounts_file();
        let dir = file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| paths.byte_config_dir());

        std::fs::create_dir_all(&dir).map_err(|source| Error::Io {
            path: dir.clone(),
            source,
        })?;

        let target = file.clone();
        let mut watcher = notify::recommended_watcher(
            move |res: notify::Result<notify::Event>| {
                let Ok(event) = res else { return };
                if event.paths.iter().any(|p| p == &target) {
                    on_change();
                }
            },
        )
        .map_err(|e| Error::Io {
            path: dir.clone(),
            source: std::io::Error::other(e),
        })?;

        watcher
            .watch(&dir, RecursiveMode::NonRecursive)
            .map_err(|e| Error::Io {
                path: dir.clone(),
                source: std::io::Error::other(e),
            })?;

        Ok(Self { _watcher: watcher })
    }

    /// How long callers should coalesce bursts before acting.
    ///
    /// One logical write can produce several events (create temp, rename,
    /// metadata update). Rebuilding the menu once per burst is enough.
    pub const DEBOUNCE: Duration = Duration::from_millis(250);
}
```

Add `pub mod watch;` to `src/tray/mod.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test tray_watch_test`
Expected: 2 passed.

If the watcher fires more than once per write, that is expected — see `DEBOUNCE`. The test only asserts "more than zero".

- [ ] **Step 6: Commit**

```bash
git add src/tray/watch.rs src/tray/mod.rs tests/tray_watch_test.rs Cargo.toml Cargo.lock
git commit -m "feat: watch the accounts file for external changes"
```

---

### Task 8: Autostart

**Files:**
- Create: `src/autostart.rs`
- Create: `tests/autostart_test.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `Error`, `Result`.
- Produces: `autostart::status() -> Result<bool>`, `autostart::enable() -> Result<()>`, `autostart::disable() -> Result<()>`, `autostart::describe_location() -> String`.

- [ ] **Step 1: Write the failing test**

Create `tests/autostart_test.rs`. The real implementation touches the Windows registry / a LaunchAgent / a `.desktop` file, none of which a test may modify. Only the platform-independent parts are asserted.

```rust
use byte::autostart;

#[test]
fn the_location_is_described_for_this_platform() {
    let described = autostart::describe_location();
    assert!(
        !described.trim().is_empty(),
        "users need to know where byte would install itself"
    );
    #[cfg(windows)]
    assert!(
        described.contains("Run") || described.to_lowercase().contains("registry"),
        "should name the registry Run key: {described}"
    );
    #[cfg(target_os = "macos")]
    assert!(
        described.to_lowercase().contains("launchagent"),
        "should name the LaunchAgent: {described}"
    );
}

#[test]
fn status_is_readable_without_changing_anything() {
    // Must not panic and must not enable anything as a side effect.
    let before = autostart::status();
    let after = autostart::status();
    assert_eq!(
        before.is_ok(),
        after.is_ok(),
        "status must be a pure read"
    );
    if let (Ok(a), Ok(b)) = (before, after) {
        assert_eq!(a, b, "status must not change between consecutive reads");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test autostart_test`
Expected: FAIL — `unresolved import byte::autostart`.

- [ ] **Step 3: Write the implementation**

Create `src/autostart.rs`. Opt-in only — byte never installs itself without being asked.

```rust
//! Starting the tray at login, on request.
//!
//! Opt-in and reversible: a credential-holding tool that adds itself to login
//! items unasked is the kind of thing users resent discovering later.

use crate::error::{Error, Result};

const ENTRY_NAME: &str = "byte";

/// Where byte would register itself, in words a user can check.
pub fn describe_location() -> String {
    #[cfg(windows)]
    {
        "the registry Run key (HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run)".to_string()
    }
    #[cfg(target_os = "macos")]
    {
        "a LaunchAgent in ~/Library/LaunchAgents".to_string()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "a .desktop entry in ~/.config/autostart".to_string()
    }
}

fn exe_path() -> Result<std::path::PathBuf> {
    std::env::current_exe().map_err(|source| Error::Io {
        path: std::path::PathBuf::from("<current_exe>"),
        source,
    })
}

#[cfg(windows)]
mod platform {
    use super::{ENTRY_NAME, Error, Result, exe_path};

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    fn run_key(write: bool) -> Result<winreg::RegKey> {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
        let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
        let access = if write { KEY_READ | KEY_WRITE } else { KEY_READ };
        hkcu.open_subkey_with_flags(RUN_KEY, access)
            .map_err(|source| Error::Io {
                path: std::path::PathBuf::from(RUN_KEY),
                source,
            })
    }

    pub fn status() -> Result<bool> {
        let key = run_key(false)?;
        Ok(key.get_value::<String, _>(ENTRY_NAME).is_ok())
    }

    pub fn enable() -> Result<()> {
        let exe = exe_path()?;
        let key = run_key(true)?;
        key.set_value(ENTRY_NAME, &format!("\"{}\"", exe.display()))
            .map_err(|source| Error::Io {
                path: std::path::PathBuf::from(RUN_KEY),
                source,
            })
    }

    pub fn disable() -> Result<()> {
        let key = run_key(true)?;
        match key.delete_value(ENTRY_NAME) {
            Ok(()) => Ok(()),
            // Already absent is success, not an error.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(Error::Io {
                path: std::path::PathBuf::from(RUN_KEY),
                source,
            }),
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{Error, Result, exe_path};

    fn entry_path() -> Result<std::path::PathBuf> {
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| Error::Io {
                path: std::path::PathBuf::from("$HOME"),
                source: std::io::Error::other("HOME is not set"),
            })?;
        #[cfg(target_os = "macos")]
        {
            Ok(home.join("Library/LaunchAgents/fyi.jocke.byte.plist"))
        }
        #[cfg(not(target_os = "macos"))]
        {
            Ok(home.join(".config/autostart/byte.desktop"))
        }
    }

    fn contents(exe: &std::path::Path) -> String {
        #[cfg(target_os = "macos")]
        {
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
                 \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                 <plist version=\"1.0\"><dict>\n\
                 <key>Label</key><string>fyi.jocke.byte</string>\n\
                 <key>ProgramArguments</key><array><string>{}</string></array>\n\
                 <key>RunAtLoad</key><true/>\n\
                 </dict></plist>\n",
                exe.display()
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            format!(
                "[Desktop Entry]\nType=Application\nName=byte\nExec={}\n\
                 X-GNOME-Autostart-enabled=true\n",
                exe.display()
            )
        }
    }

    pub fn status() -> Result<bool> {
        Ok(entry_path()?.exists())
    }

    pub fn enable() -> Result<()> {
        let path = entry_path()?;
        let exe = exe_path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|source| Error::Io {
                path: dir.to_path_buf(),
                source,
            })?;
        }
        crate::atomic::write(&path, contents(&exe).as_bytes())
    }

    pub fn disable() -> Result<()> {
        let path = entry_path()?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(Error::Io { path, source }),
        }
    }
}

/// Is byte currently set to start at login?
pub fn status() -> Result<bool> {
    platform::status()
}

/// Register byte to start at login.
pub fn enable() -> Result<()> {
    platform::enable()
}

/// Remove byte from login items. Already-absent is success.
pub fn disable() -> Result<()> {
    platform::disable()
}
```

On Windows this needs the registry crate:

```bash
cargo add winreg --target x86_64-pc-windows-msvc
```

Add `pub mod autostart;` to `src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test autostart_test`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src/autostart.rs src/lib.rs tests/autostart_test.rs Cargo.toml Cargo.lock
git commit -m "feat: add opt-in autostart registration"
```

---

### Task 9: The tray application

The event loop. This is the one file that cannot be unit-tested; everything it decides was tested in Tasks 4–7.

**Files:**
- Create: `src/tray/app.rs`
- Modify: `src/tray/mod.rs`, `Cargo.toml`

**Interfaces:**
- Consumes: `MenuModel`/`MenuEntry` (4), `Action`/`is_actionable_tray_event` (5), `notify` (6), `AccountsWatcher` (7), `InstanceGuard`/`MutationGuard` (1), `SysinfoProbe` (2), `Switcher`, `manage::list`.
- Produces: `tray::run(paths: RealPaths) -> Result<()>`.

- [ ] **Step 1: Add the dependencies**

```bash
cargo add tray-icon winit
```

- [ ] **Step 2: Write the implementation**

There is no failing-test step here: the deliverable is an event loop whose behaviour is only observable by running it. Its decisions live in already-tested pure functions.

Create `src/tray/app.rs`:

```rust
//! The tray event loop.
//!
//! Four constraints, each learned from a spike and each invisible until hit:
//!
//! 1. The tray is built in `resumed()`, not before `run_app` -- macOS requires
//!    it, Windows does not care.
//! 2. `exit()` does not stop callbacks immediately, so handlers are idempotent.
//! 3. Menu and tray events arrive on global receivers rather than through
//!    winit, so they are forwarded to the loop through an `EventLoopProxy`.
//! 4. `TrayIconEvent` is a pointer stream: hovering emits `Move` continuously,
//!    so non-clicks are dropped before any work happens.

use std::sync::mpsc::{Receiver, Sender, channel};

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::claude::detect::{ProcessProbe, SysinfoProbe};
use crate::error::{Error, Result};
use crate::lock::{InstanceGuard, MutationGuard};
use crate::ops::manage;
use crate::ops::switch::Switcher;
use crate::output;
use crate::paths::RealPaths;
use crate::store::secrets::KeyringStore;
use crate::tray::events::{Action, TrayEventKind, action_for_menu_id, is_actionable_tray_event};
use crate::tray::menu::{MenuEntry, MenuModel};
use crate::tray::notify;
use crate::tray::watch::AccountsWatcher;

/// Something that woke the loop.
#[derive(Debug, Clone, Copy)]
enum Wake {
    MenuOrTray,
    AccountsChanged,
}

fn tray_event_kind(event: &TrayIconEvent) -> TrayEventKind {
    match event {
        TrayIconEvent::Click { .. } => TrayEventKind::Click,
        TrayIconEvent::DoubleClick { .. } => TrayEventKind::DoubleClick,
        TrayIconEvent::Enter { .. } => TrayEventKind::Enter,
        TrayIconEvent::Leave { .. } => TrayEventKind::Leave,
        _ => TrayEventKind::Move,
    }
}

/// byte's icon: a filled square with a dark border, generated in code so there
/// is no asset to ship or lose.
fn icon() -> Result<Icon> {
    let (w, h) = (32u32, 32u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let border = x < 2 || y < 2 || x >= w - 2 || y >= h - 2;
            let (r, g, b) = if border { (20, 20, 20) } else { (222, 120, 60) };
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    Icon::from_rgba(rgba, w, h).map_err(|e| Error::Secret(format!("tray icon: {e}")))
}

struct App {
    paths: RealPaths,
    tray: Option<TrayIcon>,
    model: MenuModel,
    ids: Vec<String>,
    _watcher: Option<AccountsWatcher>,
    wakes: Receiver<Wake>,
    proxy: EventLoopProxy<()>,
    tx: Sender<Wake>,
    exiting: bool,
}

impl App {
    fn switcher(&self) -> Switcher<&RealPaths, KeyringStore> {
        Switcher::new(&self.paths, KeyringStore::new())
    }

    /// Rebuild the menu from what `byte list` would show.
    fn rebuild(&mut self) {
        let sw = self.switcher();
        let listing = match manage::list(&sw) {
            Ok(l) => l,
            Err(e) => {
                output::warn(&format!("could not read accounts: {e}"));
                return;
            }
        };
        self.model = MenuModel::from_listing(&listing);

        let menu = Menu::new();
        let mut ids = Vec::with_capacity(self.model.entries.len());
        for entry in &self.model.entries {
            match entry {
                MenuEntry::Account { label, detail, active, .. } => {
                    let text = match detail {
                        Some(d) => format!("{label}  ({d})"),
                        None => label.clone(),
                    };
                    let text = if *active { format!("● {text}") } else { format!("   {text}") };
                    let item = MenuItem::new(text, true, None);
                    ids.push(item.id().0.clone());
                    let _ = menu.append(&item);
                }
                MenuEntry::Separator => {
                    let sep = PredefinedMenuItem::separator();
                    ids.push(String::new());
                    let _ = menu.append(&sep);
                }
                MenuEntry::AddAccount => {
                    let item = MenuItem::new("Add account…", true, None);
                    ids.push(item.id().0.clone());
                    let _ = menu.append(&item);
                }
                MenuEntry::Quit => {
                    let item = MenuItem::new("Quit", true, None);
                    ids.push(item.id().0.clone());
                    let _ = menu.append(&item);
                }
            }
        }
        self.ids = ids;

        if let Some(tray) = &self.tray {
            tray.set_menu(Some(Box::new(menu)));
            let active = self
                .model
                .entries
                .iter()
                .find_map(|e| match e {
                    MenuEntry::Account { label, active: true, .. } => Some(label.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "no account".to_string());
            let _ = tray.set_tooltip(Some(format!("byte — {active}")));
        }
    }

    fn perform(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        match action {
            Action::Ignore => {}
            Action::Quit => {
                self.exiting = true;
                event_loop.exit();
            }
            Action::AddAccount => {
                // `add` logs the user out and waits for an interactive login,
                // which a menu click cannot supervise. Point at the command
                // that can.
                notify::send(
                    "Add an account",
                    "Run `byte add` in a terminal — it logs Claude Code out and waits for the new login.",
                );
            }
            Action::SwitchTo(uuid) => {
                let Some(_guard) = (match MutationGuard::try_acquire(&self.paths) {
                    Ok(g) => g,
                    Err(e) => {
                        output::warn(&format!("could not take the mutation lock: {e}"));
                        return;
                    }
                }) else {
                    notify::send("Busy", "Another byte process is changing accounts. Try again.");
                    return;
                };

                let sw = self.switcher();
                match sw.switch_to(&uuid) {
                    Ok(outcome) => {
                        let running = SysinfoProbe::new().running_claude_sessions();
                        let (title, body) = notify::switch_message(&outcome, running);
                        notify::send(&title, &body);
                    }
                    Err(e) => notify::send("Switch failed", &e.to_string()),
                }
                // Guard drops here, before the rebuild reads the file.
                drop(_guard);
                self.rebuild();
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        if self.tray.is_some() {
            return; // Constraint 1: built once, here rather than earlier.
        }
        let built = match icon().and_then(|i| {
            TrayIconBuilder::new()
                .with_icon(i)
                .with_tooltip("byte")
                .build()
                .map_err(|e| Error::Secret(format!("tray: {e}")))
        }) {
            Ok(t) => t,
            Err(e) => {
                output::error(&format!("could not create the tray icon: {e}"));
                return;
            }
        };
        self.tray = Some(built);
        self.rebuild();

        // Constraint 3: forward global receivers into the loop.
        let tx = self.tx.clone();
        let proxy = self.proxy.clone();
        MenuEvent::set_event_handler(Some(move |_| {
            let _ = tx.send(Wake::MenuOrTray);
            let _ = proxy.send_event(());
        }));
        let tx = self.tx.clone();
        let proxy = self.proxy.clone();
        TrayIconEvent::set_event_handler(Some(move |_| {
            let _ = tx.send(Wake::MenuOrTray);
            let _ = proxy.send_event(());
        }));

        let tx = self.tx.clone();
        let proxy = self.proxy.clone();
        match AccountsWatcher::start(&self.paths, move || {
            let _ = tx.send(Wake::AccountsChanged);
            let _ = proxy.send_event(());
        }) {
            Ok(w) => self._watcher = Some(w),
            Err(e) => output::warn(&format!("not watching accounts.json: {e}")),
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _: ()) {
        if self.exiting {
            return; // Constraint 2.
        }
        while let Ok(wake) = self.wakes.try_recv() {
            match wake {
                Wake::AccountsChanged => self.rebuild(),
                Wake::MenuOrTray => {
                    while let Ok(ev) = MenuEvent::receiver().try_recv() {
                        let action = action_for_menu_id(&self.model, &self.ids, &ev.id.0);
                        self.perform(action, event_loop);
                        if self.exiting {
                            return;
                        }
                    }
                    // Constraint 4: drop everything that is not a click.
                    while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
                        let _ = is_actionable_tray_event(tray_event_kind(&ev));
                    }
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.exiting {
            return; // Constraint 2.
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

/// Run the tray until the user quits.
pub fn run(paths: RealPaths) -> Result<()> {
    let Some(_instance) = InstanceGuard::acquire(&paths)? else {
        output::error("byte is already running — check your notification area.");
        return Err(Error::Busy);
    };

    let event_loop = EventLoop::new().map_err(|e| Error::Secret(format!("event loop: {e}")))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let (tx, wakes) = channel();

    let mut app = App {
        paths,
        tray: None,
        model: MenuModel { entries: Vec::new() },
        ids: Vec::new(),
        _watcher: None,
        wakes,
        proxy,
        tx,
        exiting: false,
    };

    event_loop
        .run_app(&mut app)
        .map_err(|e| Error::Secret(format!("tray event loop: {e}")))
}
```

Add `pub mod app;` to `src/tray/mod.rs` and re-export: `pub use app::run;`.

- [ ] **Step 3: Build and lint**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: clean.

If `Error::Secret` reads wrong for tray failures (it is named for the credential store), add a `Error::Tray(String)` variant instead and give it a `docs/troubleshooting.md` row. Say which you chose.

Two API details in the code above are **unverified** — a spike proved the tray
works but never touched either. Check both against the resolved crates and adapt
if they differ, reporting what you found:

- `MenuId` is assumed to be a tuple struct whose `.0` is the `String` id, used
  as `item.id().0.clone()` and `ev.id.0`. `action_for_menu_id` takes `&str`, so
  however the id is obtained it must end up as one. If `MenuId` exposes the
  string differently, adapt the two call sites — do **not** change
  `action_for_menu_id`'s signature, Task 5's tests depend on it.
- `TrayIconEvent`'s variants are matched in `tray_event_kind`. The enum is
  `#[non_exhaustive]`, so the `_ => TrayEventKind::Move` arm is load-bearing:
  an unrecognised variant must fall through to the non-actionable side, never
  to `Click`.

- [ ] **Step 4: Run it**

```bash
cargo run
```

Confirm by hand and report each: the icon appears; the menu lists your real accounts by label with the org in parentheses and a dot on the active one; the tooltip names the active account; hovering does **not** produce a storm of work; clicking a different account switches it (verify with `byte current` in another terminal) and the menu updates; running `byte switch <other>` in a terminal updates the menu within a second; a second `byte` refuses to start; Quit exits cleanly with no stray process.

**Do not run `byte add` from the menu during this check** — it only shows a notification, by design.

- [ ] **Step 5: Commit**

```bash
git add src/tray/ Cargo.toml Cargo.lock
git commit -m "feat: add the tray application"
```

---

### Task 10: Wire the CLI, lock mutations, document

**Files:**
- Modify: `src/cli/mod.rs`, `src/cli/run.rs`, `man/byte.md`, `README.md`, `docs/configuration.md`, `docs/troubleshooting.md`, `docs/architecture.md`, `AGENTS.md`
- Modify: `tests/cli_test.rs`

**Interfaces:**
- Consumes: `tray::run` (9), `autostart` (8), `MutationGuard` (1).
- Produces: `byte` with no arguments launches the tray; `byte autostart enable|disable|status`.

- [ ] **Step 1: Write the failing test**

Add to `tests/cli_test.rs`:

```rust
#[test]
fn autostart_status_is_a_recognised_subcommand() {
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["autostart", "status"]);
    assert!(
        out.status.success(),
        "autostart status should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn autostart_rejects_an_unknown_action() {
    let tp = TestPaths::new().unwrap();
    assert!(!byte(&tp, &["autostart", "frobnicate"]).status.success());
}

#[test]
fn help_lists_the_autostart_command() {
    let tp = TestPaths::new().unwrap();
    let text = String::from_utf8_lossy(&byte(&tp, &["--help"]).stdout).to_string();
    assert!(
        text.lines().any(|l| l.trim_start().starts_with("autostart")),
        "help should list autostart:\n{text}"
    );
}

#[test]
fn long_help_no_longer_claims_no_args_lists_accounts() {
    // No arguments now starts the tray; the old text would be a lie.
    let tp = TestPaths::new().unwrap();
    let text = String::from_utf8_lossy(&byte(&tp, &["--help"]).stdout).to_string();
    assert!(
        text.to_lowercase().contains("tray"),
        "long_about should describe the tray:\n{text}"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test cli_test`
Expected: FAIL — `autostart` is not a subcommand.

- [ ] **Step 3: Add the subcommand**

In `src/cli/mod.rs`, add to `Command`:

```rust
    /// Start byte's tray automatically at login.
    Autostart {
        #[command(subcommand)]
        action: AutostartAction,
    },
```

and:

```rust
#[derive(Debug, Subcommand)]
pub enum AutostartAction {
    /// Register byte to start at login.
    Enable,
    /// Remove byte from login items.
    Disable,
    /// Report whether byte starts at login.
    Status,
}
```

Update `long_about` to: `"Switch which Claude account Claude Code is authenticated as.\nRun with no arguments to start the tray icon."`

- [ ] **Step 4: Dispatch it, launch the tray, and lock mutations**

In `src/cli/run.rs`:

- `None => tray::run(paths)` instead of listing accounts.
- Add `Some(Command::Autostart { action })` handling:

```rust
fn cmd_autostart(action: AutostartAction, json: bool) -> Result<()> {
    match action {
        AutostartAction::Status => {
            let on = autostart::status()?;
            if json {
                output::data(&serde_json::json!({ "autostart": on }).to_string());
            } else if on {
                output::info(&format!("byte starts at login (via {}).", autostart::describe_location()));
            } else {
                output::info("byte does not start at login.");
            }
        }
        AutostartAction::Enable => {
            autostart::enable()?;
            output::status(&format!("byte will start at login, via {}.", autostart::describe_location()));
        }
        AutostartAction::Disable => {
            autostart::disable()?;
            output::status("byte will no longer start at login.");
        }
    }
    Ok(())
}
```

- Take the mutation lock around every mutating command, so a CLI switch cannot interleave with a tray switch. Wrap the dispatch of `Switch`, `Capture`, `Add`, `Remove`, and `Rename`:

```rust
        // Read-only commands (list, current, autostart) do not take the lock:
        // they tolerate a concurrent write because every file byte writes is
        // replaced atomically.
        let _guard = MutationGuard::acquire(&paths)?;
```

Bind it to a named variable, not `_`, so it lives to the end of the command rather than dropping immediately.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test`
Expected: all pass.

- [ ] **Step 6: Update the documentation**

- `man/byte.md` — add `autostart enable|disable|status`; change the SYNOPSIS/DESCRIPTION so no-arguments starts the tray; add `Error::Busy` and any new tray error to the notes.
- `README.md` — describe the tray (what the menu shows, that one instance runs at a time, that a CLI switch updates it), and autostart as opt-in.
- `docs/configuration.md` — add `mutation.lock` and `tray.lock` to the config-directory table, and say where autostart registers per platform.
- `docs/troubleshooting.md` — rows for `Error::Busy` and any new tray error; a note that the tray shows a notification for "Add account" rather than performing it, and why.
- `docs/architecture.md` — add `tray/`, `lock.rs`, `autostart.rs`, `claude/detect.rs` to the module map, and state the dependency direction (`tray` sits beside `cli`, both above `ops`).
- `AGENTS.md` — add the four tray constraints to the parity rules, so nobody rediscovers them.

- [ ] **Step 7: Full verification**

```bash
cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check && cargo build --release
```

Then by hand: `byte` starts the tray; `byte list` still works with the tray running; `byte autostart status` reports correctly; `byte autostart enable` then `status` reports enabled; `byte autostart disable` returns it. Report each.

- [ ] **Step 8: Commit**

```bash
git add src/cli/ man/ README.md docs/ AGENTS.md tests/cli_test.rs
git commit -m "feat: launch the tray with no arguments and add autostart"
```

---

## Self-Review

**Spec coverage.** Phase 5 (§11): tray icon → Task 9; menu → Tasks 4, 9; events → Tasks 5, 9; notifications → Task 6. Phase 6: process detection → Task 2, warnings → Tasks 3, 6. §6.1's module layout: `tray/mod.rs`, `tray/menu.rs`, `tray/events.rs`, `tray/notify.rs` all present, plus `tray/app.rs` and `tray/watch.rs` which §6.1 did not anticipate — noted below. §9's running-session row → Task 3.

**Beyond the spec, deliberately.** Three things this plan adds that §6.1 does not list, each with a reason: `src/lock.rs` (the tray makes byte-vs-byte concurrency possible for the first time — spec §12 risk 4 only considers byte vs Claude Code); `src/tray/watch.rs` (the cooperation mechanism that replaces IPC); `src/autostart.rs` (a product decision taken during planning). The spec should gain a short §6.4 describing the tray/CLI cooperation model once this lands.

**Deferred to phase 7:** packaging, installers, signing, release artifacts, the website. Also deferred: making the tray perform `add` itself rather than pointing at the CLI — it needs to supervise an interactive login, which is a design problem of its own.

**Known follow-ups recorded rather than hidden:**
- `Action::AddAccount` shows a notification instead of adding an account. Task 9 Step 4 says not to treat that as a bug.
- Windows toast notifications may silently no-op for an unpackaged binary (Task 6 Step 6). If so, the tray's feedback is its tooltip and menu until phase 7.
- `MenuEntry::Separator` occupies an index in `ids` with an empty string so the two vectors stay parallel; `action_for_menu_id` maps it to `Ignore`.
- Task 3 changes `cmd_switch`'s signature to take a probe. Existing callers in `run()` must be updated in the same task.
