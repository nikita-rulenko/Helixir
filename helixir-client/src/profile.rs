//! Non-secret client profile persisted under `~/.helixir/client.json`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::registration::ClientKind;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientProfile {
    pub gateway_url: String,
    pub principal_id: String,
    pub owner_id: String,
    pub clients: Vec<ClientKind>,
    pub project_root: PathBuf,
    pub token_env: String,
    pub installed_at: String,
}

impl ClientProfile {
    pub fn default_path() -> Result<PathBuf> {
        Ok(home_dir()?.join(".helixir/client.json"))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let bytes =
            fs::read(path).with_context(|| format!("read client profile {}", path.display()))?;
        serde_json::from_slice(&bytes)
            .with_context(|| format!("parse client profile {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("profile path has no parent"))?;
        fs::create_dir_all(parent)
            .with_context(|| format!("create profile directory {}", parent.display()))?;
        let temporary = parent.join(format!(".client.json.tmp.{}", std::process::id()));
        let bytes = serde_json::to_vec_pretty(self)?;
        fs::write(&temporary, bytes)
            .with_context(|| format!("write temporary profile {}", temporary.display()))?;
        set_private_permissions(&temporary)?;
        fs::rename(&temporary, path)
            .with_context(|| format!("replace client profile {}", path.display()))?;
        Ok(())
    }
}

pub fn home_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home {
        Some(path) if path.is_absolute() => Ok(path),
        _ => bail!("HOME must be an absolute path"),
    }
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_round_trip_contains_no_token_value() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("client.json");
        let profile = ClientProfile {
            gateway_url: "https://memory.example/mcp".to_string(),
            principal_id: "codex-laptop".to_string(),
            owner_id: "codex".to_string(),
            clients: vec![ClientKind::Codex],
            project_root: root.path().to_path_buf(),
            token_env: "HELIXIR_GATEWAY_TOKEN".to_string(),
            installed_at: "2026-08-21T00:00:00Z".to_string(),
        };
        profile.save(&path).unwrap();
        assert_eq!(ClientProfile::load(&path).unwrap(), profile);
        assert!(!fs::read_to_string(path).unwrap().contains("secret"));
    }
}
