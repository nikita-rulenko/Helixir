//! Central installer configuration with lossless, atomic TOML updates.
//!
//! Client MCP entries must contain only a stable command. Provider settings and
//! secrets belong in this file, protected by filesystem permissions.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;
use toml_edit::{DocumentMut, Item, Value as TomlValue};

/// Errors raised while reading or atomically updating the central config.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Existing bytes are not valid TOML.
    #[error("existing central config is not valid TOML: {0}")]
    InvalidToml(#[from] toml_edit::TomlError),
    /// Filesystem operation failed.
    #[error("central config filesystem operation failed: {0}")]
    Io(#[from] io::Error),
    /// A dotted patch path collided with a scalar TOML value.
    #[error("config path is not a table: {0}")]
    InvalidShape(String),
}

/// Lossless update request. Keys use the same dotted paths as `helixir config set`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigPatch {
    /// Scalar values to set; secrets are accepted but never rendered in logs.
    pub values: BTreeMap<String, String>,
}

impl ConfigPatch {
    /// Add one scalar TOML value and return the builder.
    #[must_use]
    pub fn set(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.values.insert(key.into(), value.into());
        self
    }
}

/// Outcome of an atomic central-config update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigWriteResult {
    /// Path written.
    pub path: PathBuf,
    /// Whether any value changed.
    pub changed: bool,
    /// Timestamped backup when an existing file was replaced.
    pub backup: Option<PathBuf>,
}

/// Apply a patch to TOML bytes without dropping unrelated keys or comments.
pub fn merge_patch(
    existing: Option<&str>,
    patch: &ConfigPatch,
) -> Result<(String, bool), ConfigError> {
    let mut document = match existing {
        Some(raw) => raw.parse::<DocumentMut>()?,
        None => DocumentMut::new(),
    };
    let mut changed = false;
    for (key, raw_value) in &patch.values {
        let segments: Vec<&str> = key.split('.').collect();
        if segments.is_empty() || segments.iter().any(|segment| segment.is_empty()) {
            continue;
        }
        let value = parse_scalar(raw_value);
        let leaf = segments[segments.len() - 1];
        let before = document.to_string();
        {
            let mut table = document.as_table_mut();
            for segment in &segments[..segments.len() - 1] {
                table = table
                    .entry(segment)
                    .or_insert(Item::Table(toml_edit::Table::new()))
                    .as_table_mut()
                    .ok_or_else(|| ConfigError::InvalidShape((*segment).to_string()))?;
            }
            table[leaf] = Item::Value(value);
        }
        changed |= document.to_string() != before;
    }
    Ok((document.to_string(), changed))
}

fn parse_scalar(raw: &str) -> TomlValue {
    raw.parse::<TomlValue>()
        .unwrap_or_else(|_| TomlValue::from(raw.to_string()))
}

/// Atomically merge a patch into a central config file.
pub fn write_patch(path: &Path, patch: &ConfigPatch) -> Result<ConfigWriteResult, ConfigError> {
    let existing = if path.exists() {
        Some(fs::read_to_string(path)?)
    } else {
        None
    };
    let (rendered, changed) = merge_patch(existing.as_deref(), patch)?;
    if !changed {
        return Ok(ConfigWriteResult {
            path: path.to_path_buf(),
            changed: false,
            backup: None,
        });
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S%.3f");
    let backup = if path.exists() {
        let backup = PathBuf::from(format!("{}.bak.{stamp}", path.display()));
        fs::copy(path, &backup)?;
        Some(backup)
    } else {
        None
    };
    let temporary = PathBuf::from(format!("{}.tmp.{}", path.display(), std::process::id()));
    fs::write(&temporary, rendered)?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(ConfigError::Io(error));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(ConfigWriteResult {
        path: path.to_path_buf(),
        changed: true,
        backup,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn path(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("helixir-config-{name}-{stamp}.toml"))
    }

    #[test]
    fn patch_preserves_unrelated_toml() {
        let patch = ConfigPatch::default()
            .set("mode", "Collective")
            .set("llm_provider", "ollama")
            .set("llm_model", "llama3.2:3b");
        let (rendered, changed) = merge_patch(Some("# keep me\nport = 6970\n"), &patch).unwrap();
        assert!(changed);
        assert!(rendered.contains("# keep me"));
        assert!(rendered.contains("mode = \"Collective\""));
        assert!(rendered.contains("port = 6970"));
    }

    #[test]
    fn malformed_toml_is_rejected_before_write() {
        let error = merge_patch(
            Some("[broken\n"),
            &ConfigPatch::default().set("mode", "Solo"),
        )
        .expect_err("invalid TOML must abort");
        assert!(matches!(error, ConfigError::InvalidToml(_)));
    }

    #[test]
    fn write_is_idempotent_and_protects_file() {
        let path = path("atomic");
        let patch = ConfigPatch::default().set("mode", "Collective");
        let first = write_patch(&path, &patch).unwrap();
        assert!(first.changed);
        assert!(first.backup.is_none());
        let second = write_patch(&path, &patch).unwrap();
        assert!(!second.changed);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let _ = fs::remove_file(path);
    }
}
