//! Crash-safe file replacement.
//!
//! Writes go to a temporary file in the *same directory* as the target, are
//! flushed and fsynced, and are then atomically renamed over the target. Same
//! directory matters: a cross-volume rename is a copy, which is not atomic.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::output;

/// How many timestamped backups of a given file `prune` keeps. Applied
/// uniformly to `.claude.json` and `.credentials.json` (via
/// `JsonDocument::save`) and to `accounts.json` (via `AccountsFile::save`).
pub const BACKUP_RETENTION: usize = 10;

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
///
/// Infallible by contract: this is pure housekeeping on files that have
/// already served their purpose (the write they were protecting has already
/// been committed and verified by the time this runs), so a failure here
/// must never invalidate that write. Problems are reported via
/// `output::warn` instead of `Result`, and a failure removing one backup
/// does not stop the rest from being pruned.
pub fn prune(backup_dir: &Path, stem: &str, keep: usize) {
    if !backup_dir.is_dir() {
        return;
    }
    let prefix = format!("{stem}.");
    let entries = match std::fs::read_dir(backup_dir) {
        Ok(entries) => entries,
        Err(e) => {
            output::warn(&format!(
                "could not list {} to prune old backups: {e}",
                backup_dir.display()
            ));
            return;
        }
    };

    let mut found: Vec<PathBuf> = entries
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
        if let Err(e) = std::fs::remove_file(&path) {
            output::warn(&format!(
                "could not remove old backup {}: {e}",
                path.display()
            ));
        }
    }
}
