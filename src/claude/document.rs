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
