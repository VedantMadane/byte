//! Watching `accounts.json` so a CLI-driven change refreshes a running tray.
//!
//! The file is the shared state between the two processes -- no IPC. Because
//! every write goes through an atomic replace, a watcher never observes a torn
//! file; the worst case is a briefly stale menu, never a wrong one.

use std::ffi::OsStr;
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
    pub fn start(paths: &impl HostPaths, on_change: impl Fn() + Send + 'static) -> Result<Self> {
        let file = paths.accounts_file();
        let dir = file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| paths.byte_config_dir());

        std::fs::create_dir_all(&dir).map_err(|source| Error::Io {
            path: dir.clone(),
            source,
        })?;

        // Compare by file name, not the full path. `notify`'s FSEvents
        // backend on macOS canonicalizes the watched root and reports event
        // paths in canonical, symlink-resolved form: `/var`, `/tmp` and
        // `/etc` are symlinks to `/private/var`, `/private/tmp` and
        // `/private/etc`, and a config directory rooted under any of those
        // (as `TestPaths`, backed by `tempfile::tempdir()`, always is on
        // macOS -- it lands under `/var/folders/...`) reports events under
        // `/private/var/folders/...` instead. Neither `RealPaths` nor
        // `TestPaths` canonicalizes `accounts_file()`, so comparing full
        // paths is false for every real event on macOS. The watch is
        // already scoped to exactly one non-recursive directory, so a
        // file-name comparison is sufficient and sidesteps ancestor-
        // directory canonicalization entirely -- and it stays correct on
        // Windows and Linux, where event paths are reported as given.
        //
        // `accounts_file()` always resolves to `<dir>/accounts.json`
        // (see `HostPaths::accounts_file`'s default body), so `target_name`
        // is always `Some` in practice. If some future `HostPaths` impl
        // ever did return a nameless path (`/`, `C:\`), treat that as
        // "never matches" rather than "matches every nameless path" --
        // firing on unrelated directory-level events that also happen to
        // lack a file name would be worse than never firing at all.
        let target_name = file.file_name().map(OsStr::to_os_string);

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            let Some(name) = target_name.as_deref() else {
                return;
            };
            if event.paths.iter().any(|p| p.file_name() == Some(name)) {
                on_change();
            }
        })
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
