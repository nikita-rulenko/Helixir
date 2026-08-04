use super::*;

pub(crate) fn register_onboard_client(
    client: helixir::installer::ClientKind,
) -> std::result::Result<(), String> {
    let server = helixir::installer::clients::StdioServer::new(
        current_sibling("helixir-mcp").display().to_string(),
    )
    .with_env("HELIXIR_RBAC_ACTOR", client.principal_id());
    if client == helixir::installer::ClientKind::Cursor {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
        let path = helixir::installer::clients::default_json_config_path(client, &home)
            .ok_or_else(|| "Cursor home not found".to_string())?;
        helixir::installer::clients::register_json_client(&path, "helixir-local", &server)
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    let executable = native_client_executable(client)
        .ok_or_else(|| "native client executable not found".to_string())?;
    let backup = if native_registration_exists(client, "helixir-local") {
        Some(backup_native_client_config(client)?)
    } else {
        None
    };
    let update = (|| {
        if backup.is_some() {
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
        if !native_registration_exists(client, "helixir-local") {
            return Err(format!(
                "{} registration could not be verified",
                client.label()
            ));
        }
        Ok(())
    })();
    if update.is_err()
        && let Some((original, backup)) = backup
    {
        let _ = std::fs::copy(backup, original);
    }
    update
}

fn backup_native_client_config(
    client: helixir::installer::ClientKind,
) -> std::result::Result<(PathBuf, PathBuf), String> {
    let home = PathBuf::from(
        std::env::var("HOME")
            .map_err(|_| "HOME is required to back up MCP client config".to_string())?,
    );
    let original = match client {
        helixir::installer::ClientKind::ClaudeCode => home.join(".claude.json"),
        helixir::installer::ClientKind::Codex => home.join(".codex/config.toml"),
        helixir::installer::ClientKind::Cursor => unreachable!("Cursor is JSON-configured"),
    };
    if !original.is_file() {
        return Err(format!(
            "refusing to replace {} registration without a readable config backup",
            client.label()
        ));
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S%.3f");
    let backup = PathBuf::from(format!("{}.bak.{stamp}", original.display()));
    std::fs::copy(&original, &backup).map_err(|error| error.to_string())?;
    Ok((original, backup))
}
