use super::*;

pub(crate) fn register_onboard_client(
    client: helixir::installer::ClientKind,
    interactive: bool,
) -> std::result::Result<(), String> {
    let server = helixir::installer::clients::StdioServer::new(
        current_sibling("helixir-mcp").display().to_string(),
    )
    .with_env("HELIXIR_RBAC_ACTOR", client.principal_id());
    if client == helixir::installer::ClientKind::Cursor {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
        let path = helixir::installer::clients::default_json_config_path(client, &home)
            .ok_or_else(|| "Cursor home not found".to_string())?;
        let existing = existing_json_registration(&path, "helixir-local")?;
        approve_registration_change(client, existing.as_ref(), &server, interactive)?;
        let registration =
            helixir::installer::clients::register_json_client(&path, "helixir-local", &server)
                .map_err(|error| error.to_string())?;
        if existing_json_registration(&path, "helixir-local")?.as_ref()
            == Some(&server.json_entry())
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
    if existing.as_ref() == Some(&server.json_entry()) {
        return Ok(());
    }
    approve_registration_change(client, existing.as_ref(), &server, interactive)?;
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
        if existing_native_registration(client, "helixir-local")?.as_ref()
            != Some(&server.json_entry())
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

pub(crate) fn client_registration_matches(
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
    existing.is_ok_and(|entry| entry.as_ref() == Some(&server.json_entry()))
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
    interactive: bool,
) -> std::result::Result<(), String> {
    let Some(existing) = existing else {
        return Ok(());
    };
    if existing == &server.json_entry() {
        return Ok(());
    }
    if !interactive {
        return Err(format!(
            "{} has a conflicting helixir-local entry; rerun interactively to approve replacement",
            client.label()
        ));
    }
    println!(
        "{} helixir-local change:\n  old: {}\n  new: {}",
        client.label(),
        safe_registration_summary(existing),
        safe_registration_summary(&server.json_entry())
    );
    let approved = Confirm::new()
        .with_prompt(format!(
            "Replace {} helixir-local registration?",
            client.label()
        ))
        .default(false)
        .interact()
        .map_err(|error| error.to_string())?;
    if approved {
        Ok(())
    } else {
        Err(format!(
            "{} registration replacement declined",
            client.label()
        ))
    }
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
        assert!(error.contains("rerun interactively"));
    }
}
