//! Atomic installation manifest for versioned source/release installs.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Durable state describing the currently selected Helixir installation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallManifest {
    /// Installed Helixir version or source revision.
    pub version: String,
    /// Versioned directory containing binaries and schema.
    pub install_dir: PathBuf,
    /// Selected backend volume.
    pub backend_volume: String,
    /// Selected local models.
    pub models: Vec<String>,
    /// Clients registered by onboarding.
    pub clients: Vec<String>,
    /// Most recent backend snapshot, when one exists.
    pub last_backup: Option<PathBuf>,
}

/// Errors while reading or atomically writing the manifest.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// Filesystem error.
    #[error("manifest filesystem operation failed: {0}")]
    Io(#[from] io::Error),
    /// JSON encoding/decoding error.
    #[error("manifest JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Write a manifest with a sibling temporary file and restrictive permissions.
pub fn write(path: &Path, manifest: &InstallManifest) -> Result<(), ManifestError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(manifest)?;
    fs::write(&temporary, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(ManifestError::Io(error));
    }
    Ok(())
}

/// Read a manifest if it exists.
pub fn read(path: &Path) -> Result<Option<InstallManifest>, ManifestError> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&fs::read_to_string(path)?)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn manifest_round_trips_atomically() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("helixir-manifest-{stamp}.json"));
        let value = InstallManifest {
            version: "0.13.1".to_string(),
            install_dir: PathBuf::from("/tmp/versions/0.13.1"),
            backend_volume: "helixdb_data".to_string(),
            models: vec!["nomic-embed-text".to_string()],
            clients: vec!["codex".to_string()],
            last_backup: None,
        };
        write(&path, &value).unwrap();
        assert_eq!(read(&path).unwrap(), Some(value));
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
