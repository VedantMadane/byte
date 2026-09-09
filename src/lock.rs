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

use fs4::{FileExt, TryLockError};

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

/// Try to take an exclusive advisory lock on `file` without blocking.
///
/// The brief this module was built from assumed `fs4::fs_std::FileExt`
/// exposing `try_lock_exclusive() -> Result<bool>`. Neither exists in the
/// `fs4` version that actually resolved (1.1.0): there is no `fs_std`
/// module -- `sync`-feature `std::fs::File` support hangs the trait
/// directly off the crate root -- and the method is `try_lock` (fs4 1.x
/// renamed it to mirror the `File::try_lock` stabilized in the standard
/// library in Rust 1.89), returning `Result<(), fs4::TryLockError>` rather
/// than `Result<bool>`.
///
/// `TryLockError` is a two-variant enum, which is actually a stronger
/// signal than the `Result<bool>` the brief anticipated: `WouldBlock` means
/// another process holds the lock -- refused, not an error -- and `Error`
/// carries the genuine `io::Error` (missing directory, permission denied,
/// ...) that must stay distinguishable from a busy lock. This function
/// collapses that back into the `Result<bool>` shape the rest of this
/// module (and the brief's guard logic) is written against.
///
/// Called as `FileExt::try_lock(file)` rather than `file.try_lock()`: `std`
/// has pre-announced a same-named `File::try_lock` (stabilized in Rust
/// 1.89, after this crate's pinned 1.88 toolchain), so rustc already lints
/// the dotted call as an `unstable_name_collisions` future hazard --
/// today it is unambiguous, but an inherent method always outranks a trait
/// method, so the dotted call would silently start resolving to `std`'s
/// version (a different `Result` shape) the moment the toolchain moves
/// past 1.89. The fully-qualified form pins it to `fs4`'s trait method
/// regardless of toolchain.
fn try_lock_exclusive(file: &File, path: &Path) -> Result<bool> {
    match FileExt::try_lock(file) {
        Ok(()) => Ok(true),
        Err(TryLockError::WouldBlock) => Ok(false),
        Err(TryLockError::Error(source)) => Err(Error::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
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
        if try_lock_exclusive(&file, &path)? {
            Ok(Some(Self { _file: file, path }))
        } else {
            Ok(None)
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
        if try_lock_exclusive(&file, &path)? {
            Ok(Some(Self { _file: file }))
        } else {
            Ok(None)
        }
    }
}
