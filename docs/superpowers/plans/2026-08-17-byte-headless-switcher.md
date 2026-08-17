# byte Headless Switcher Implementation Plan (Phases 0–4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a fully working headless Claude account switcher — `byte list`, `byte switch`, `byte add`, `byte capture`, `byte remove`, `byte rename`, `byte current` — with no GUI code.

**Architecture:** A core library moves an `AccountSnapshot` (the `claudeAiOauth` block from `.credentials.json` plus `oauthAccount`/`userID` from `.claude.json`) between the live Claude Code config files and a per-account store. Snapshots are held as opaque `serde_json::Value` so unknown upstream fields pass through untouched. Every write patches a parsed document in place and is replaced atomically, then verified. Three trait seams — `HostPaths`, `SecretStore`, `ProcessProbe` — make everything testable against a `TempDir` with no keychain and no Claude installation.

**Tech Stack:** Rust 1.88 (edition 2024), `serde_json` **with `preserve_order`**, `keyring` v3, `clap` v4 derive, `tempfile`, `time`, `thiserror`, `anyhow`.

**Spec:** [`docs/superpowers/specs/2026-08-17-byte-account-switcher-design.md`](../specs/2026-08-17-byte-account-switcher-design.md)

## Global Constraints

Every task's requirements implicitly include this section.

- **Rust 1.88.0, edition 2024.** Pinned by `rust-toolchain.toml`. Do not raise or lower.
- **`serde_json` MUST enable the `preserve_order` feature.** Without it, `serde_json::Value` objects are backed by a `BTreeMap` that silently re-sorts every key alphabetically. Writing `~/.claude.json` without this would reorder all ~80 top-level keys on every switch. This is the single most important dependency flag in the project.
- **No raw `println!` / `eprintln!` outside `src/output.rs`** (OSS_SPEC §19.4). All user-facing output routes through the semantic helpers there.
- **All tests live in `tests/`**, in files whose stem matches `_?[Tt]ests?$` (e.g. `paths_test.rs`). Never use inline `#[cfg(test)]` modules (AGENTS.md, OSS_SPEC §20).
- **Non-test source files stay under 1000 physical lines** (OSS_SPEC §20.5).
- **`make lint` (`cargo clippy --all-targets -- -D warnings`) and `make fmt-check` must pass before every commit.**
- **Conventional Commits** for every commit message.
- **Never mutate a caller's data in place.** Prefer functions returning new values (user's global coding-style rule).
- **Never write to a real home directory in a test.** Every test constructs a `TempDir` and a `TestPaths`.

---

## File Structure

| File | Responsibility |
|---|---|
| `src/error.rs` | `Error` enum (thiserror) + `Result` alias. Every fallible path in the crate returns this. |
| `src/paths.rs` | `HostPaths` trait; `RealPaths` (home dir + env overrides) and `TestPaths` (tempdir). |
| `src/atomic.rs` | Atomic file replace, timestamped backup, backup pruning. |
| `src/claude/document.rs` | `JsonDocument` — load/patch/save JSON preserving unknown keys, key order, and formatting style. |
| `src/claude/snapshot.rs` | `AccountSnapshot` type, field accessors, validation. |
| `src/claude/files.rs` | `ClaudeFiles` — capture/apply/clear a snapshot against the two live files. |
| `src/store/metadata.rs` | `AccountsFile`, `AccountMeta`, load/save `accounts.json`, name resolution. |
| `src/store/secrets.rs` | `SecretStore` trait, `KeyringStore`, `MemoryStore`. |
| `src/ops/switch.rs` | Sync-back + switch, the core algorithm (spec §7). |
| `src/ops/add.rs` | Logout-and-watch add flow (spec §8). |
| `src/ops/manage.rs` | list / remove / rename / current. |
| `src/cli/mod.rs` | clap command definitions. |
| `src/cli/run.rs` | Command dispatch, human and `--json` rendering. |
| `src/output.rs` | **Modify** — add a `data()` helper writing to stdout. |

Deferred to Plan 2 (Phases 5–7): `src/tray/*`, `src/claude/detect.rs`, packaging.

---

### Task 1: Research, dependency verification, and project skeleton

Phase 0 of the spec. This task produces no runtime behavior — its deliverable is a findings document plus a compiling skeleton with verified dependencies. It exists because several design decisions rest on assumptions that must be checked before code depends on them.

**Files:**
- Create: `docs/superpowers/research/2026-08-17-prior-art.md`
- Modify: `Cargo.toml`
- Create: `src/error.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Survey prior art**

Per the mandatory Research & Reuse workflow, check whether this already exists before building it:

```bash
gh search repos "claude account switcher" --limit 20
```

```bash
gh search repos "claude code multiple accounts" --limit 20
```

```bash
gh search code "claudeAiOauth" --limit 30
```

Record in the findings doc: project name, language, approach, license, and whether it can be adopted, ported, or wrapped rather than rewritten. If any project solves 80%+ of this, **stop and raise it** before continuing — that is a design-changing finding.

- [ ] **Step 2: Verify the `--user-data-dir` question for Phase 8**

Close Claude Desktop, then:

```bash
"$LOCALAPPDATA/AnthropicClaude/claude.exe" --user-data-dir="$TEMP/byte-desktop-probe"
```

Record whether a fresh profile directory is created at that path and whether the app presents a logged-out state. This decides approach A vs B in spec §14.2. Do not implement anything from it.

- [ ] **Step 3: Verify refresh-token rotation**

This determines whether spec §7.1's sync-back is merely correct or strictly necessary. Compare hashes, never raw token values:

```bash
python -c "
import json,os,hashlib
p=os.path.expanduser('~/.claude/.credentials.json')
d=json.load(open(p))['claudeAiOauth']
print('refresh sha256[:12] =', hashlib.sha256(d['refreshToken'].encode()).hexdigest()[:12])
print('expiresAt =', d['expiresAt'])
"
```

Run it, use Claude Code long enough for a token refresh to occur, run it again, and record whether the refresh-token hash changed. Note the result in the findings doc either way.

- [ ] **Step 4: Add dependencies**

Use `cargo add` rather than hand-editing versions, so the resolver picks versions compatible with Rust 1.88:

```bash
cargo add serde --features derive
```

```bash
cargo add serde_json --features preserve_order
```

```bash
cargo add thiserror anyhow tempfile
```

```bash
cargo add time --features formatting,parsing,macros,serde
```

```bash
cargo add clap --features derive
```

```bash
cargo add keyring --features apple-native,windows-native,sync-secret-service
```

- [ ] **Step 5: Verify `preserve_order` is actually active**

This guards the project's most important invariant. Write this throwaway check and run it:

```bash
cat > /tmp/order_check.rs <<'EOF'
fn main() {
    let v: serde_json::Value = serde_json::from_str(r#"{"zebra":1,"apple":2}"#).unwrap();
    println!("{}", serde_json::to_string(&v).unwrap());
}
EOF
```

Run it as a temporary example (`cargo run --example order_check` after placing it in `examples/`), confirm the output is `{"zebra":1,"apple":2}` and **not** `{"apple":2,"zebra":1}`, then delete the file. If keys sorted, the feature is not active — fix `Cargo.toml` before proceeding.

- [ ] **Step 6: Create the error type**

Create `src/error.rs`:

```rust
//! Error type for byte.

use std::path::PathBuf;

/// Every fallible operation in this crate returns this error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Claude Code file not found: {0}\nIs Claude Code installed and logged in?")]
    ClaudeFileMissing(PathBuf),

    #[error("failed to parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("io error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("no account matching '{0}'")]
    NoSuchAccount(String),

    #[error("'{query}' is ambiguous; it matches {count} accounts")]
    AmbiguousAccount { query: String, count: usize },

    #[error("no Claude account is currently logged in")]
    NotLoggedIn,

    #[error("unsupported snapshot schema version {found}; this build expects {expected}")]
    SchemaMismatch { found: u32, expected: u32 },

    #[error("stored credentials for '{account}' are unusable: {reason}")]
    InvalidSnapshot { account: String, reason: String },

    #[error("secret store unavailable: {0}")]
    Secret(String),

    #[error("write verification failed for {path}; the original was restored from backup")]
    VerifyFailed { path: PathBuf },

    #[error("timed out after {0} seconds waiting for a new login")]
    LoginTimeout(u64),
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 7: Wire the module and confirm the build**

Replace `src/lib.rs` with:

```rust
//! byte — claude account switcher

pub mod error;
pub mod output;

pub use error::{Error, Result};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
```

Run: `make build && make lint && make fmt-check`
Expected: all three succeed.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock src/error.rs src/lib.rs docs/superpowers/research/
git commit -m "chore: add dependencies, error type, and phase 0 research findings"
```

---

### Task 2: Path resolution

**Files:**
- Create: `src/paths.rs`
- Create: `tests/paths_test.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `byte::Result` from Task 1.
- Produces: `HostPaths` trait with `claude_config()`, `claude_credentials()`, `byte_config_dir()`, `backup_dir()`; concrete `RealPaths` and `TestPaths`. Every later task takes a `&dyn HostPaths` or a generic `P: HostPaths`.

- [ ] **Step 1: Write the failing test**

Create `tests/paths_test.rs`:

```rust
use byte::paths::{HostPaths, TestPaths};

#[test]
fn test_paths_puts_all_files_under_its_root() {
    let tp = TestPaths::new().unwrap();
    let root = tp.root().to_path_buf();

    assert_eq!(tp.claude_config(), root.join(".claude.json"));
    assert_eq!(
        tp.claude_credentials(),
        root.join(".claude").join(".credentials.json")
    );
    assert!(tp.byte_config_dir().starts_with(&root));
    assert_eq!(tp.backup_dir(), tp.byte_config_dir().join("backups"));
}

#[test]
fn test_paths_creates_parent_directories() {
    let tp = TestPaths::new().unwrap();
    assert!(tp.claude_credentials().parent().unwrap().is_dir());
    assert!(tp.byte_config_dir().is_dir());
}

#[test]
fn accounts_file_lives_in_the_config_dir() {
    let tp = TestPaths::new().unwrap();
    assert_eq!(tp.accounts_file(), tp.byte_config_dir().join("accounts.json"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test paths_test`
Expected: FAIL — `unresolved import byte::paths`.

- [ ] **Step 3: Write the implementation**

Create `src/paths.rs`:

```rust
//! Filesystem locations byte reads and writes.
//!
//! `HostPaths` is the primary test seam: production code uses [`RealPaths`],
//! tests use [`TestPaths`], which is rooted in a temporary directory that is
//! deleted when it drops.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Where the Claude Code config files and byte's own state live.
pub trait HostPaths: Send + Sync {
    /// `~/.claude.json` — the large document holding `oauthAccount`.
    fn claude_config(&self) -> PathBuf;

    /// `~/.claude/.credentials.json` — holds `claudeAiOauth`.
    fn claude_credentials(&self) -> PathBuf;

    /// byte's own configuration directory.
    fn byte_config_dir(&self) -> PathBuf;

    /// Where pre-write backups are kept.
    fn backup_dir(&self) -> PathBuf {
        self.byte_config_dir().join("backups")
    }

    /// The account metadata file.
    fn accounts_file(&self) -> PathBuf {
        self.byte_config_dir().join("accounts.json")
    }
}

/// Lets `&TestPaths` and `&RealPaths` satisfy `HostPaths`, so callers can hold
/// a cheap `Copy` reference instead of taking ownership. Later types are
/// generic over `P: HostPaths + Copy`, which this makes possible.
impl<T: HostPaths + ?Sized> HostPaths for &T {
    fn claude_config(&self) -> PathBuf {
        (**self).claude_config()
    }
    fn claude_credentials(&self) -> PathBuf {
        (**self).claude_credentials()
    }
    fn byte_config_dir(&self) -> PathBuf {
        (**self).byte_config_dir()
    }
}

/// Production paths, resolved from the user's home directory.
///
/// Two environment variables override discovery:
/// - `CLAUDE_CONFIG_DIR` — directory containing `.claude.json` and `.credentials.json`
/// - `BYTE_CONFIG_DIR` — byte's own config directory
#[derive(Debug, Clone)]
pub struct RealPaths {
    claude_config: PathBuf,
    claude_credentials: PathBuf,
    byte_config_dir: PathBuf,
}

impl RealPaths {
    pub fn discover() -> Result<Self> {
        let home = home_dir().ok_or_else(|| Error::ClaudeFileMissing(PathBuf::from("$HOME")))?;

        let (claude_config, claude_credentials) = match std::env::var_os("CLAUDE_CONFIG_DIR") {
            Some(dir) => {
                let dir = PathBuf::from(dir);
                (dir.join(".claude.json"), dir.join(".credentials.json"))
            }
            None => (
                home.join(".claude.json"),
                home.join(".claude").join(".credentials.json"),
            ),
        };

        let byte_config_dir = match std::env::var_os("BYTE_CONFIG_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => default_config_dir(&home),
        };

        Ok(Self {
            claude_config,
            claude_credentials,
            byte_config_dir,
        })
    }
}

impl HostPaths for RealPaths {
    fn claude_config(&self) -> PathBuf {
        self.claude_config.clone()
    }
    fn claude_credentials(&self) -> PathBuf {
        self.claude_credentials.clone()
    }
    fn byte_config_dir(&self) -> PathBuf {
        self.byte_config_dir.clone()
    }
}

#[cfg(windows)]
fn default_config_dir(home: &Path) -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData").join("Roaming"))
        .join("byte")
}

#[cfg(target_os = "macos")]
fn default_config_dir(home: &Path) -> PathBuf {
    home.join("Library").join("Application Support").join("byte")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn default_config_dir(home: &Path) -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
        .join("byte")
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Test paths rooted in a temporary directory.
///
/// The directory is removed when this value drops, so tests must keep it
/// alive for as long as they use the paths.
#[derive(Debug)]
pub struct TestPaths {
    dir: tempfile::TempDir,
}

impl TestPaths {
    pub fn new() -> Result<Self> {
        let dir = tempfile::tempdir().map_err(|source| Error::Io {
            path: PathBuf::from("<tempdir>"),
            source,
        })?;
        let this = Self { dir };
        for d in [
            this.root().join(".claude"),
            this.byte_config_dir(),
            this.backup_dir(),
        ] {
            std::fs::create_dir_all(&d).map_err(|source| Error::Io { path: d, source })?;
        }
        Ok(this)
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }
}

impl HostPaths for TestPaths {
    fn claude_config(&self) -> PathBuf {
        self.root().join(".claude.json")
    }
    fn claude_credentials(&self) -> PathBuf {
        self.root().join(".claude").join(".credentials.json")
    }
    fn byte_config_dir(&self) -> PathBuf {
        self.root().join("byte-config")
    }
}
```

Add `pub mod paths;` to `src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test paths_test`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src/paths.rs src/lib.rs tests/paths_test.rs
git commit -m "feat: add host path resolution with test seam"
```

---

### Task 3: Atomic writes and backups

**Files:**
- Create: `src/atomic.rs`
- Create: `tests/atomic_test.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `Error`, `Result` (Task 1).
- Produces: `atomic::write(path, bytes) -> Result<()>`, `atomic::backup(path, backup_dir) -> Result<Option<PathBuf>>`, `atomic::prune(backup_dir, stem, keep) -> Result<()>`, `atomic::restore(backup, path) -> Result<()>`.

- [ ] **Step 1: Write the failing test**

Create `tests/atomic_test.rs`:

```rust
use byte::atomic;
use byte::paths::{HostPaths, TestPaths};

#[test]
fn write_creates_a_new_file() {
    let tp = TestPaths::new().unwrap();
    let target = tp.root().join("new.json");

    atomic::write(&target, b"{\"a\":1}").unwrap();

    assert_eq!(std::fs::read(&target).unwrap(), b"{\"a\":1}");
}

#[test]
fn write_replaces_existing_content_completely() {
    let tp = TestPaths::new().unwrap();
    let target = tp.root().join("existing.json");
    std::fs::write(&target, b"a much longer original body").unwrap();

    atomic::write(&target, b"short").unwrap();

    assert_eq!(std::fs::read(&target).unwrap(), b"short");
}

#[test]
fn write_leaves_no_temporary_files_behind() {
    let tp = TestPaths::new().unwrap();
    let target = tp.root().join("clean.json");

    atomic::write(&target, b"{}").unwrap();

    let strays: Vec<_> = std::fs::read_dir(tp.root())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".tmp") || n.starts_with('.') && n.contains("tmp"))
        .collect();
    assert!(strays.is_empty(), "left temp files: {strays:?}");
}

#[test]
fn backup_copies_the_file_and_returns_its_path() {
    let tp = TestPaths::new().unwrap();
    let target = tp.root().join("orig.json");
    std::fs::write(&target, b"original").unwrap();

    let saved = atomic::backup(&target, &tp.backup_dir()).unwrap().unwrap();

    assert!(saved.exists());
    assert_eq!(std::fs::read(&saved).unwrap(), b"original");
    assert!(saved.starts_with(tp.backup_dir()));
}

#[test]
fn backup_of_a_missing_file_is_not_an_error() {
    let tp = TestPaths::new().unwrap();
    let missing = tp.root().join("nope.json");

    assert!(atomic::backup(&missing, &tp.backup_dir()).unwrap().is_none());
}

#[test]
fn restore_puts_the_backup_back() {
    let tp = TestPaths::new().unwrap();
    let target = tp.root().join("orig.json");
    std::fs::write(&target, b"original").unwrap();
    let saved = atomic::backup(&target, &tp.backup_dir()).unwrap().unwrap();
    std::fs::write(&target, b"corrupted").unwrap();

    atomic::restore(&saved, &target).unwrap();

    assert_eq!(std::fs::read(&target).unwrap(), b"original");
}

#[test]
fn prune_keeps_only_the_newest_backups() {
    let tp = TestPaths::new().unwrap();
    let dir = tp.backup_dir();
    for i in 0..5 {
        std::fs::write(dir.join(format!("orig.json.{i:03}.bak")), b"x").unwrap();
    }

    atomic::prune(&dir, "orig.json", 3).unwrap();

    let remaining = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(remaining, 3);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test atomic_test`
Expected: FAIL — `unresolved import byte::atomic`.

- [ ] **Step 3: Write the implementation**

Create `src/atomic.rs`:

```rust
//! Crash-safe file replacement.
//!
//! Writes go to a temporary file in the *same directory* as the target, are
//! flushed and fsynced, and are then atomically renamed over the target. Same
//! directory matters: a cross-volume rename is a copy, which is not atomic.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

fn io_err(path: &Path) -> impl Fn(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Atomically replace `path` with `contents`.
pub fn write(path: &Path, contents: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).map_err(io_err(dir))?;

    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(io_err(dir))?;
    tmp.write_all(contents).map_err(io_err(path))?;
    tmp.flush().map_err(io_err(path))?;
    tmp.as_file().sync_all().map_err(io_err(path))?;

    // persist() replaces an existing destination atomically on all supported
    // platforms (MoveFileExW with MOVEFILE_REPLACE_EXISTING on Windows).
    tmp.persist(path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e.error,
    })?;
    Ok(())
}

/// Copy `path` into `backup_dir` under a sortable timestamped name.
///
/// Returns `Ok(None)` when the source does not exist, which is not an error:
/// there is nothing to protect.
pub fn backup(path: &Path, backup_dir: &Path) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    std::fs::create_dir_all(backup_dir).map_err(io_err(backup_dir))?;

    let stem = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let dest = backup_dir.join(format!("{stem}.{stamp:013}.bak"));
    std::fs::copy(path, &dest).map_err(io_err(path))?;
    Ok(Some(dest))
}

/// Copy a backup back over `path`.
pub fn restore(backup: &Path, path: &Path) -> Result<()> {
    let contents = std::fs::read(backup).map_err(io_err(backup))?;
    write(path, &contents)
}

/// Keep only the `keep` newest backups whose name begins with `stem`.
pub fn prune(backup_dir: &Path, stem: &str, keep: usize) -> Result<()> {
    if !backup_dir.is_dir() {
        return Ok(());
    }
    let prefix = format!("{stem}.");
    let mut found: Vec<PathBuf> = std::fs::read_dir(backup_dir)
        .map_err(io_err(backup_dir))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with(&prefix))
                .unwrap_or(false)
        })
        .collect();

    // Names embed a zero-padded millisecond timestamp, so lexical order is
    // chronological order.
    found.sort();
    let excess = found.len().saturating_sub(keep);
    for path in found.into_iter().take(excess) {
        std::fs::remove_file(&path).map_err(io_err(&path))?;
    }
    Ok(())
}
```

Add `pub mod atomic;` to `src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test atomic_test`
Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add src/atomic.rs src/lib.rs tests/atomic_test.rs
git commit -m "feat: add atomic file replacement with backup and pruning"
```

---

### Task 4: The JSON preservation layer

This is the highest-risk task in the project. `~/.claude.json` is ~109 KB of unrelated user state; a careless rewrite destroys it. Spec §5 is implemented here.

**Files:**
- Create: `src/claude/mod.rs`
- Create: `src/claude/document.rs`
- Create: `tests/document_test.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `atomic::write` (Task 3), `Error`/`Result` (Task 1).
- Produces: `JsonDocument` with `load(&Path) -> Result<Self>`, `load_or_empty(&Path) -> Result<Self>`, `get(&str) -> Option<&Value>`, `set(&str, Value)`, `remove(&str)`, `to_bytes() -> Result<Vec<u8>>`, `save(&Path, &Path) -> Result<()>` (second argument is the backup dir).

- [ ] **Step 1: Write the failing test**

Create `tests/document_test.rs`:

```rust
use byte::claude::document::JsonDocument;
use byte::paths::{HostPaths, TestPaths};
use serde_json::json;

/// A stand-in for ~/.claude.json: many unrelated keys, deliberately NOT in
/// alphabetical order, with nested structures byte knows nothing about.
const PRETTY_FIXTURE: &str = r#"{
  "numStartups": 60,
  "installMethod": "native",
  "tipsHistory": {
    "new-user-warmup": 5,
    "zebra-tip": 1,
    "alpha-tip": 2
  },
  "oauthAccount": {
    "accountUuid": "old-uuid",
    "emailAddress": "old@example.com"
  },
  "userID": "old-user-id",
  "projects": {
    "/some/path": {
      "history": [
        1,
        2,
        3
      ]
    }
  },
  "autoUpdates": false
}"#;

#[test]
fn patching_one_key_leaves_every_other_byte_identical() {
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join(".claude.json");
    std::fs::write(&path, PRETTY_FIXTURE).unwrap();

    let mut doc = JsonDocument::load(&path).unwrap();
    doc.set("userID", json!("new-user-id"));
    doc.save(&path, &tp.backup_dir()).unwrap();

    let after = std::fs::read_to_string(&path).unwrap();
    let expected = PRETTY_FIXTURE.replace("\"old-user-id\"", "\"new-user-id\"");
    assert_eq!(after, expected);
}

#[test]
fn key_order_is_never_alphabetised() {
    // Guards the serde_json `preserve_order` feature. Without it this fails.
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join("order.json");
    std::fs::write(&path, r#"{"zebra":1,"apple":2,"mango":3}"#).unwrap();

    let doc = JsonDocument::load(&path).unwrap();
    let out = String::from_utf8(doc.to_bytes().unwrap()).unwrap();

    assert_eq!(out.find("zebra") < out.find("apple"), true);
    assert_eq!(out.find("apple") < out.find("mango"), true);
}

#[test]
fn unknown_nested_keys_survive_a_round_trip() {
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join(".claude.json");
    std::fs::write(&path, PRETTY_FIXTURE).unwrap();

    let mut doc = JsonDocument::load(&path).unwrap();
    doc.set("oauthAccount", json!({"accountUuid": "new-uuid"}));
    doc.save(&path, &tp.backup_dir()).unwrap();

    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(after["tipsHistory"]["zebra-tip"], json!(1));
    assert_eq!(after["projects"]["/some/path"]["history"], json!([1, 2, 3]));
    assert_eq!(after["numStartups"], json!(60));
}

#[test]
fn compact_files_stay_compact() {
    // .credentials.json is minified on disk; rewriting it pretty would
    // produce a gratuitous whole-file diff.
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join(".credentials.json");
    std::fs::write(&path, r#"{"mcpOAuth":{"a":1},"claudeAiOauth":{"x":1}}"#).unwrap();

    let mut doc = JsonDocument::load(&path).unwrap();
    doc.set("claudeAiOauth", json!({"x": 2}));
    doc.save(&path, &tp.backup_dir()).unwrap();

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(!after.contains('\n'), "compact file gained newlines: {after}");
    assert_eq!(after, r#"{"mcpOAuth":{"a":1},"claudeAiOauth":{"x":2}}"#);
}

#[test]
fn pretty_files_stay_pretty_with_two_space_indent() {
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join(".claude.json");
    std::fs::write(&path, PRETTY_FIXTURE).unwrap();

    let mut doc = JsonDocument::load(&path).unwrap();
    doc.set("userID", json!("x"));
    doc.save(&path, &tp.backup_dir()).unwrap();

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("\n  \"numStartups\""));
}

#[test]
fn a_trailing_newline_is_preserved_when_present() {
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join("nl.json");
    std::fs::write(&path, "{\"a\":1}\n").unwrap();

    let mut doc = JsonDocument::load(&path).unwrap();
    doc.set("a", json!(2));
    doc.save(&path, &tp.backup_dir()).unwrap();

    assert!(std::fs::read_to_string(&path).unwrap().ends_with("}\n"));
}

#[test]
fn a_missing_trailing_newline_is_not_added() {
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join("nonl.json");
    std::fs::write(&path, "{\"a\":1}").unwrap();

    let mut doc = JsonDocument::load(&path).unwrap();
    doc.set("a", json!(2));
    doc.save(&path, &tp.backup_dir()).unwrap();

    assert!(!std::fs::read_to_string(&path).unwrap().ends_with('\n'));
}

#[test]
fn loading_malformed_json_fails_without_writing() {
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join("bad.json");
    std::fs::write(&path, "{ this is not json").unwrap();

    assert!(JsonDocument::load(&path).is_err());
    // The file must be untouched.
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ this is not json");
}

#[test]
fn saving_creates_a_backup_first() {
    let tp = TestPaths::new().unwrap();
    let path = tp.root().join(".claude.json");
    std::fs::write(&path, PRETTY_FIXTURE).unwrap();

    let mut doc = JsonDocument::load(&path).unwrap();
    doc.set("userID", json!("changed"));
    doc.save(&path, &tp.backup_dir()).unwrap();

    let backups: Vec<_> = std::fs::read_dir(tp.backup_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(backups.len(), 1);
    let saved = std::fs::read_to_string(backups[0].path()).unwrap();
    assert_eq!(saved, PRETTY_FIXTURE);
}

#[test]
fn load_or_empty_gives_an_empty_object_for_a_missing_file() {
    let tp = TestPaths::new().unwrap();
    let doc = JsonDocument::load_or_empty(&tp.root().join("absent.json")).unwrap();
    assert!(doc.get("anything").is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test document_test`
Expected: FAIL — `unresolved import byte::claude`.

- [ ] **Step 3: Write the implementation**

Create `src/claude/mod.rs`:

```rust
//! Reading and writing Claude Code's on-disk state.

pub mod document;
```

Create `src/claude/document.rs`:

```rust
//! A JSON document that can be patched without disturbing anything else.
//!
//! byte owns at most three keys across two files that contain, between them,
//! a great deal of unrelated user state. Every read parses into
//! `serde_json::Value` (order-preserving, see the crate's `preserve_order`
//! feature) and every write re-serialises that same value with only the
//! targeted keys replaced. The document is never deserialised into a typed
//! struct, because any field such a struct did not know about would be
//! silently dropped.

use std::path::Path;

use serde_json::Value;

use crate::atomic;
use crate::error::{Error, Result};

/// How the file is laid out on disk. Claude Code writes `.claude.json`
/// pretty-printed and `.credentials.json` minified; byte reproduces whichever
/// it finds rather than imposing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Pretty,
    Compact,
}

impl Style {
    fn detect(raw: &str) -> Self {
        // A pretty-printed object has a newline before its first key.
        match raw.find('\n') {
            Some(nl) => match raw.find('"') {
                Some(q) if nl < q => Style::Pretty,
                Some(_) => Style::Compact,
                None => Style::Pretty,
            },
            None => Style::Compact,
        }
    }
}

/// A parsed JSON object plus the formatting details needed to write it back.
#[derive(Debug, Clone)]
pub struct JsonDocument {
    value: Value,
    style: Style,
    trailing_newline: bool,
}

impl JsonDocument {
    /// Parse an existing file. Fails if it is absent or malformed — byte never
    /// writes over a file it could not read (spec §5.4).
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                Error::ClaudeFileMissing(path.to_path_buf())
            } else {
                Error::Io {
                    path: path.to_path_buf(),
                    source,
                }
            }
        })?;

        let value: Value = serde_json::from_str(&raw).map_err(|source| Error::Parse {
            path: path.to_path_buf(),
            source,
        })?;

        Ok(Self {
            style: Style::detect(&raw),
            trailing_newline: raw.ends_with('\n'),
            value,
        })
    }

    /// Like [`load`], but a missing file yields an empty object rather than an
    /// error. Used for byte's own files, which may not exist on first run.
    pub fn load_or_empty(path: &Path) -> Result<Self> {
        match Self::load(path) {
            Err(Error::ClaudeFileMissing(_)) => Ok(Self {
                value: Value::Object(serde_json::Map::new()),
                style: Style::Pretty,
                trailing_newline: true,
            }),
            other => other,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.value.get(key)
    }

    /// Replace (or insert) a top-level key. An existing key keeps its position
    /// in the document.
    pub fn set(&mut self, key: &str, value: Value) {
        if let Value::Object(map) = &mut self.value {
            map.insert(key.to_string(), value);
        }
    }

    pub fn remove(&mut self, key: &str) {
        if let Value::Object(map) = &mut self.value {
            map.shift_remove(key);
        }
    }

    /// Serialise in the document's original style.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut text = match self.style {
            Style::Pretty => serde_json::to_string_pretty(&self.value),
            Style::Compact => serde_json::to_string(&self.value),
        }
        .map_err(|source| Error::Parse {
            path: std::path::PathBuf::from("<memory>"),
            source,
        })?;

        if self.trailing_newline {
            text.push('\n');
        }
        Ok(text.into_bytes())
    }

    /// Back up, write atomically, then verify. On verification failure the
    /// backup is restored and an error is returned (spec §5.3, §5.5).
    pub fn save(&self, path: &Path, backup_dir: &Path) -> Result<()> {
        let bytes = self.to_bytes()?;
        let saved = atomic::backup(path, backup_dir)?;

        atomic::write(path, &bytes)?;

        match Self::load(path) {
            Ok(reread) if reread.value == self.value => {
                if let Some(name) = path.file_name() {
                    atomic::prune(backup_dir, &name.to_string_lossy(), 10)?;
                }
                Ok(())
            }
            _ => {
                if let Some(saved) = saved {
                    atomic::restore(&saved, path)?;
                }
                Err(Error::VerifyFailed {
                    path: path.to_path_buf(),
                })
            }
        }
    }
}
```

Add `pub mod claude;` to `src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test document_test`
Expected: 10 passed.

If `key_order_is_never_alphabetised` fails, the `preserve_order` feature is missing from `Cargo.toml` — fix that before continuing. If `shift_remove` does not resolve, the same feature is missing (that method only exists on the `IndexMap` backing).

- [ ] **Step 5: Verify against the real file**

The fixture proves the logic; only the user's actual `~/.claude.json` proves
compatibility with how Claude Code formats it. `serde_json` and Node's
`JSON.stringify(x, null, 2)` agree on indentation and array expansion, but can
diverge on floating-point values (`1.0` vs `1`). This step catches that.

**This never touches the original file** — it works on a copy.

Create `examples/roundtrip_check.rs`:

```rust
//! Round-trips a real Claude config through JsonDocument and diffs the result.
//! Usage: cargo run --example roundtrip_check -- <path-to-json>

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let src = std::env::args().nth(1).ok_or("usage: roundtrip_check <file>")?;
    let original = std::fs::read_to_string(&src)?;

    let scratch = std::env::temp_dir().join("byte-roundtrip-check.json");
    std::fs::write(&scratch, &original)?;

    let doc = byte::claude::document::JsonDocument::load(&scratch)?;
    let rewritten = String::from_utf8(doc.to_bytes()?)?;

    if original == rewritten {
        println!("IDENTICAL — {} bytes round-tripped exactly", original.len());
    } else {
        println!("DIFFERS — original {} bytes, rewritten {} bytes", original.len(), rewritten.len());
        for (i, (a, b)) in original.lines().zip(rewritten.lines()).enumerate() {
            if a != b {
                println!("first difference at line {}:\n  original:  {a}\n  rewritten: {b}", i + 1);
                break;
            }
        }
    }
    std::fs::remove_file(&scratch).ok();
    Ok(())
}
```

Run it against both real files:

```bash
cargo run --example roundtrip_check -- "$HOME/.claude.json"
```

```bash
cargo run --example roundtrip_check -- "$HOME/.claude/.credentials.json"
```

Expected: `IDENTICAL` for both. If either differs, **stop** — record the exact
divergence in the research findings doc and fix `to_bytes` before continuing.
A byte-identical round-trip is the precondition for every write byte performs.

- [ ] **Step 6: Commit**

```bash
git add src/claude/ src/lib.rs tests/document_test.rs examples/roundtrip_check.rs
git commit -m "feat: add order- and format-preserving JSON document layer"
```

---

### Task 5: Account snapshots

**Files:**
- Create: `src/claude/snapshot.rs`
- Create: `src/claude/files.rs`
- Create: `tests/snapshot_test.rs`
- Modify: `src/claude/mod.rs`

**Interfaces:**
- Consumes: `JsonDocument` (Task 4), `HostPaths` (Task 2).
- Produces: `AccountSnapshot { schema, oauth, account, user_id }` with `account_uuid()`, `email()`, `organization_name()`, `subscription_type()`, `validate()`; `SCHEMA_VERSION: u32`; `ClaudeFiles::new(P)` with `capture() -> Result<Option<AccountSnapshot>>`, `apply(&AccountSnapshot) -> Result<()>`, `clear() -> Result<()>`.

- [ ] **Step 1: Write the failing test**

Create `tests/snapshot_test.rs`:

```rust
use byte::claude::files::ClaudeFiles;
use byte::claude::snapshot::AccountSnapshot;
use byte::paths::{HostPaths, TestPaths};
use serde_json::json;

fn seed(tp: &TestPaths, uuid: &str, email: &str, refresh: &str) {
    std::fs::write(
        tp.claude_credentials(),
        serde_json::to_string(&json!({
            "mcpOAuth": {"server-a": {"accessToken": "keep-me"}},
            "claudeAiOauth": {
                "accessToken": "access-1",
                "refreshToken": refresh,
                "expiresAt": 1234567890i64,
                "scopes": ["a", "b"],
                "subscriptionType": "max"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    std::fs::write(
        tp.claude_config(),
        serde_json::to_string_pretty(&json!({
            "numStartups": 60,
            "oauthAccount": {
                "accountUuid": uuid,
                "emailAddress": email,
                "organizationName": "Test Org"
            },
            "userID": "user-1",
            "projects": {"/x": {"history": [1]}}
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn capture_reads_the_live_account() {
    let tp = TestPaths::new().unwrap();
    seed(&tp, "uuid-1", "a@example.com", "refresh-1");

    let snap = ClaudeFiles::new(&tp).capture().unwrap().unwrap();

    assert_eq!(snap.account_uuid(), Some("uuid-1"));
    assert_eq!(snap.email(), Some("a@example.com"));
    assert_eq!(snap.organization_name(), Some("Test Org"));
    assert_eq!(snap.subscription_type(), Some("max"));
    assert_eq!(snap.user_id.as_deref(), Some("user-1"));
}

#[test]
fn capture_returns_none_when_logged_out() {
    let tp = TestPaths::new().unwrap();
    std::fs::write(tp.claude_credentials(), r#"{"mcpOAuth":{}}"#).unwrap();
    std::fs::write(tp.claude_config(), r#"{"numStartups":1}"#).unwrap();

    assert!(ClaudeFiles::new(&tp).capture().unwrap().is_none());
}

#[test]
fn apply_swaps_the_account_and_preserves_everything_else() {
    let tp = TestPaths::new().unwrap();
    seed(&tp, "uuid-1", "a@example.com", "refresh-1");
    let files = ClaudeFiles::new(&tp);

    let target = AccountSnapshot::new(
        json!({"accessToken": "access-2", "refreshToken": "refresh-2", "expiresAt": 99i64}),
        json!({"accountUuid": "uuid-2", "emailAddress": "b@example.com"}),
        Some("user-2".to_string()),
    );
    files.apply(&target).unwrap();

    let creds: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_credentials()).unwrap()).unwrap();
    let cfg: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_config()).unwrap()).unwrap();

    assert_eq!(creds["claudeAiOauth"]["refreshToken"], json!("refresh-2"));
    assert_eq!(cfg["oauthAccount"]["accountUuid"], json!("uuid-2"));
    assert_eq!(cfg["userID"], json!("user-2"));

    // Untouched neighbours.
    assert_eq!(creds["mcpOAuth"]["server-a"]["accessToken"], json!("keep-me"));
    assert_eq!(cfg["numStartups"], json!(60));
    assert_eq!(cfg["projects"]["/x"]["history"], json!([1]));
}

#[test]
fn capture_then_apply_round_trips_exactly() {
    let tp = TestPaths::new().unwrap();
    seed(&tp, "uuid-1", "a@example.com", "refresh-1");
    let files = ClaudeFiles::new(&tp);

    let before = files.capture().unwrap().unwrap();
    files.apply(&before).unwrap();
    let after = files.capture().unwrap().unwrap();

    assert_eq!(before, after);
}

#[test]
fn clear_removes_the_account_but_keeps_other_keys() {
    let tp = TestPaths::new().unwrap();
    seed(&tp, "uuid-1", "a@example.com", "refresh-1");
    let files = ClaudeFiles::new(&tp);

    files.clear().unwrap();

    assert!(files.capture().unwrap().is_none());
    let creds: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_credentials()).unwrap()).unwrap();
    assert_eq!(creds["mcpOAuth"]["server-a"]["accessToken"], json!("keep-me"));
}

#[test]
fn validate_rejects_a_snapshot_with_no_refresh_token() {
    let snap = AccountSnapshot::new(
        json!({"accessToken": "a", "refreshToken": ""}),
        json!({"accountUuid": "u"}),
        None,
    );
    assert!(snap.validate().is_err());
}

#[test]
fn validate_rejects_an_unknown_schema_version() {
    let mut snap = AccountSnapshot::new(
        json!({"refreshToken": "r"}),
        json!({"accountUuid": "u"}),
        None,
    );
    snap.schema = 999;
    assert!(snap.validate().is_err());
}

#[test]
fn validate_accepts_a_well_formed_snapshot() {
    let snap = AccountSnapshot::new(
        json!({"refreshToken": "r"}),
        json!({"accountUuid": "u"}),
        None,
    );
    assert!(snap.validate().is_ok());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test snapshot_test`
Expected: FAIL — unresolved imports.

- [ ] **Step 3: Write the snapshot type**

Create `src/claude/snapshot.rs`:

```rust
//! The unit of account identity that byte moves around.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

/// Bumped only when the *stored* representation changes incompatibly.
pub const SCHEMA_VERSION: u32 = 1;

/// Everything that makes Claude Code "logged in as" a particular account.
///
/// `oauth` and `account` are held as opaque JSON on purpose: byte does not
/// model their fields, so any field Anthropic adds travels through a capture
/// and apply cycle untouched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountSnapshot {
    pub schema: u32,
    /// The `claudeAiOauth` object from `.credentials.json`.
    pub oauth: Value,
    /// The `oauthAccount` object from `.claude.json`.
    pub account: Value,
    /// The `userID` value from `.claude.json`.
    pub user_id: Option<String>,
}

impl AccountSnapshot {
    pub fn new(oauth: Value, account: Value, user_id: Option<String>) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            oauth,
            account,
            user_id,
        }
    }

    fn account_str(&self, key: &str) -> Option<&str> {
        self.account.get(key).and_then(Value::as_str)
    }

    pub fn account_uuid(&self) -> Option<&str> {
        self.account_str("accountUuid")
    }

    pub fn email(&self) -> Option<&str> {
        self.account_str("emailAddress")
    }

    pub fn organization_name(&self) -> Option<&str> {
        self.account_str("organizationName")
    }

    pub fn organization_uuid(&self) -> Option<&str> {
        self.account_str("organizationUuid")
    }

    pub fn subscription_type(&self) -> Option<&str> {
        self.oauth
            .get("subscriptionType")
            .and_then(Value::as_str)
    }

    /// A stable identity for this account. Falls back to the email address
    /// when no UUID is present, which keeps pre-UUID snapshots usable.
    pub fn identity(&self) -> Option<&str> {
        self.account_uuid().or_else(|| self.email())
    }

    /// A human label, used when byte auto-names a captured account.
    pub fn default_label(&self) -> String {
        self.email()
            .or_else(|| self.account_uuid())
            .unwrap_or("unknown account")
            .to_string()
    }

    /// Refuse snapshots byte cannot safely write back (spec §7 step 4).
    pub fn validate(&self) -> Result<()> {
        let who = self.default_label();

        if self.schema != SCHEMA_VERSION {
            return Err(Error::SchemaMismatch {
                found: self.schema,
                expected: SCHEMA_VERSION,
            });
        }

        let refresh = self
            .oauth
            .get("refreshToken")
            .and_then(Value::as_str)
            .unwrap_or_default();

        if refresh.is_empty() {
            return Err(Error::InvalidSnapshot {
                account: who,
                reason: "no refresh token; re-authenticate this account with `byte add`".into(),
            });
        }

        if self.identity().is_none() {
            return Err(Error::InvalidSnapshot {
                account: who,
                reason: "no account UUID or email address".into(),
            });
        }

        Ok(())
    }
}
```

- [ ] **Step 4: Write the file-level capture/apply**

Create `src/claude/files.rs`:

```rust
//! Capturing and applying snapshots against the live Claude Code files.

use serde_json::Value;

use crate::claude::document::JsonDocument;
use crate::claude::snapshot::AccountSnapshot;
use crate::error::Result;
use crate::paths::HostPaths;

const OAUTH_KEY: &str = "claudeAiOauth";
const ACCOUNT_KEY: &str = "oauthAccount";
const USER_ID_KEY: &str = "userID";

/// Reads and writes the two files that hold Claude Code's account identity.
pub struct ClaudeFiles<P: HostPaths> {
    paths: P,
}

impl<P: HostPaths> ClaudeFiles<P> {
    pub fn new(paths: P) -> Self {
        Self { paths }
    }

    /// Snapshot whichever account is logged in right now.
    ///
    /// Returns `Ok(None)` when no account is present — a logged-out state is
    /// normal, not an error.
    pub fn capture(&self) -> Result<Option<AccountSnapshot>> {
        let creds = JsonDocument::load_or_empty(&self.paths.claude_credentials())?;
        let cfg = JsonDocument::load_or_empty(&self.paths.claude_config())?;

        let oauth = match creds.get(OAUTH_KEY) {
            Some(v) if !v.is_null() => v.clone(),
            _ => return Ok(None),
        };

        let account = cfg.get(ACCOUNT_KEY).cloned().unwrap_or(Value::Null);
        let user_id = cfg
            .get(USER_ID_KEY)
            .and_then(Value::as_str)
            .map(str::to_string);

        Ok(Some(AccountSnapshot::new(oauth, account, user_id)))
    }

    /// Make `snapshot` the logged-in account, leaving all other keys alone.
    pub fn apply(&self, snapshot: &AccountSnapshot) -> Result<()> {
        snapshot.validate()?;
        let backups = self.paths.backup_dir();

        let creds_path = self.paths.claude_credentials();
        let mut creds = JsonDocument::load_or_empty(&creds_path)?;
        creds.set(OAUTH_KEY, snapshot.oauth.clone());
        creds.save(&creds_path, &backups)?;

        let cfg_path = self.paths.claude_config();
        let mut cfg = JsonDocument::load_or_empty(&cfg_path)?;
        cfg.set(ACCOUNT_KEY, snapshot.account.clone());
        match &snapshot.user_id {
            Some(id) => cfg.set(USER_ID_KEY, Value::String(id.clone())),
            None => cfg.remove(USER_ID_KEY),
        }
        cfg.save(&cfg_path, &backups)?;

        Ok(())
    }

    /// Put Claude Code into a logged-out state, used by the add flow.
    pub fn clear(&self) -> Result<()> {
        let backups = self.paths.backup_dir();

        let creds_path = self.paths.claude_credentials();
        let mut creds = JsonDocument::load_or_empty(&creds_path)?;
        creds.remove(OAUTH_KEY);
        creds.save(&creds_path, &backups)?;

        let cfg_path = self.paths.claude_config();
        let mut cfg = JsonDocument::load_or_empty(&cfg_path)?;
        cfg.remove(ACCOUNT_KEY);
        cfg.remove(USER_ID_KEY);
        cfg.save(&cfg_path, &backups)?;

        Ok(())
    }
}
```

Update `src/claude/mod.rs`:

```rust
//! Reading and writing Claude Code's on-disk state.

pub mod document;
pub mod files;
pub mod snapshot;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test snapshot_test`
Expected: 8 passed.

- [ ] **Step 6: Commit**

```bash
git add src/claude/ tests/snapshot_test.rs
git commit -m "feat: add account snapshot capture and apply"
```

---

### Task 6: Account metadata store

**Files:**
- Create: `src/store/mod.rs`
- Create: `src/store/metadata.rs`
- Create: `tests/metadata_test.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `AccountSnapshot` (Task 5), `JsonDocument` (Task 4), `HostPaths` (Task 2).
- Produces: `AccountMeta { uuid, label, email, organization_name, subscription_type, added_at, last_used_at }`; `AccountsFile` with `load(&Path)`, `save(&Path, &Path)`, `upsert_from(&AccountSnapshot) -> AccountMeta`, `resolve(&str) -> Result<&AccountMeta>`, `remove(&str) -> Result<AccountMeta>`, `set_active(&str)`, `active_meta() -> Option<&AccountMeta>`.

- [ ] **Step 1: Write the failing test**

Create `tests/metadata_test.rs`:

```rust
use byte::claude::snapshot::AccountSnapshot;
use byte::paths::{HostPaths, TestPaths};
use byte::store::metadata::AccountsFile;
use serde_json::json;

fn snap(uuid: &str, email: &str) -> AccountSnapshot {
    AccountSnapshot::new(
        json!({"refreshToken": "r", "subscriptionType": "max"}),
        json!({"accountUuid": uuid, "emailAddress": email, "organizationName": "Org"}),
        Some("uid".to_string()),
    )
}

#[test]
fn upsert_adds_a_new_account_labelled_by_email() {
    let mut file = AccountsFile::default();
    let meta = file.upsert_from(&snap("u1", "a@example.com"));

    assert_eq!(meta.label, "a@example.com");
    assert_eq!(meta.uuid, "u1");
    assert_eq!(meta.subscription_type.as_deref(), Some("max"));
    assert_eq!(file.accounts.len(), 1);
}

#[test]
fn upsert_updates_in_place_and_keeps_a_custom_label() {
    let mut file = AccountsFile::default();
    file.upsert_from(&snap("u1", "a@example.com"));
    file.rename("u1", "work").unwrap();

    file.upsert_from(&snap("u1", "a@example.com"));

    assert_eq!(file.accounts.len(), 1);
    assert_eq!(file.accounts[0].label, "work");
}

#[test]
fn resolve_matches_label_then_email_then_uuid_prefix() {
    let mut file = AccountsFile::default();
    file.upsert_from(&snap("abcdef123456", "a@example.com"));
    file.rename("abcdef123456", "personal").unwrap();
    file.upsert_from(&snap("999999999999", "b@example.com"));

    assert_eq!(file.resolve("personal").unwrap().uuid, "abcdef123456");
    assert_eq!(file.resolve("b@example.com").unwrap().uuid, "999999999999");
    assert_eq!(file.resolve("abcdef").unwrap().uuid, "abcdef123456");
}

#[test]
fn resolve_is_case_insensitive() {
    let mut file = AccountsFile::default();
    file.upsert_from(&snap("u1", "Alice@Example.com"));

    assert_eq!(file.resolve("alice@example.com").unwrap().uuid, "u1");
}

#[test]
fn resolve_reports_an_unknown_name() {
    let file = AccountsFile::default();
    assert!(file.resolve("nobody").is_err());
}

#[test]
fn resolve_reports_ambiguity_rather_than_guessing() {
    let mut file = AccountsFile::default();
    file.upsert_from(&snap("aaa111", "x@example.com"));
    file.upsert_from(&snap("aaa222", "y@example.com"));

    assert!(file.resolve("aaa").is_err());
}

#[test]
fn remove_deletes_the_account_and_clears_active_when_it_matches() {
    let mut file = AccountsFile::default();
    file.upsert_from(&snap("u1", "a@example.com"));
    file.set_active("u1");

    let removed = file.remove("u1").unwrap();

    assert_eq!(removed.uuid, "u1");
    assert!(file.accounts.is_empty());
    assert!(file.active.is_none());
}

#[test]
fn accounts_survive_a_save_and_load_cycle() {
    let tp = TestPaths::new().unwrap();
    let mut file = AccountsFile::default();
    file.upsert_from(&snap("u1", "a@example.com"));
    file.set_active("u1");
    file.save(&tp.accounts_file(), &tp.backup_dir()).unwrap();

    let loaded = AccountsFile::load(&tp.accounts_file()).unwrap();

    assert_eq!(loaded.accounts.len(), 1);
    assert_eq!(loaded.active.as_deref(), Some("u1"));
    assert_eq!(loaded.accounts[0].email.as_deref(), Some("a@example.com"));
}

#[test]
fn loading_a_missing_file_yields_an_empty_store() {
    let tp = TestPaths::new().unwrap();
    let loaded = AccountsFile::load(&tp.accounts_file()).unwrap();
    assert!(loaded.accounts.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test metadata_test`
Expected: FAIL — unresolved import `byte::store`.

- [ ] **Step 3: Write the implementation**

Create `src/store/mod.rs`. It declares only `metadata` for now — `secrets.rs`
does not exist until Task 7, and declaring a module whose file is missing is a
compile error:

```rust
//! byte's own persisted state.

pub mod metadata;
```

Create `src/store/metadata.rs`:

```rust
//! Non-secret account metadata, stored as plain JSON.
//!
//! Kept separate from the secret store so that listing accounts — including
//! rendering the tray menu — never needs to unlock the OS keychain.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::atomic;
use crate::claude::snapshot::AccountSnapshot;
use crate::error::{Error, Result};

const METADATA_SCHEMA: u32 = 1;

/// Everything shown about an account without touching its secrets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountMeta {
    pub uuid: String,
    pub label: String,
    pub email: Option<String>,
    pub organization_name: Option<String>,
    pub subscription_type: Option<String>,
    pub added_at: String,
    pub last_used_at: Option<String>,
}

/// The contents of `accounts.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountsFile {
    pub schema: u32,
    pub active: Option<String>,
    pub accounts: Vec<AccountMeta>,
}

impl Default for AccountsFile {
    fn default() -> Self {
        Self {
            schema: METADATA_SCHEMA,
            active: None,
            accounts: Vec::new(),
        }
    }
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| String::from("unknown"))
}

impl AccountsFile {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => {
                return Err(Error::Io {
                    path: path.to_path_buf(),
                    source,
                })
            }
        };

        serde_json::from_str(&raw).map_err(|source| Error::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn save(&self, path: &Path, backup_dir: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(|source| Error::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        atomic::backup(path, backup_dir)?;
        atomic::write(path, format!("{text}\n").as_bytes())
    }

    /// Insert or refresh the entry for a snapshot's account, returning the
    /// stored metadata. A label the user set with `rename` is preserved.
    pub fn upsert_from(&mut self, snapshot: &AccountSnapshot) -> AccountMeta {
        let uuid = snapshot
            .identity()
            .unwrap_or("unknown")
            .to_string();

        let existing = self.accounts.iter().position(|a| a.uuid == uuid);

        let meta = AccountMeta {
            uuid: uuid.clone(),
            label: match existing {
                Some(i) => self.accounts[i].label.clone(),
                None => snapshot.default_label(),
            },
            email: snapshot.email().map(str::to_string),
            organization_name: snapshot.organization_name().map(str::to_string),
            subscription_type: snapshot.subscription_type().map(str::to_string),
            added_at: match existing {
                Some(i) => self.accounts[i].added_at.clone(),
                None => now_rfc3339(),
            },
            last_used_at: existing.and_then(|i| self.accounts[i].last_used_at.clone()),
        };

        match existing {
            Some(i) => self.accounts[i] = meta.clone(),
            None => self.accounts.push(meta.clone()),
        }
        meta
    }

    /// Find an account by label, email, or UUID prefix.
    pub fn resolve(&self, query: &str) -> Result<&AccountMeta> {
        let q = query.trim().to_lowercase();

        let exact: Vec<&AccountMeta> = self
            .accounts
            .iter()
            .filter(|a| {
                a.label.to_lowercase() == q
                    || a.email.as_deref().map(str::to_lowercase) == Some(q.clone())
                    || a.uuid.to_lowercase() == q
            })
            .collect();

        if exact.len() == 1 {
            return Ok(exact[0]);
        }
        if exact.len() > 1 {
            return Err(Error::AmbiguousAccount {
                query: query.to_string(),
                count: exact.len(),
            });
        }

        let prefixed: Vec<&AccountMeta> = self
            .accounts
            .iter()
            .filter(|a| {
                a.uuid.to_lowercase().starts_with(&q) || a.label.to_lowercase().starts_with(&q)
            })
            .collect();

        match prefixed.len() {
            1 => Ok(prefixed[0]),
            0 => Err(Error::NoSuchAccount(query.to_string())),
            n => Err(Error::AmbiguousAccount {
                query: query.to_string(),
                count: n,
            }),
        }
    }

    pub fn rename(&mut self, uuid: &str, label: &str) -> Result<AccountMeta> {
        let idx = self
            .accounts
            .iter()
            .position(|a| a.uuid == uuid)
            .ok_or_else(|| Error::NoSuchAccount(uuid.to_string()))?;
        self.accounts[idx].label = label.to_string();
        Ok(self.accounts[idx].clone())
    }

    pub fn remove(&mut self, uuid: &str) -> Result<AccountMeta> {
        let idx = self
            .accounts
            .iter()
            .position(|a| a.uuid == uuid)
            .ok_or_else(|| Error::NoSuchAccount(uuid.to_string()))?;
        let removed = self.accounts.remove(idx);
        if self.active.as_deref() == Some(uuid) {
            self.active = None;
        }
        Ok(removed)
    }

    pub fn set_active(&mut self, uuid: &str) {
        self.active = Some(uuid.to_string());
        if let Some(a) = self.accounts.iter_mut().find(|a| a.uuid == uuid) {
            a.last_used_at = Some(now_rfc3339());
        }
    }

    pub fn active_meta(&self) -> Option<&AccountMeta> {
        let active = self.active.as_deref()?;
        self.accounts.iter().find(|a| a.uuid == active)
    }
}
```

Add `pub mod store;` to `src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test metadata_test`
Expected: 9 passed.

- [ ] **Step 5: Commit**

```bash
git add src/store/ src/lib.rs tests/metadata_test.rs
git commit -m "feat: add account metadata store with name resolution"
```

---

### Task 7: Secret store

**Files:**
- Create: `src/store/secrets.rs`
- Create: `tests/secrets_test.rs`

**Interfaces:**
- Consumes: `AccountSnapshot` (Task 5).
- Produces: `trait SecretStore { put, get, delete }`; `KeyringStore::new()`; `MemoryStore::new()`. Later tasks are generic over `S: SecretStore`.

- [ ] **Step 1: Write the failing test**

Create `tests/secrets_test.rs`. Only `MemoryStore` is exercised in CI — a keychain is not available on headless runners, and prompting for one would hang the suite.

```rust
use byte::claude::snapshot::AccountSnapshot;
use byte::store::secrets::{MemoryStore, SecretStore};
use serde_json::json;

fn snap(refresh: &str) -> AccountSnapshot {
    AccountSnapshot::new(
        json!({"refreshToken": refresh}),
        json!({"accountUuid": "u1"}),
        None,
    )
}

#[test]
fn get_returns_what_put_stored() {
    let store = MemoryStore::new();
    store.put("u1", &snap("r1")).unwrap();

    assert_eq!(store.get("u1").unwrap(), Some(snap("r1")));
}

#[test]
fn get_returns_none_for_an_unknown_account() {
    let store = MemoryStore::new();
    assert_eq!(store.get("missing").unwrap(), None);
}

#[test]
fn put_overwrites_an_existing_secret() {
    let store = MemoryStore::new();
    store.put("u1", &snap("old")).unwrap();
    store.put("u1", &snap("new")).unwrap();

    assert_eq!(store.get("u1").unwrap(), Some(snap("new")));
}

#[test]
fn delete_removes_the_secret() {
    let store = MemoryStore::new();
    store.put("u1", &snap("r1")).unwrap();
    store.delete("u1").unwrap();

    assert_eq!(store.get("u1").unwrap(), None);
}

#[test]
fn deleting_something_absent_is_not_an_error() {
    let store = MemoryStore::new();
    assert!(store.delete("missing").is_ok());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test secrets_test`
Expected: FAIL — unresolved import `byte::store::secrets`.

- [ ] **Step 3: Write the implementation**

Create `src/store/secrets.rs`:

```rust
//! Where account credentials are kept at rest.
//!
//! The whole snapshot is stored as one keychain entry per account. That keeps
//! the secret and the identity it belongs to together, so a half-written
//! account cannot occur.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::claude::snapshot::AccountSnapshot;
use crate::error::{Error, Result};

/// Identifies byte's entries within the OS credential store.
const SERVICE: &str = "byte-claude-account-switcher";

pub trait SecretStore: Send + Sync {
    fn put(&self, uuid: &str, snapshot: &AccountSnapshot) -> Result<()>;
    fn get(&self, uuid: &str) -> Result<Option<AccountSnapshot>>;
    fn delete(&self, uuid: &str) -> Result<()>;
}

/// The OS credential store: Windows Credential Manager, macOS Keychain, or
/// the Secret Service on Linux.
#[derive(Debug, Default)]
pub struct KeyringStore;

impl KeyringStore {
    pub fn new() -> Self {
        Self
    }

    fn entry(uuid: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, uuid).map_err(|e| Error::Secret(e.to_string()))
    }
}

impl SecretStore for KeyringStore {
    fn put(&self, uuid: &str, snapshot: &AccountSnapshot) -> Result<()> {
        let payload =
            serde_json::to_string(snapshot).map_err(|e| Error::Secret(e.to_string()))?;
        Self::entry(uuid)?
            .set_password(&payload)
            .map_err(|e| Error::Secret(e.to_string()))
    }

    fn get(&self, uuid: &str) -> Result<Option<AccountSnapshot>> {
        match Self::entry(uuid)?.get_password() {
            Ok(payload) => {
                let snap = serde_json::from_str(&payload)
                    .map_err(|e| Error::Secret(format!("stored snapshot is corrupt: {e}")))?;
                Ok(Some(snap))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::Secret(e.to_string())),
        }
    }

    fn delete(&self, uuid: &str) -> Result<()> {
        match Self::entry(uuid)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Error::Secret(e.to_string())),
        }
    }
}

/// An in-process store used by tests, so the suite never touches a real
/// keychain or blocks on an unlock prompt.
#[derive(Debug, Default)]
pub struct MemoryStore {
    inner: Mutex<HashMap<String, AccountSnapshot>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemoryStore {
    fn put(&self, uuid: &str, snapshot: &AccountSnapshot) -> Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| Error::Secret("memory store poisoned".into()))?;
        guard.insert(uuid.to_string(), snapshot.clone());
        Ok(())
    }

    fn get(&self, uuid: &str) -> Result<Option<AccountSnapshot>> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| Error::Secret("memory store poisoned".into()))?;
        Ok(guard.get(uuid).cloned())
    }

    fn delete(&self, uuid: &str) -> Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| Error::Secret("memory store poisoned".into()))?;
        guard.remove(uuid);
        Ok(())
    }
}
```

Now declare the module. Task 6 created `src/store/mod.rs` with only `metadata`;
add `secrets` to it so the file reads:

```rust
//! byte's own persisted state.

pub mod metadata;
pub mod secrets;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test secrets_test`
Expected: 5 passed.

If `delete_credential` does not resolve, check the `keyring` v3 API — earlier versions named it `delete_password`. Adjust and note it in the research findings doc.

- [ ] **Step 5: Commit**

```bash
git add src/store/secrets.rs tests/secrets_test.rs
git commit -m "feat: add keyring and in-memory secret stores"
```

---

### Task 8: Switch with sync-back

The core algorithm from spec §7. Sync-back is what keeps stored refresh tokens from going stale when Claude Code rotates them.

**Files:**
- Create: `src/ops/mod.rs`
- Create: `src/ops/switch.rs`
- Create: `tests/switch_test.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `ClaudeFiles` (Task 5), `AccountsFile`/`AccountMeta` (Task 6), `SecretStore` (Task 7), `HostPaths` (Task 2).
- Produces: `Switcher::new(paths, secrets)`; `sync_back() -> Result<SyncOutcome>`; `switch_to(&str) -> Result<SwitchOutcome>`; `capture_current() -> Result<AccountMeta>`; `SyncOutcome { Updated(AccountMeta), Captured(AccountMeta), LoggedOut }`; `SwitchOutcome { switched_to: AccountMeta, sync: SyncOutcome, already_active: bool }`.

- [ ] **Step 1: Write the failing test**

Create `tests/switch_test.rs`:

```rust
use byte::claude::files::ClaudeFiles;
use byte::ops::switch::{SwitchOutcome, SyncOutcome, Switcher};
use byte::paths::{HostPaths, TestPaths};
use byte::store::metadata::AccountsFile;
use byte::store::secrets::{MemoryStore, SecretStore};
use serde_json::json;

fn login_as(tp: &TestPaths, uuid: &str, email: &str, refresh: &str) {
    std::fs::write(
        tp.claude_credentials(),
        serde_json::to_string(&json!({
            "mcpOAuth": {"srv": {"accessToken": "keep"}},
            "claudeAiOauth": {
                "accessToken": "a", "refreshToken": refresh,
                "expiresAt": 1i64, "subscriptionType": "max"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        tp.claude_config(),
        serde_json::to_string_pretty(&json!({
            "numStartups": 7,
            "oauthAccount": {"accountUuid": uuid, "emailAddress": email},
            "userID": format!("uid-{uuid}")
        }))
        .unwrap(),
    )
    .unwrap();
}

fn live_refresh(tp: &TestPaths) -> String {
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_credentials()).unwrap()).unwrap();
    v["claudeAiOauth"]["refreshToken"].as_str().unwrap().to_string()
}

#[test]
fn capture_current_stores_the_live_account() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());

    let meta = sw.capture_current().unwrap();

    assert_eq!(meta.uuid, "u1");
    let file = AccountsFile::load(&tp.accounts_file()).unwrap();
    assert_eq!(file.accounts.len(), 1);
    assert_eq!(file.active.as_deref(), Some("u1"));
}

#[test]
fn sync_back_captures_an_account_byte_has_never_seen() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());

    match sw.sync_back().unwrap() {
        SyncOutcome::Captured(m) => assert_eq!(m.uuid, "u1"),
        other => panic!("expected Captured, got {other:?}"),
    }
}

#[test]
fn sync_back_refreshes_a_rotated_token() {
    // The scenario spec section 7.1 exists to defend against.
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "original");
    let sw = Switcher::new(&tp, MemoryStore::new());
    sw.capture_current().unwrap();

    // Claude Code refreshes and rotates the token behind byte's back.
    login_as(&tp, "u1", "a@example.com", "rotated");

    match sw.sync_back().unwrap() {
        SyncOutcome::Updated(m) => assert_eq!(m.uuid, "u1"),
        other => panic!("expected Updated, got {other:?}"),
    }
    assert_eq!(
        sw.secrets().get("u1").unwrap().unwrap().oauth["refreshToken"],
        json!("rotated")
    );
}

#[test]
fn sync_back_reports_logged_out_when_no_account_is_present() {
    let tp = TestPaths::new().unwrap();
    std::fs::write(tp.claude_credentials(), "{}").unwrap();
    std::fs::write(tp.claude_config(), "{}").unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());

    assert!(matches!(sw.sync_back().unwrap(), SyncOutcome::LoggedOut));
}

#[test]
fn switching_writes_the_target_account_to_the_live_files() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    sw.capture_current().unwrap();

    login_as(&tp, "u2", "b@example.com", "r2");
    sw.capture_current().unwrap();

    let out: SwitchOutcome = sw.switch_to("a@example.com").unwrap();

    assert_eq!(out.switched_to.uuid, "u1");
    assert_eq!(live_refresh(&tp), "r1");
    let snap = ClaudeFiles::new(&tp).capture().unwrap().unwrap();
    assert_eq!(snap.email(), Some("a@example.com"));
}

#[test]
fn switching_away_saves_the_current_account_first() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    sw.capture_current().unwrap();
    login_as(&tp, "u2", "b@example.com", "r2");
    sw.capture_current().unwrap();

    // u2 is live; rotate it, then switch away without capturing.
    login_as(&tp, "u2", "b@example.com", "r2-rotated");
    sw.switch_to("a@example.com").unwrap();

    assert_eq!(
        sw.secrets().get("u2").unwrap().unwrap().oauth["refreshToken"],
        json!("r2-rotated")
    );
}

#[test]
fn switching_back_and_forth_round_trips_cleanly() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    sw.capture_current().unwrap();
    login_as(&tp, "u2", "b@example.com", "r2");
    sw.capture_current().unwrap();

    sw.switch_to("a@example.com").unwrap();
    assert_eq!(live_refresh(&tp), "r1");
    sw.switch_to("b@example.com").unwrap();
    assert_eq!(live_refresh(&tp), "r2");
    sw.switch_to("a@example.com").unwrap();
    assert_eq!(live_refresh(&tp), "r1");
}

#[test]
fn switching_preserves_unrelated_keys_in_both_files() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    sw.capture_current().unwrap();
    login_as(&tp, "u2", "b@example.com", "r2");
    sw.capture_current().unwrap();

    sw.switch_to("a@example.com").unwrap();

    let creds: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_credentials()).unwrap()).unwrap();
    let cfg: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tp.claude_config()).unwrap()).unwrap();
    assert_eq!(creds["mcpOAuth"]["srv"]["accessToken"], json!("keep"));
    assert_eq!(cfg["numStartups"], json!(7));
}

#[test]
fn switching_to_the_active_account_is_a_no_op_that_still_syncs() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    sw.capture_current().unwrap();

    let out = sw.switch_to("a@example.com").unwrap();

    assert!(out.already_active);
    assert_eq!(live_refresh(&tp), "r1");
}

#[test]
fn switching_to_an_unknown_account_fails_without_touching_the_files() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    sw.capture_current().unwrap();

    assert!(sw.switch_to("nobody@example.com").is_err());
    assert_eq!(live_refresh(&tp), "r1");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test switch_test`
Expected: FAIL — unresolved import `byte::ops`.

- [ ] **Step 3: Write the implementation**

Create `src/ops/mod.rs`:

```rust
//! Operations composed from the storage and file layers.

pub mod switch;
```

Create `src/ops/switch.rs`:

```rust
//! Capturing, syncing back, and switching accounts (spec §7).

use crate::claude::files::ClaudeFiles;
use crate::error::{Error, Result};
use crate::paths::HostPaths;
use crate::store::metadata::{AccountMeta, AccountsFile};
use crate::store::secrets::SecretStore;

/// What sync-back did before a switch proceeded.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncOutcome {
    /// A known account's stored credentials were refreshed from disk.
    Updated(AccountMeta),
    /// A live account byte had never seen was saved (spec §7.2).
    Captured(AccountMeta),
    /// Nothing was logged in.
    LoggedOut,
}

/// The result of a completed switch.
#[derive(Debug, Clone)]
pub struct SwitchOutcome {
    pub switched_to: AccountMeta,
    pub sync: SyncOutcome,
    pub already_active: bool,
}

pub struct Switcher<P: HostPaths, S: SecretStore> {
    paths: P,
    secrets: S,
}

impl<P: HostPaths + Copy, S: SecretStore> Switcher<P, S> {
    pub fn new(paths: P, secrets: S) -> Self {
        Self { paths, secrets }
    }

    pub fn secrets(&self) -> &S {
        &self.secrets
    }

    fn files(&self) -> ClaudeFiles<P> {
        ClaudeFiles::new(self.paths)
    }

    fn load_accounts(&self) -> Result<AccountsFile> {
        AccountsFile::load(&self.paths.accounts_file())
    }

    fn save_accounts(&self, file: &AccountsFile) -> Result<()> {
        file.save(&self.paths.accounts_file(), &self.paths.backup_dir())
    }

    /// Save whatever account is live right now, then mark it active.
    pub fn capture_current(&self) -> Result<AccountMeta> {
        let snapshot = self.files().capture()?.ok_or(Error::NotLoggedIn)?;
        snapshot.validate()?;

        let uuid = snapshot
            .identity()
            .ok_or_else(|| Error::InvalidSnapshot {
                account: snapshot.default_label(),
                reason: "no account UUID or email address".into(),
            })?
            .to_string();

        let mut accounts = self.load_accounts()?;
        let meta = accounts.upsert_from(&snapshot);
        accounts.set_active(&uuid);

        self.secrets.put(&uuid, &snapshot)?;
        self.save_accounts(&accounts)?;
        Ok(meta)
    }

    /// Copy the live credentials into the store, so a token Claude Code
    /// rotated behind byte's back is not lost.
    pub fn sync_back(&self) -> Result<SyncOutcome> {
        let Some(snapshot) = self.files().capture()? else {
            return Ok(SyncOutcome::LoggedOut);
        };
        if snapshot.validate().is_err() {
            return Ok(SyncOutcome::LoggedOut);
        }

        let Some(uuid) = snapshot.identity().map(str::to_string) else {
            return Ok(SyncOutcome::LoggedOut);
        };

        let mut accounts = self.load_accounts()?;
        let known = accounts.accounts.iter().any(|a| a.uuid == uuid);

        let meta = accounts.upsert_from(&snapshot);
        self.secrets.put(&uuid, &snapshot)?;
        self.save_accounts(&accounts)?;

        Ok(if known {
            SyncOutcome::Updated(meta)
        } else {
            SyncOutcome::Captured(meta)
        })
    }

    /// Switch to a stored account, syncing the current one back first.
    pub fn switch_to(&self, query: &str) -> Result<SwitchOutcome> {
        // Resolve before touching anything, so an unknown name is a clean
        // no-op rather than a half-applied switch.
        let target_uuid = {
            let accounts = self.load_accounts()?;
            accounts.resolve(query)?.uuid.clone()
        };

        let sync = self.sync_back()?;

        let snapshot = self
            .secrets
            .get(&target_uuid)?
            .ok_or_else(|| Error::InvalidSnapshot {
                account: target_uuid.clone(),
                reason: "no stored credentials; re-authenticate with `byte add`".into(),
            })?;
        snapshot.validate()?;

        let mut accounts = self.load_accounts()?;
        let already_active = accounts.active.as_deref() == Some(target_uuid.as_str());

        self.files().apply(&snapshot)?;

        accounts.upsert_from(&snapshot);
        accounts.set_active(&target_uuid);
        self.save_accounts(&accounts)?;

        let switched_to = accounts
            .resolve(&target_uuid)
            .map(Clone::clone)
            .map_err(|_| Error::NoSuchAccount(target_uuid.clone()))?;

        Ok(SwitchOutcome {
            switched_to,
            sync,
            already_active,
        })
    }
}
```

Add `pub mod ops;` to `src/lib.rs`.

Note: `Switcher` is generic over `P: HostPaths + Copy` so callers can pass
`&TestPaths` or `&RealPaths` (shared references are `Copy`). The blanket
`impl<T: HostPaths + ?Sized> HostPaths for &T` that makes this work was added
in Task 2; no change to `src/paths.rs` is needed here.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test switch_test`
Expected: 10 passed.

- [ ] **Step 5: Run the whole suite and lint**

Run: `make test && make lint && make fmt-check`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/ops/ src/paths.rs src/lib.rs tests/switch_test.rs
git commit -m "feat: add account switching with sync-back"
```

---

### Task 9: The add flow

Spec §8. The dangerous part is step 2: byte deliberately logs Claude Code out. It must never do that unless the outgoing account is provably recoverable.

**Files:**
- Create: `src/ops/add.rs`
- Create: `tests/add_test.rs`
- Modify: `src/ops/mod.rs`

**Interfaces:**
- Consumes: `Switcher` (Task 8), `ClaudeFiles` (Task 5).
- Produces: `AddSession::begin(&Switcher) -> Result<AddSession>`; `AddSession::poll_once() -> Result<Option<AccountMeta>>`; `AddSession::abort() -> Result<()>`; `AddSession::previous() -> Option<&AccountMeta>`.

Polling is exposed as a single step rather than a blocking loop so that the CLI can drive it with a timeout and, later, the tray can drive it from its event loop without blocking.

- [ ] **Step 1: Write the failing test**

Create `tests/add_test.rs`:

```rust
use byte::claude::files::ClaudeFiles;
use byte::ops::add::AddSession;
use byte::ops::switch::Switcher;
use byte::paths::{HostPaths, TestPaths};
use byte::store::secrets::{MemoryStore, SecretStore};
use serde_json::json;

fn login_as(tp: &TestPaths, uuid: &str, email: &str, refresh: &str) {
    std::fs::write(
        tp.claude_credentials(),
        serde_json::to_string(&json!({
            "claudeAiOauth": {
                "accessToken": "a", "refreshToken": refresh, "expiresAt": 1i64
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        tp.claude_config(),
        serde_json::to_string_pretty(&json!({
            "oauthAccount": {"accountUuid": uuid, "emailAddress": email},
            "userID": "uid"
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn begin_saves_the_outgoing_account_then_logs_out() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());

    let session = AddSession::begin(&sw).unwrap();

    assert_eq!(session.previous().unwrap().uuid, "u1");
    assert!(ClaudeFiles::new(&tp).capture().unwrap().is_none());
    // Recoverable: the outgoing credentials are in the store.
    assert!(sw.secrets().get("u1").unwrap().is_some());
}

#[test]
fn begin_works_from_a_logged_out_state() {
    let tp = TestPaths::new().unwrap();
    std::fs::write(tp.claude_credentials(), "{}").unwrap();
    std::fs::write(tp.claude_config(), "{}").unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());

    let session = AddSession::begin(&sw).unwrap();

    assert!(session.previous().is_none());
}

#[test]
fn poll_returns_none_while_still_logged_out() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    let session = AddSession::begin(&sw).unwrap();

    assert!(session.poll_once(&sw).unwrap().is_none());
}

#[test]
fn poll_captures_a_new_login() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    let session = AddSession::begin(&sw).unwrap();

    login_as(&tp, "u2", "b@example.com", "r2");
    let found = session.poll_once(&sw).unwrap().unwrap();

    assert_eq!(found.uuid, "u2");
    assert!(sw.secrets().get("u2").unwrap().is_some());
}

#[test]
fn poll_ignores_a_re_login_as_the_same_account() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    let session = AddSession::begin(&sw).unwrap();

    login_as(&tp, "u1", "a@example.com", "r1-again");

    assert!(session.poll_once(&sw).unwrap().is_none());
}

#[test]
fn abort_restores_the_previous_account() {
    let tp = TestPaths::new().unwrap();
    login_as(&tp, "u1", "a@example.com", "r1");
    let sw = Switcher::new(&tp, MemoryStore::new());
    let session = AddSession::begin(&sw).unwrap();

    session.abort(&sw).unwrap();

    let restored = ClaudeFiles::new(&tp).capture().unwrap().unwrap();
    assert_eq!(restored.email(), Some("a@example.com"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test add_test`
Expected: FAIL — unresolved import `byte::ops::add`.

- [ ] **Step 3: Write the implementation**

Create `src/ops/add.rs`:

```rust
//! Adding an account by logging Claude Code out and watching for a new login.

use crate::claude::files::ClaudeFiles;
use crate::error::{Error, Result};
use crate::ops::switch::Switcher;
use crate::paths::HostPaths;
use crate::store::metadata::AccountMeta;
use crate::store::secrets::SecretStore;

/// An in-progress add. Created by [`AddSession::begin`], driven by
/// [`AddSession::poll_once`], and cancelled by [`AddSession::abort`].
#[derive(Debug, Clone)]
pub struct AddSession {
    previous: Option<AccountMeta>,
}

impl AddSession {
    /// Save the current account, verify it is recoverable, then log out.
    ///
    /// The verification step is what makes this safe: byte only clears
    /// credentials it has already read back out of the store.
    pub fn begin<P: HostPaths + Copy, S: SecretStore>(sw: &Switcher<P, S>) -> Result<Self> {
        let previous = match sw.capture_current() {
            Ok(meta) => {
                // Prove recoverability before destroying the live copy.
                if sw.secrets().get(&meta.uuid)?.is_none() {
                    return Err(Error::InvalidSnapshot {
                        account: meta.label.clone(),
                        reason: "could not be read back from the store; refusing to log out"
                            .into(),
                    });
                }
                Some(meta)
            }
            Err(Error::NotLoggedIn) => None,
            Err(e) => return Err(e),
        };

        sw.files_for_add().clear()?;
        Ok(Self { previous })
    }

    pub fn previous(&self) -> Option<&AccountMeta> {
        self.previous.as_ref()
    }

    /// Check once for a completed login as a *different* account.
    pub fn poll_once<P: HostPaths + Copy, S: SecretStore>(
        &self,
        sw: &Switcher<P, S>,
    ) -> Result<Option<AccountMeta>> {
        let Some(snapshot) = sw.files_for_add().capture()? else {
            return Ok(None);
        };
        if snapshot.validate().is_err() {
            return Ok(None);
        }

        let identity = snapshot.identity().unwrap_or_default();
        if let Some(prev) = &self.previous {
            if identity == prev.uuid {
                return Ok(None);
            }
        }

        Ok(Some(sw.capture_current()?))
    }

    /// Give up and put the previous account back.
    pub fn abort<P: HostPaths + Copy, S: SecretStore>(&self, sw: &Switcher<P, S>) -> Result<()> {
        match &self.previous {
            Some(prev) => {
                sw.switch_to(&prev.uuid)?;
                Ok(())
            }
            None => Ok(()),
        }
    }
}
```

Add to `src/ops/switch.rs`, inside `impl<P: HostPaths + Copy, S: SecretStore> Switcher<P, S>`:

```rust
    /// Exposed for the add flow, which needs direct file access.
    pub fn files_for_add(&self) -> ClaudeFiles<P> {
        self.files()
    }
```

Update `src/ops/mod.rs`:

```rust
//! Operations composed from the storage and file layers.

pub mod add;
pub mod switch;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test add_test`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add src/ops/ tests/add_test.rs
git commit -m "feat: add logout-and-watch account add flow"
```

---

### Task 10: List, remove, rename, current

**Files:**
- Create: `src/ops/manage.rs`
- Create: `tests/manage_test.rs`
- Modify: `src/ops/mod.rs`

**Interfaces:**
- Consumes: `Switcher` (Task 8), `AccountsFile`/`AccountMeta` (Task 6), `SecretStore` (Task 7).
- Produces: `list(&Switcher) -> Result<Vec<AccountListing>>`; `current(&Switcher) -> Result<Option<AccountMeta>>`; `remove(&Switcher, &str) -> Result<AccountMeta>`; `rename(&Switcher, &str, &str) -> Result<AccountMeta>`; `AccountListing { meta: AccountMeta, active: bool }`.

- [ ] **Step 1: Write the failing test**

Create `tests/manage_test.rs`:

```rust
use byte::ops::manage;
use byte::ops::switch::Switcher;
use byte::paths::{HostPaths, TestPaths};
use byte::store::secrets::{MemoryStore, SecretStore};
use serde_json::json;

fn login_as(tp: &TestPaths, uuid: &str, email: &str) {
    std::fs::write(
        tp.claude_credentials(),
        serde_json::to_string(&json!({
            "claudeAiOauth": {"refreshToken": "r", "expiresAt": 1i64}
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        tp.claude_config(),
        serde_json::to_string_pretty(&json!({
            "oauthAccount": {"accountUuid": uuid, "emailAddress": email},
            "userID": "uid"
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn list_is_empty_before_anything_is_captured() {
    let tp = TestPaths::new().unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());
    assert!(manage::list(&sw).unwrap().is_empty());
}

#[test]
fn list_marks_exactly_one_account_active() {
    let tp = TestPaths::new().unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());
    login_as(&tp, "u1", "a@example.com");
    sw.capture_current().unwrap();
    login_as(&tp, "u2", "b@example.com");
    sw.capture_current().unwrap();

    let listing = manage::list(&sw).unwrap();

    assert_eq!(listing.len(), 2);
    assert_eq!(listing.iter().filter(|l| l.active).count(), 1);
    assert!(listing.iter().find(|l| l.active).unwrap().meta.uuid == "u2");
}

#[test]
fn current_returns_the_active_account() {
    let tp = TestPaths::new().unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());
    login_as(&tp, "u1", "a@example.com");
    sw.capture_current().unwrap();

    assert_eq!(manage::current(&sw).unwrap().unwrap().uuid, "u1");
}

#[test]
fn rename_changes_the_label_and_it_is_then_resolvable() {
    let tp = TestPaths::new().unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());
    login_as(&tp, "u1", "a@example.com");
    sw.capture_current().unwrap();

    manage::rename(&sw, "a@example.com", "work").unwrap();

    assert_eq!(manage::current(&sw).unwrap().unwrap().label, "work");
    assert!(sw.switch_to("work").is_ok());
}

#[test]
fn remove_deletes_both_metadata_and_secret() {
    let tp = TestPaths::new().unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());
    login_as(&tp, "u1", "a@example.com");
    sw.capture_current().unwrap();

    manage::remove(&sw, "a@example.com").unwrap();

    assert!(manage::list(&sw).unwrap().is_empty());
    assert!(sw.secrets().get("u1").unwrap().is_none());
}

#[test]
fn removing_an_unknown_account_errors() {
    let tp = TestPaths::new().unwrap();
    let sw = Switcher::new(&tp, MemoryStore::new());
    assert!(manage::remove(&sw, "nobody").is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test manage_test`
Expected: FAIL — unresolved import `byte::ops::manage`.

- [ ] **Step 3: Write the implementation**

Create `src/ops/manage.rs`:

```rust
//! Listing and maintaining stored accounts.

use crate::error::Result;
use crate::ops::switch::Switcher;
use crate::paths::HostPaths;
use crate::store::metadata::{AccountMeta, AccountsFile};
use crate::store::secrets::SecretStore;

/// An account plus whether it is the active one.
#[derive(Debug, Clone)]
pub struct AccountListing {
    pub meta: AccountMeta,
    pub active: bool,
}

fn accounts<P: HostPaths + Copy, S: SecretStore>(sw: &Switcher<P, S>) -> Result<AccountsFile> {
    AccountsFile::load(&sw.paths().accounts_file())
}

pub fn list<P: HostPaths + Copy, S: SecretStore>(
    sw: &Switcher<P, S>,
) -> Result<Vec<AccountListing>> {
    let file = accounts(sw)?;
    Ok(file
        .accounts
        .iter()
        .map(|meta| AccountListing {
            active: file.active.as_deref() == Some(meta.uuid.as_str()),
            meta: meta.clone(),
        })
        .collect())
}

pub fn current<P: HostPaths + Copy, S: SecretStore>(
    sw: &Switcher<P, S>,
) -> Result<Option<AccountMeta>> {
    Ok(accounts(sw)?.active_meta().cloned())
}

pub fn rename<P: HostPaths + Copy, S: SecretStore>(
    sw: &Switcher<P, S>,
    query: &str,
    label: &str,
) -> Result<AccountMeta> {
    let mut file = accounts(sw)?;
    let uuid = file.resolve(query)?.uuid.clone();
    let meta = file.rename(&uuid, label)?;
    sw.save_accounts_public(&file)?;
    Ok(meta)
}

pub fn remove<P: HostPaths + Copy, S: SecretStore>(
    sw: &Switcher<P, S>,
    query: &str,
) -> Result<AccountMeta> {
    let mut file = accounts(sw)?;
    let uuid = file.resolve(query)?.uuid.clone();
    let meta = file.remove(&uuid)?;
    sw.secrets().delete(&uuid)?;
    sw.save_accounts_public(&file)?;
    Ok(meta)
}
```

Add these two accessors to `src/ops/switch.rs`, inside the same `impl` block:

```rust
    pub fn paths(&self) -> &P {
        &self.paths
    }

    pub fn save_accounts_public(&self, file: &AccountsFile) -> Result<()> {
        self.save_accounts(file)
    }
```

Update `src/ops/mod.rs`:

```rust
//! Operations composed from the storage and file layers.

pub mod add;
pub mod manage;
pub mod switch;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test manage_test`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add src/ops/ tests/manage_test.rs
git commit -m "feat: add list, current, rename, and remove operations"
```

---

### Task 11: CLI, documentation, and man page

**Files:**
- Create: `src/cli/mod.rs`
- Create: `src/cli/run.rs`
- Modify: `src/main.rs`, `src/output.rs`, `src/lib.rs`
- Create: `tests/cli_test.rs`
- Create: `man/byte.md`
- Modify: `README.md`, `docs/getting-started.md`, `docs/configuration.md`, `docs/troubleshooting.md`, `docs/architecture.md`, `SECURITY.md`

**Interfaces:**
- Consumes: every op from Tasks 8–10.
- Produces: the `byte` binary's command surface.

- [ ] **Step 1: Write the failing test**

Create `tests/cli_test.rs`.

**Scope note — read before writing these.** The real binary uses
`KeyringStore`, so any command that reads or writes a secret would touch the
user's actual OS keychain. CI runners have no unlocked keychain, and a test
suite must never write to a developer's real credential store. So these tests
cover only what is unique to the CLI layer and touches no secrets: argument
parsing, help and version output, exit codes, and JSON output shape. Switching,
capturing, sync-back, and the add flow are already covered end to end against
`MemoryStore` in Tasks 8–10, which is where that behavior belongs.

```rust
use std::process::Command;

use byte::paths::{HostPaths, TestPaths};

/// Runs the real binary against a throwaway config directory.
///
/// `CLAUDE_CONFIG_DIR` places `.claude.json` and `.credentials.json` directly
/// in the given directory — a flatter layout than a real home directory.
fn byte(tp: &TestPaths, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_byte"))
        .args(args)
        .env("CLAUDE_CONFIG_DIR", tp.root())
        .env("BYTE_CONFIG_DIR", tp.byte_config_dir())
        .output()
        .expect("failed to run byte")
}

#[test]
fn list_on_an_empty_store_succeeds() {
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["list"]);
    assert!(out.status.success());
}

#[test]
fn list_json_on_an_empty_store_is_an_empty_array() {
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["list", "--json"]);

    assert!(out.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(parsed, serde_json::json!([]));
}

#[test]
fn json_output_goes_to_stdout_and_status_text_does_not() {
    // Guards the output split: --json must be pipeable.
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["list", "--json"]);

    assert!(serde_json::from_slice::<serde_json::Value>(&out.stdout).is_ok());
}

#[test]
fn current_json_on_an_empty_store_is_null() {
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["current", "--json"]);

    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "null");
}

#[test]
fn switching_to_an_unknown_account_exits_non_zero() {
    // resolve() fails before anything reaches the keychain.
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["switch", "nobody"]);

    assert!(!out.status.success());
}

#[test]
fn an_unknown_subcommand_exits_non_zero() {
    let tp = TestPaths::new().unwrap();
    assert!(!byte(&tp, &["frobnicate"]).status.success());
}

#[test]
fn help_lists_every_command() {
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["--help"]);
    let text = String::from_utf8_lossy(&out.stdout);

    for cmd in ["list", "switch", "add", "capture", "remove", "rename", "current"] {
        assert!(text.contains(cmd), "help is missing '{cmd}':\n{text}");
    }
}

#[test]
fn version_prints_the_crate_version() {
    let tp = TestPaths::new().unwrap();
    let out = byte(&tp, &["--version"]);

    assert!(String::from_utf8_lossy(&out.stdout).contains(env!("CARGO_PKG_VERSION")));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test cli_test`
Expected: FAIL — no subcommands exist yet.

- [ ] **Step 3: Add a stdout helper to `src/output.rs`**

Every other helper writes to stderr; machine-readable output must go to stdout so it can be piped. Append to `src/output.rs`:

```rust
/// Machine-readable output. This is the only helper that writes to stdout,
/// so `--json` can be piped without status messages contaminating it.
pub fn data(text: &str) {
    let _ = writeln!(std::io::stdout(), "{text}");
}
```

- [ ] **Step 4: Define the command surface**

Create `src/cli/mod.rs`:

```rust
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
        /// How long to wait for a login, in seconds.
        #[arg(long, default_value_t = 300)]
        timeout: u64,
    },
    /// Forget a stored account.
    Remove {
        /// Label, email address, or account UUID prefix.
        name: String,
    },
    /// Change an account's display label.
    Rename {
        /// The account to rename.
        name: String,
        /// The new label.
        label: String,
    },
}
```

- [ ] **Step 5: Implement dispatch**

Create `src/cli/run.rs`:

```rust
//! Executing CLI commands and rendering their results.

use crate::cli::{Cli, Command};
use crate::error::{Error, Result};
use crate::ops::add::AddSession;
use crate::ops::manage::{self, AccountListing};
use crate::ops::switch::{SwitchOutcome, SyncOutcome, Switcher};
use crate::output;
use crate::paths::RealPaths;
use crate::store::secrets::KeyringStore;

/// How often the add flow checks for a completed login.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

pub fn run(cli: Cli) -> Result<()> {
    let paths = RealPaths::discover()?;
    let switcher = Switcher::new(&paths, KeyringStore::new());

    match cli.command {
        None | Some(Command::List) => cmd_list(&switcher, cli.json),
        Some(Command::Current) => cmd_current(&switcher, cli.json),
        Some(Command::Switch { name }) => cmd_switch(&switcher, &name, cli.json),
        Some(Command::Capture) => cmd_capture(&switcher, cli.json),
        Some(Command::Add { timeout }) => cmd_add(&switcher, timeout),
        Some(Command::Remove { name }) => cmd_remove(&switcher, &name),
        Some(Command::Rename { name, label }) => cmd_rename(&switcher, &name, &label),
    }
}

fn listing_json(l: &AccountListing) -> serde_json::Value {
    serde_json::json!({
        "label": l.meta.label,
        "email": l.meta.email,
        "organization": l.meta.organization_name,
        "subscription": l.meta.subscription_type,
        "uuid": l.meta.uuid,
        "active": l.active,
        "last_used_at": l.meta.last_used_at,
    })
}

fn cmd_list(sw: &Switcher<&RealPaths, KeyringStore>, json: bool) -> Result<()> {
    let listing = manage::list(sw)?;

    if json {
        let payload: Vec<_> = listing.iter().map(listing_json).collect();
        output::data(&serde_json::to_string_pretty(&payload).unwrap_or_default());
        return Ok(());
    }

    if listing.is_empty() {
        output::info("No accounts stored yet. Run `byte capture` to save the current one.");
        return Ok(());
    }

    output::header("Accounts");
    for l in &listing {
        let mark = if l.active { "*" } else { " " };
        let org = l.meta.organization_name.as_deref().unwrap_or("-");
        output::info(&format!("{mark} {}  ({org})", l.meta.label));
    }
    Ok(())
}

fn cmd_current(sw: &Switcher<&RealPaths, KeyringStore>, json: bool) -> Result<()> {
    match manage::current(sw)? {
        Some(meta) if json => {
            output::data(&serde_json::json!({"label": meta.label, "uuid": meta.uuid}).to_string());
        }
        Some(meta) => output::data(&meta.label),
        None if json => output::data("null"),
        None => output::info("No active account."),
    }
    Ok(())
}

fn report_sync(sync: &SyncOutcome) {
    if let SyncOutcome::Captured(meta) = sync {
        output::status(&format!("Saved previously unknown account {}", meta.label));
    }
}

fn cmd_switch(sw: &Switcher<&RealPaths, KeyringStore>, name: &str, json: bool) -> Result<()> {
    let SwitchOutcome {
        switched_to,
        sync,
        already_active,
    } = sw.switch_to(name)?;

    if json {
        output::data(
            &serde_json::json!({
                "switched_to": switched_to.label,
                "uuid": switched_to.uuid,
                "already_active": already_active,
            })
            .to_string(),
        );
        return Ok(());
    }

    report_sync(&sync);
    if already_active {
        output::info(&format!("{} is already active.", switched_to.label));
    } else {
        output::status(&format!("Switched to {}", switched_to.label));
        output::warn("Claude Code sessions already running keep the previous account until restarted.");
    }
    Ok(())
}

fn cmd_capture(sw: &Switcher<&RealPaths, KeyringStore>, json: bool) -> Result<()> {
    let meta = sw.capture_current()?;
    if json {
        output::data(&serde_json::json!({"captured": meta.label}).to_string());
    } else {
        output::status(&format!("Saved {}", meta.label));
    }
    Ok(())
}

fn cmd_add(sw: &Switcher<&RealPaths, KeyringStore>, timeout: u64) -> Result<()> {
    let session = AddSession::begin(sw)?;

    output::status("Claude Code is now logged out.");
    output::info("Run `claude` in another terminal and log in as the account you want to add.");
    output::info(&format!("Waiting up to {timeout} seconds..."));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    while std::time::Instant::now() < deadline {
        if let Some(meta) = session.poll_once(sw)? {
            output::status(&format!("Added {}", meta.label));
            return Ok(());
        }
        std::thread::sleep(POLL_INTERVAL);
    }

    output::warn("Timed out. Restoring the previous account.");
    session.abort(sw)?;
    Err(Error::LoginTimeout(timeout))
}

fn cmd_remove(sw: &Switcher<&RealPaths, KeyringStore>, name: &str) -> Result<()> {
    let meta = manage::remove(sw, name)?;
    output::status(&format!("Removed {}", meta.label));
    Ok(())
}

fn cmd_rename(sw: &Switcher<&RealPaths, KeyringStore>, name: &str, label: &str) -> Result<()> {
    let meta = manage::rename(sw, name, label)?;
    output::status(&format!("Renamed to {}", meta.label));
    Ok(())
}
```

Add `pub mod cli;` to `src/lib.rs`.

- [ ] **Step 6: Wire up `main`**

Replace `src/main.rs`:

```rust
use clap::Parser as _;

use byte::cli::{run, Cli};

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    match run::run(cli) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            byte::output::error(&e.to_string());
            std::process::ExitCode::FAILURE
        }
    }
}
```

Note: running `byte` with no arguments currently lists accounts. Plan 2 changes that default to launching the tray.

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --test cli_test`
Expected: 8 passed.

Run: `make test && make lint && make fmt-check`
Expected: all green.

- [ ] **Step 8: Write the man page**

Create `man/byte.md`:

```markdown
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

**remove** *NAME*
: Forget a stored account, deleting both its metadata and its stored
  credentials.

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

0 on success, 1 on any error.

# NOTES

Claude Code reads its credentials at startup, so sessions that are already
running keep the previous account until they are restarted.
```

- [ ] **Step 9: Update the documentation**

Per the sync points in `AGENTS.md` and spec §13, fill in these files, replacing the scaffold placeholders:

- `README.md` — replace the `_Add 3–5 concrete value propositions here._` block with real ones; fill Prerequisites (Rust 1.88, Claude Code installed), Install (`cargo install --path .`), Quick start (`byte capture`, `byte add`, `byte switch <name>`), Usage (the command table from Task 11 Step 4), Configuration (the paths from `docs/configuration.md`), and Troubleshooting.
- `docs/getting-started.md` — the first-run walkthrough: capture the current account, add a second with `byte add`, switch between them.
- `docs/configuration.md` — `accounts.json` location per platform, `BYTE_CONFIG_DIR`, `CLAUDE_CONFIG_DIR`, and where credentials are stored in each OS keychain.
- `docs/architecture.md` — the module layout and dependency direction from the plan's File Structure table.
- `docs/troubleshooting.md` — the error table from spec §9, plus how to recover from `backups/`.
- `SECURITY.md` — add the threat-model note from spec §12 risk 5: byte stores live refresh tokens in the OS keychain; anyone able to run code as the user can read them, which is also true of Claude Code's own credential file.

- [ ] **Step 10: Verify the whole suite, then the keychain path by hand**

```bash
make test && make lint && make fmt-check && cargo build --release
```

Expected: all green.

The automated tests deliberately never touch a real keychain, so `KeyringStore`
and the `cli/run.rs` wiring are still unproven at this point. Verify them once,
by hand, against your real accounts. Do this only when `make test` is green —
these commands modify your actual Claude Code login.

Record the result of each step in the research findings doc.

1. Save the account you are currently logged in as:

```bash
byte capture
```

2. Confirm it is listed and marked active:

```bash
byte list
```

3. Confirm the credential landed in the OS credential store, not in a file.
   On Windows, look for a generic credential named
   `byte-claude-account-switcher` in Credential Manager. On macOS, search the
   login keychain for the same service name.

4. Add your second account. This logs Claude Code out, so have the login
   details ready:

```bash
byte add
```

5. Switch back and forth, confirming `claude` reports the expected account each
   time:

```bash
byte switch <first-account-label>
```

6. Confirm nothing else was disturbed — this is the check that matters most:

```bash
git -C "$HOME" diff --no-index /dev/null /dev/null 2>/dev/null; python -c "
import json,os
d=json.load(open(os.path.expanduser('~/.claude.json')))
print('top-level keys:', len(d))
print('projects entries:', len(d.get('projects', {})))
print('parses cleanly: yes')
"
```

Expected: the key count and project count match what they were before the
switches. If either dropped, stop and restore from byte's `backups/`
directory.

- [ ] **Step 11: Commit**

```bash
git add src/ man/ docs/ README.md SECURITY.md tests/cli_test.rs
git commit -m "feat: add CLI surface, man page, and documentation"
```

---

## Self-Review

**Spec coverage.** Every v1 section maps to a task: §2 background → Tasks 4–5; §3 surfaces → Task 11; §4 swap scope → Task 5; §5 preservation requirements 1–6 → Tasks 3–4; §6.1 module layout → File Structure; §6.2 trait seams → Tasks 2, 7 (`ProcessProbe` is Plan 2); §6.3 storage → Tasks 6–7; §7 switch algorithm incl. sync-back → Task 8; §7.2 auto-capture → Task 8; §8 add flow → Task 9; §9 error table → Task 1 (`Error`) and Task 11 (`docs/troubleshooting.md`); §10 testing → every task; §11 phases 0–4 → Tasks 1–11; §12 risks 2, 5 → Tasks 5 and 11; §13 documentation → Task 11.

**Deliberately deferred to Plan 2:** spec §6.1 `tray/*` and `claude/detect.rs`, §9's running-session *detection* (Task 11 prints the warning unconditionally rather than probing), §11 phases 5–7, and §14 Phase 8 in its entirety.

**Coverage boundary, stated rather than implied:** `tests/cli_test.rs` does not
exercise capture, switch, or add through the real binary, because the binary
uses `KeyringStore` and a test suite must not write to a developer's real
credential store or fail on a CI runner that has none. Those flows are covered
against `MemoryStore` in Tasks 8–10. The untested surface is therefore the
`KeyringStore` implementation itself and the wiring in `cli/run.rs`; both are
verified manually in Task 11 Step 10.

**Known follow-ups recorded rather than hidden:**
- `byte` with no arguments lists accounts in this plan; Plan 2 changes that default to the tray.
- `switch_to` re-resolves the target after applying it, so a rename racing a switch resolves to the newer label. Acceptable for a single-user tool.
- `cmd_switch` prints the running-session warning unconditionally, because `ProcessProbe` does not exist until Plan 2.

---

## Execution Handoff

Plan 1 covers spec Phases 0–4 and ends with a working headless switcher. Plan 2 (tray, process detection, packaging) is written after this lands, informed by Task 1's research findings.
