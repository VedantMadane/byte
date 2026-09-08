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

        let target = file.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            if event.paths.iter().any(|p| p == &target) {
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
