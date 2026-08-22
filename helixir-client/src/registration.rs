//! Backup-verified registration of the remote gateway in supported agent clients.

use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::profile::home_dir;

const SERVER_NAME: &str = "helixir-local";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ClientKind {
    Claude,
    Codex,
    Cursor,
}

impl ClientKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Cursor => "Cursor",
        }
    }

    pub fn executable(self) -> Option<PathBuf> {
        match self {
            Self::Claude => resolve_command("claude"),
            Self::Codex => resolve_command("codex").or_else(|| {
                [
                    "/Applications/ChatGPT.app/Contents/Resources/codex",
                    "/Applications/Codex.app/Contents/Resources/codex",
                ]
                .into_iter()
                .map(PathBuf::from)
                .find(|path| path.is_file())
            }),
            Self::Cursor => None,
        }
    }
}

pub fn detect_clients() -> Vec<ClientKind> {
    let home = home_dir().ok();
    [ClientKind::Claude, ClientKind::Codex, ClientKind::Cursor]
        .into_iter()
        .filter(|client| match client {
            ClientKind::Cursor => home
                .as_ref()
                .is_some_and(|home| home.join(".cursor").exists()),
            _ => client.executable().is_some(),
        })
        .collect()
}

pub fn register(
    client: ClientKind,
    gateway_url: &str,
    token_env: Option<&str>,
    replace: bool,
) -> Result<()> {
    let expected = expected_entry(client, gateway_url, token_env);
    let existing = existing_registration(client)?;
    if existing
        .as_ref()
        .is_some_and(|entry| registrations_match(entry, &expected))
    {
        return Ok(());
    }
    if existing.is_some() && !replace {
        bail!(
            "{} has a conflicting {SERVER_NAME} entry; rerun with --replace",
            client.label()
        );
    }
    match client {
        ClientKind::Cursor => register_cursor(gateway_url, token_env),
        ClientKind::Claude | ClientKind::Codex => {
            register_native(client, gateway_url, token_env, existing.is_some())
        }
    }?;
    if !registration_matches(client, gateway_url, token_env)? {
        bail!("{} registration could not be verified", client.label());
    }
    Ok(())
}

pub fn registration_matches(
    client: ClientKind,
    gateway_url: &str,
    token_env: Option<&str>,
) -> Result<bool> {
    Ok(existing_registration(client)?
        .as_ref()
        .is_some_and(|entry| {
            registrations_match(entry, &expected_entry(client, gateway_url, token_env))
        }))
}

fn register_native(
    client: ClientKind,
    gateway_url: &str,
    token_env: Option<&str>,
    remove_existing: bool,
) -> Result<()> {
    let executable = client
        .executable()
        .ok_or_else(|| anyhow::anyhow!("{} executable not found", client.label()))?;
    let backup = backup_native_config(client)?;
    let update = (|| -> Result<()> {
        if remove_existing {
            let status = Command::new(&executable)
                .args(["mcp", "remove", SERVER_NAME])
                .status()?;
            if !status.success() {
                bail!("{} MCP removal exited with {status}", client.label());
            }
        }
        let mut args = match client {
            ClientKind::Claude => vec![
                "mcp",
                "add",
                "--transport",
                "http",
                "--scope",
                "user",
                SERVER_NAME,
                gateway_url,
            ],
            ClientKind::Codex => vec!["mcp", "add", SERVER_NAME, "--url", gateway_url],
            ClientKind::Cursor => unreachable!(),
        };
        let header;
        if let Some(token_env) = token_env {
            match client {
                ClientKind::Claude => {
                    header = format!("Authorization: Bearer ${{{token_env}}}");
                    args.extend(["--header", &header]);
                }
                ClientKind::Codex => args.extend(["--bearer-token-env-var", token_env]),
                ClientKind::Cursor => unreachable!(),
            }
        }
        let status = Command::new(&executable).args(args).status()?;
        if !status.success() {
            bail!("{} MCP registration exited with {status}", client.label());
        }
        if !registration_matches(client, gateway_url, token_env)? {
            bail!(
                "{} registration did not match the requested gateway",
                client.label()
            );
        }
        Ok(())
    })();
    if update.is_err() {
        restore_native_config(client, backup.as_ref())?;
    }
    update
}

fn register_cursor(gateway_url: &str, token_env: Option<&str>) -> Result<()> {
    let path = home_dir()?.join(".cursor/mcp.json");
    let original = path.exists().then(|| fs::read(&path)).transpose()?;
    let mut document = if let Some(original) = &original {
        serde_json::from_slice::<Value>(original)
            .with_context(|| format!("refusing malformed {}", path.display()))?
    } else {
        json!({})
    };
    let root = document
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("{} must contain a JSON object", path.display()))?;
    let servers = root
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("{}.mcpServers must be an object", path.display()))?;
    servers.insert(
        SERVER_NAME.to_string(),
        expected_entry(ClientKind::Cursor, gateway_url, token_env),
    );
    atomic_write_json(&path, &document)?;
    if !registration_matches(ClientKind::Cursor, gateway_url, token_env)? {
        if let Some(original) = original {
            fs::write(&path, original)?;
        } else {
            fs::remove_file(&path)?;
        }
        bail!("Cursor registration could not be verified; restored previous config");
    }
    Ok(())
}

fn existing_registration(client: ClientKind) -> Result<Option<Value>> {
    let home = home_dir()?;
    match client {
        ClientKind::Claude => existing_json(&home.join(".claude.json")),
        ClientKind::Cursor => existing_json(&home.join(".cursor/mcp.json")),
        ClientKind::Codex => {
            let path = home.join(".codex/config.toml");
            if !path.exists() {
                return Ok(None);
            }
            let raw = fs::read_to_string(&path)?;
            let document = toml::from_str::<toml::Value>(&raw)
                .with_context(|| format!("refusing malformed {}", path.display()))?;
            document
                .get("mcp_servers")
                .and_then(|servers| servers.get(SERVER_NAME))
                .map(serde_json::to_value)
                .transpose()
                .map(|entry| entry.map(|value| normalized(&value)))
                .map_err(Into::into)
        }
    }
}

fn existing_json(path: &Path) -> Result<Option<Value>> {
    if !path.exists() {
        return Ok(None);
    }
    let document: Value = serde_json::from_slice(&fs::read(path)?)
        .with_context(|| format!("refusing malformed {}", path.display()))?;
    let root = document
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("{} must contain a JSON object", path.display()))?;
    Ok(root
        .get("mcpServers")
        .and_then(Value::as_object)
        .and_then(|servers| servers.get(SERVER_NAME))
        .map(normalized))
}

fn expected_entry(client: ClientKind, gateway_url: &str, token_env: Option<&str>) -> Value {
    let mut entry = json!({"type":"http","url":gateway_url});
    if let Some(token_env) = token_env {
        match client {
            ClientKind::Codex => {
                entry["bearer_token_env_var"] = json!(token_env);
            }
            ClientKind::Claude => {
                entry["headers"] = json!({"Authorization": format!("Bearer ${{{token_env}}}")});
            }
            ClientKind::Cursor => {
                entry["headers"] = json!({"Authorization": format!("Bearer ${{env:{token_env}}}")});
            }
        }
    }
    entry
}

fn normalized(entry: &Value) -> Value {
    let Some(url) = entry.get("url") else {
        return entry.clone();
    };
    let mut normalized = json!({"type":"http","url":url});
    for field in ["headers", "bearer_token_env_var"] {
        if let Some(value) = entry.get(field) {
            normalized[field] = value.clone();
        }
    }
    normalized
}

fn registrations_match(existing: &Value, expected: &Value) -> bool {
    normalized(existing) == normalized(expected)
}

fn native_config_path(client: ClientKind) -> Result<PathBuf> {
    let home = home_dir()?;
    Ok(match client {
        ClientKind::Claude => home.join(".claude.json"),
        ClientKind::Codex => home.join(".codex/config.toml"),
        ClientKind::Cursor => bail!("Cursor does not use a native config"),
    })
}

fn backup_native_config(client: ClientKind) -> Result<Option<PathBuf>> {
    let path = native_config_path(client)?;
    if !path.exists() {
        return Ok(None);
    }
    if !path.is_file() {
        bail!("{} is not a regular file", path.display());
    }
    let backup = PathBuf::from(format!(
        "{}.bak.{}",
        path.display(),
        chrono::Utc::now().format("%Y%m%d-%H%M%S%.3f")
    ));
    fs::copy(path, &backup)?;
    Ok(Some(backup))
}

fn restore_native_config(client: ClientKind, backup: Option<&PathBuf>) -> Result<()> {
    let path = native_config_path(client)?;
    if let Some(backup) = backup {
        fs::copy(backup, path)?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn atomic_write_json(path: &Path, document: &Value) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let existing_metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                bail!("refusing to replace symlink {}", path.display());
            }
            if !metadata.is_file() {
                bail!("{} is not a regular file", path.display());
            }
            Some(metadata)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if existing_metadata.is_some() {
        let backup = PathBuf::from(format!(
            "{}.bak.{}",
            path.display(),
            chrono::Utc::now().format("%Y%m%d-%H%M%S%.3f")
        ));
        fs::copy(path, backup)?;
    }
    let temporary = PathBuf::from(format!(
        "{}.tmp.{}.{}",
        path.display(),
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let mode = existing_metadata
            .as_ref()
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .unwrap_or(0o600);
        options.mode(mode);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(&serde_json::to_vec_pretty(document)?)?;
    file.sync_all()?;
    #[cfg(not(unix))]
    if let Some(metadata) = &existing_metadata {
        fs::set_permissions(&temporary, metadata.permissions())?;
    }
    fs::rename(&temporary, path)?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn resolve_command(command: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|directory| directory.join(command))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_accepts_native_http_shape() {
        assert!(registrations_match(
            &json!({"url":"http://host:8765/mcp"}),
            &json!({"type":"http","url":"http://host:8765/mcp"})
        ));
    }

    #[test]
    fn cursor_write_preserves_other_servers() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("mcp.json");
        fs::write(&path, br#"{"mcpServers":{"other":{"command":"x"}}}"#).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        document["mcpServers"][SERVER_NAME] =
            expected_entry(ClientKind::Cursor, "http://host:8765/mcp", None);
        atomic_write_json(&path, &document).unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["mcpServers"]["other"]["command"], "x");
        assert_eq!(
            saved["mcpServers"][SERVER_NAME]["url"],
            "http://host:8765/mcp"
        );
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn cursor_new_config_is_private_and_symlinks_are_refused() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let root = tempfile::tempdir().unwrap();
        let created = root.path().join("new.json");
        atomic_write_json(&created, &json!({"mcpServers": {}})).unwrap();
        assert_eq!(
            fs::metadata(&created).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let target = root.path().join("target.json");
        fs::write(&target, b"{}").unwrap();
        let linked = root.path().join("linked.json");
        symlink(&target, &linked).unwrap();
        let error = atomic_write_json(&linked, &json!({})).unwrap_err();
        assert!(error.to_string().contains("refusing to replace symlink"));
        assert_eq!(fs::read(&target).unwrap(), b"{}");
    }

    #[test]
    fn auth_registration_stores_only_the_environment_reference() {
        let codex = expected_entry(
            ClientKind::Codex,
            "https://memory.example/mcp",
            Some("HELIXIR_GATEWAY_TOKEN"),
        );
        let claude = expected_entry(
            ClientKind::Claude,
            "https://memory.example/mcp",
            Some("HELIXIR_GATEWAY_TOKEN"),
        );
        let cursor = expected_entry(
            ClientKind::Cursor,
            "https://memory.example/mcp",
            Some("HELIXIR_GATEWAY_TOKEN"),
        );
        assert_eq!(codex["bearer_token_env_var"], "HELIXIR_GATEWAY_TOKEN");
        assert_eq!(
            claude["headers"]["Authorization"],
            "Bearer ${HELIXIR_GATEWAY_TOKEN}"
        );
        assert_eq!(
            cursor["headers"]["Authorization"],
            "Bearer ${env:HELIXIR_GATEWAY_TOKEN}"
        );
        for entry in [codex, claude, cursor] {
            assert!(!entry.to_string().contains("actual-secret"));
        }
    }
}
