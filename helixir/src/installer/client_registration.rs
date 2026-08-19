//! Verified, rollback-safe MCP client registration used by the native executor.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate as helixir;

/// Secret-safe description of an MCP entry that needs explicit replacement.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RegistrationConflict {
    /// Client whose stable `helixir-local` entry would change.
    pub client: helixir::installer::ClientKind,
    /// Existing command, arguments and environment key names without values.
    pub existing: serde_json::Value,
    /// Requested command, arguments and environment key names without values.
    pub requested: serde_json::Value,
}

/// Inspect selected clients without mutation so a frontend can request consent.
pub fn registration_conflicts(
    clients: &BTreeSet<helixir::installer::ClientKind>,
) -> std::result::Result<Vec<RegistrationConflict>, String> {
    let mut conflicts = Vec::new();
    for client in clients {
        let server = desired_server(*client);
        let existing = if *client == helixir::installer::ClientKind::Cursor {
            let home = PathBuf::from(
                std::env::var("HOME").map_err(|_| "HOME is required for MCP config".to_string())?,
            );
            existing_json_registration(&home.join(".cursor/mcp.json"), "helixir-local")?
        } else {
            existing_native_registration(*client, "helixir-local")?
        };
        if let Some(existing) = existing
            && !registrations_match(&existing, &server.json_entry())
        {
            conflicts.push(RegistrationConflict {
                client: *client,
                existing: safe_registration_summary(&existing),
                requested: safe_registration_summary(&server.json_entry()),
            });
        }
    }
    Ok(conflicts)
}

fn current_sibling(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(name)))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn native_client_executable(client: helixir::installer::ClientKind) -> Option<PathBuf> {
    let command = match client {
        helixir::installer::ClientKind::ClaudeCode => "claude",
        helixir::installer::ClientKind::Codex => "codex",
        helixir::installer::ClientKind::Cursor => return None,
    };
    helixir::installer::clients::resolve_command(command).or_else(|| {
        (client == helixir::installer::ClientKind::Codex)
            .then(|| {
                [
                    "/Applications/ChatGPT.app/Contents/Resources/codex",
                    "/Applications/Codex.app/Contents/Resources/codex",
                ]
                .into_iter()
                .map(PathBuf::from)
                .find(|path| path.is_file())
            })
            .flatten()
    })
}

pub(crate) fn register_onboard_client(
    client: helixir::installer::ClientKind,
    replace_conflicting: bool,
) -> std::result::Result<(), String> {
    let server = desired_server(client);
    if client == helixir::installer::ClientKind::Cursor {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
        let path = helixir::installer::clients::default_json_config_path(client, &home)
            .ok_or_else(|| "Cursor home not found".to_string())?;
        let existing = existing_json_registration(&path, "helixir-local")?;
        approve_registration_change(client, existing.as_ref(), &server, replace_conflicting)?;
        let registration =
            helixir::installer::clients::register_json_client(&path, "helixir-local", &server)
                .map_err(|error| error.to_string())?;
        if existing_json_registration(&path, "helixir-local")?
            .as_ref()
            .is_some_and(|entry| registrations_match(entry, &server.json_entry()))
        {
            return Ok(());
        }
        if registration.changed {
            if let Some(backup) = registration.backup {
                let _ = std::fs::copy(backup, &path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
        return Err("Cursor registration could not be verified; restored backup".to_string());
    }

    let executable = native_client_executable(client)
        .ok_or_else(|| "native client executable not found".to_string())?;
    let existing = existing_native_registration(client, "helixir-local")?;
    if existing
        .as_ref()
        .is_some_and(|entry| registrations_match(entry, &server.json_entry()))
    {
        return Ok(());
    }
    approve_registration_change(client, existing.as_ref(), &server, replace_conflicting)?;
    let backup = backup_native_client_config(client)?;
    let update = (|| {
        if existing.is_some() {
            let removed = Command::new(&executable)
                .args(["mcp", "remove", "helixir-local"])
                .status()
                .map_err(|error| error.to_string())?;
            if !removed.success() {
                return Err(format!("{} registration removal failed", client.label()));
            }
        }
        let command =
            helixir::installer::clients::native_add_command(client, "helixir-local", &server);
        let status = Command::new(&executable)
            .args(command.args)
            .status()
            .map_err(|error| error.to_string())?;
        if !status.success() {
            return Err(format!(
                "{} registration exited with {status}",
                client.label()
            ));
        }
        if !existing_native_registration(client, "helixir-local")?
            .as_ref()
            .is_some_and(|entry| registrations_match(entry, &server.json_entry()))
        {
            return Err(format!(
                "{} registration did not match the requested entry",
                client.label()
            ));
        }
        Ok(())
    })();
    if update.is_err() {
        if let Some((original, backup)) = backup {
            let _ = std::fs::copy(backup, original);
        } else if existing.is_none() {
            let _ = Command::new(&executable)
                .args(["mcp", "remove", "helixir-local"])
                .status();
        }
    }
    update
}

pub fn client_registration_matches(
    client: helixir::installer::ClientKind,
    server_name: &str,
    server: &helixir::installer::clients::StdioServer,
) -> bool {
    let existing = if client == helixir::installer::ClientKind::Cursor {
        std::env::var("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(".cursor/mcp.json"))
            .map_err(|error| error.to_string())
            .and_then(|path| existing_json_registration(&path, server_name))
    } else {
        existing_native_registration(client, server_name)
    };
    existing.is_ok_and(|entry| {
        entry
            .as_ref()
            .is_some_and(|entry| registrations_match(entry, &server.json_entry()))
    })
}

fn desired_server(
    client: helixir::installer::ClientKind,
) -> helixir::installer::clients::StdioServer {
    helixir::installer::clients::StdioServer::new(
        current_sibling("helixir-mcp").display().to_string(),
    )
    .with_env("HELIXIR_RBAC_ACTOR", client.principal_id())
}

fn registrations_match(existing: &serde_json::Value, expected: &serde_json::Value) -> bool {
    if existing == expected {
        return true;
    }
    let (Some(existing_command), Some(expected_command)) = (
        existing.get("command").and_then(serde_json::Value::as_str),
        expected.get("command").and_then(serde_json::Value::as_str),
    ) else {
        return false;
    };
    let (Ok(existing_command), Ok(expected_command)) = (
        std::fs::canonicalize(existing_command),
        std::fs::canonicalize(expected_command),
    ) else {
        return false;
    };
    if existing_command != expected_command {
        return false;
    }

    let mut existing = existing.clone();
    let mut expected = expected.clone();
    existing["command"] = serde_json::Value::Null;
    expected["command"] = serde_json::Value::Null;
    existing == expected
}

fn existing_native_registration(
    client: helixir::installer::ClientKind,
    server_name: &str,
) -> std::result::Result<Option<serde_json::Value>, String> {
    let home = PathBuf::from(
        std::env::var("HOME").map_err(|_| "HOME is required for MCP config".to_string())?,
    );
    match client {
        helixir::installer::ClientKind::ClaudeCode => {
            existing_json_registration(&home.join(".claude.json"), server_name)
        }
        helixir::installer::ClientKind::Codex => {
            let path = home.join(".codex/config.toml");
            if !path.exists() {
                return Ok(None);
            }
            let raw = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
            let document = toml::from_str::<toml::Value>(&raw)
                .map_err(|error| format!("refusing malformed {}: {error}", path.display()))?;
            let Some(entry) = document
                .get("mcp_servers")
                .and_then(|servers| servers.get(server_name))
            else {
                return Ok(None);
            };
            serde_json::to_value(entry)
                .map(|entry| Some(normalized_registration(&entry)))
                .map_err(|error| error.to_string())
        }
        helixir::installer::ClientKind::Cursor => unreachable!("Cursor uses JSON inspection"),
    }
}

fn existing_json_registration(
    path: &Path,
    server_name: &str,
) -> std::result::Result<Option<serde_json::Value>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let document = serde_json::from_str::<serde_json::Value>(&raw)
        .map_err(|error| format!("refusing malformed {}: {error}", path.display()))?;
    let root = document
        .as_object()
        .ok_or_else(|| format!("refusing non-object config {}", path.display()))?;
    Ok(root
        .get("mcpServers")
        .and_then(serde_json::Value::as_object)
        .and_then(|servers| servers.get(server_name))
        .map(normalized_registration))
}

fn normalized_registration(entry: &serde_json::Value) -> serde_json::Value {
    let mut normalized = serde_json::Map::new();
    if let Some(command) = entry.get("command") {
        normalized.insert("command".to_string(), command.clone());
    }
    if let Some(args) = entry
        .get("args")
        .and_then(serde_json::Value::as_array)
        .filter(|args| !args.is_empty())
    {
        normalized.insert("args".to_string(), serde_json::Value::Array(args.clone()));
    }
    if let Some(env) = entry
        .get("env")
        .and_then(serde_json::Value::as_object)
        .filter(|env| !env.is_empty())
    {
        normalized.insert("env".to_string(), serde_json::Value::Object(env.clone()));
    }
    serde_json::Value::Object(normalized)
}

fn approve_registration_change(
    client: helixir::installer::ClientKind,
    existing: Option<&serde_json::Value>,
    server: &helixir::installer::clients::StdioServer,
    replace_conflicting: bool,
) -> std::result::Result<(), String> {
    let Some(existing) = existing else {
        return Ok(());
    };
    if registrations_match(existing, &server.json_entry()) {
        return Ok(());
    }
    if replace_conflicting {
        return Ok(());
    }
    Err(format!(
        "{} has a conflicting helixir-local entry; explicit replacement approval is required",
        client.label()
    ))
}

fn safe_registration_summary(entry: &serde_json::Value) -> serde_json::Value {
    let mut summary = serde_json::Map::new();
    for key in ["command", "args"] {
        if let Some(value) = entry.get(key) {
            summary.insert(key.to_string(), value.clone());
        }
    }
    if let Some(env) = entry.get("env").and_then(serde_json::Value::as_object) {
        summary.insert(
            "env_keys".to_string(),
            serde_json::Value::Array(env.keys().cloned().map(serde_json::Value::String).collect()),
        );
    }
    serde_json::Value::Object(summary)
}

fn backup_native_client_config(
    client: helixir::installer::ClientKind,
) -> std::result::Result<Option<(PathBuf, PathBuf)>, String> {
    let home = PathBuf::from(
        std::env::var("HOME")
            .map_err(|_| "HOME is required to back up MCP client config".to_string())?,
    );
    let original = match client {
        helixir::installer::ClientKind::ClaudeCode => home.join(".claude.json"),
        helixir::installer::ClientKind::Codex => home.join(".codex/config.toml"),
        helixir::installer::ClientKind::Cursor => unreachable!("Cursor is JSON-configured"),
    };
    if !original.exists() {
        return Ok(None);
    }
    if !original.is_file() {
        return Err(format!("{} is not a regular file", original.display()));
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S%.3f");
    let backup = PathBuf::from(format!("{}.bak.{stamp}", original.display()));
    std::fs::copy(&original, &backup).map_err(|error| error.to_string())?;
    Ok(Some((original, backup)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_registration_comparison_ignores_client_owned_fields() {
        let normalized = normalized_registration(&serde_json::json!({
            "command": "/stable/helixir-mcp",
            "args": [],
            "env": {"HELIXIR_RBAC_ACTOR": "codex"},
            "enabled": true,
            "startup_timeout_sec": 15
        }));
        assert_eq!(
            normalized,
            helixir::installer::clients::StdioServer::new("/stable/helixir-mcp")
                .with_env("HELIXIR_RBAC_ACTOR", "codex")
                .json_entry()
        );
    }

    #[test]
    fn conflict_summary_never_prints_environment_values() {
        let summary = safe_registration_summary(&serde_json::json!({
            "command": "helixir-mcp",
            "env": {"HELIX_LLM_API_KEY": "must-not-leak"}
        }));
        assert!(!summary.to_string().contains("must-not-leak"));
        assert_eq!(
            summary["env_keys"],
            serde_json::json!(["HELIX_LLM_API_KEY"])
        );
    }

    #[test]
    fn non_interactive_conflict_fails_before_mutation() {
        let error = approve_registration_change(
            helixir::installer::ClientKind::Codex,
            Some(&serde_json::json!({"command": "old"})),
            &helixir::installer::clients::StdioServer::new("new"),
            false,
        )
        .expect_err("conflict must require approval");
        assert!(error.contains("explicit replacement approval"));
    }

    #[test]
    fn reviewed_non_interactive_conflict_is_approved() {
        approve_registration_change(
            helixir::installer::ClientKind::Codex,
            Some(&serde_json::json!({"command": "old"})),
            &helixir::installer::clients::StdioServer::new("new"),
            true,
        )
        .expect("typed control-plane consent should approve replacement");
    }

    #[cfg(unix)]
    #[test]
    fn registration_comparison_accepts_equivalent_symlink_commands() {
        use std::os::unix::fs::symlink;

        let temp = std::env::temp_dir().join(format!(
            "helixir-registration-match-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&temp).expect("temporary directory");
        let binary = temp.join("helixir-mcp-v3");
        let stable = temp.join("helixir-mcp");
        std::fs::write(&binary, b"test binary").expect("binary fixture");
        symlink(&binary, &stable).expect("stable symlink");
        let existing = serde_json::json!({
            "command": stable,
            "env": {"HELIXIR_RBAC_ACTOR": "codex"}
        });
        let expected = serde_json::json!({
            "command": binary,
            "env": {"HELIXIR_RBAC_ACTOR": "codex"}
        });
        assert!(registrations_match(&existing, &expected));
        std::fs::remove_dir_all(&temp).expect("remove temporary directory");
    }
}
