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
    home.join("Library")
        .join("Application Support")
        .join("byte")
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
