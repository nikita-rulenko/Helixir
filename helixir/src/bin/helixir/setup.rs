use super::*;

pub(crate) struct SetupConfig {
    pub(crate) host: String,
    pub(crate) port: String,
    pub(crate) instance: String,
    pub(crate) llm_provider: String,
    pub(crate) llm_model: String,
    pub(crate) llm_key: String,
    pub(crate) emb_provider: String,
    pub(crate) emb_model: String,
    pub(crate) emb_url: String,
    pub(crate) mcp_bin: String,
    /// Privilege tier written as HELIXIR_MODE (default solo).
    pub(crate) mode: String,
}

pub(crate) fn default_mcp_bin() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("helixir-mcp")))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "helixir-mcp".to_string())
}

pub(crate) fn gather_config(
    interactive: bool,
    discovered: Option<(String, u16)>,
) -> Result<SetupConfig> {
    let e = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    let mut c = SetupConfig {
        host: e("HELIX_HOST", "localhost"),
        port: e("HELIX_PORT", "6970"),
        instance: e("HELIX_INSTANCE", "bench"),
        llm_provider: e("HELIX_LLM_PROVIDER", "ollama"),
        llm_model: e("HELIX_LLM_MODEL", helixir::DEFAULT_LLM_MODEL),
        llm_key: e("HELIX_LLM_API_KEY", ""),
        emb_provider: e("HELIX_EMBEDDING_PROVIDER", "ollama"),
        emb_model: e("HELIX_EMBEDDING_MODEL", "nomic-embed-text"),
        emb_url: e("HELIX_EMBEDDING_URL", "http://localhost:11434"),
        mcp_bin: default_mcp_bin(),
        mode: e("HELIXIR_MODE", "solo"),
    };
    // A discovered backend pre-fills host/port — but only where the user has not
    // explicitly pinned them via env, so a scripted run with HELIX_* still wins.
    if let Some((h, p)) = discovered {
        if std::env::var("HELIX_HOST").is_err() {
            c.host = h;
        }
        if std::env::var("HELIX_PORT").is_err() {
            c.port = p.to_string();
        }
    }
    if interactive {
        let ask = |prompt: &str, def: &str| -> Result<String> {
            Ok(Input::<String>::new()
                .with_prompt(prompt)
                .default(def.to_string())
                .allow_empty(true)
                .interact_text()?)
        };
        c.host = ask("HelixDB host", &c.host)?;
        c.port = ask("HelixDB port", &c.port)?;
        c.instance = ask("HelixDB instance", &c.instance)?;
        c.llm_provider = ask("LLM provider (cerebras / ollama)", &c.llm_provider)?;
        c.llm_model = ask("LLM model", &c.llm_model)?;
        c.llm_key = ask("LLM API key (blank for local)", &c.llm_key)?;
        c.emb_model = ask("Embedding model", &c.emb_model)?;
        c.emb_url = ask("Embedding URL", &c.emb_url)?;
        c.mcp_bin = ask("Path to the helixir-mcp binary", &c.mcp_bin)?;
    }
    Ok(c)
}

pub(crate) fn mcp_entry(c: &SetupConfig) -> serde_json::Value {
    serde_json::json!({
        "command": c.mcp_bin,
        "args": [],
        "env": {
            "HELIXIR_SELF_SEED": "1",
            "HELIX_HOST": c.host,
            "HELIX_PORT": c.port,
            "HELIX_INSTANCE": c.instance,
            "HELIX_LLM_PROVIDER": c.llm_provider,
            "HELIX_LLM_MODEL": c.llm_model,
            "HELIX_LLM_API_KEY": c.llm_key,
            "HELIX_EMBEDDING_PROVIDER": c.emb_provider,
            "HELIX_EMBEDDING_MODEL": c.emb_model,
            "HELIX_EMBEDDING_URL": c.emb_url,
            "HELIXIR_RETRIEVAL_PROFILE": "algo_opt",
            "HELIXIR_MODE": c.mode,
        }
    })
}

/// Normalize a gateway arg (URL or `host:port`) to a full streamable-http URL.
pub(crate) fn normalize_gateway_url(raw: &str) -> String {
    let s = raw.trim();
    let with_scheme = if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("http://{s}")
    };
    if with_scheme.trim_end_matches('/').ends_with("/mcp") {
        with_scheme
    } else {
        format!("{}/mcp", with_scheme.trim_end_matches('/'))
    }
}

/// Client entry for a remote gateway: HTTP transport, no command, no env — the
/// gateway holds all the HELIX_* config.
pub(crate) fn mcp_entry_gateway(url: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "http",
        "url": url,
    })
}

pub(crate) fn client_targets() -> Vec<(String, PathBuf)> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let desktop = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Claude/claude_desktop_config.json")
    } else {
        home.join(".config/Claude/claude_desktop_config.json")
    };
    vec![
        ("Claude Desktop".to_string(), desktop),
        ("Cursor".to_string(), home.join(".cursor/mcp.json")),
        ("Gemini CLI".to_string(), home.join(".gemini/settings.json")),
    ]
}

/// Native CLI clients own their configuration and must not be treated as JSON
/// files. Returns only clients whose executable can be resolved from PATH (or
/// the known Codex.app location).
pub(crate) fn native_client_targets() -> Vec<helixir::installer::ClientKind> {
    use helixir::installer::ClientKind;
    [ClientKind::ClaudeCode, ClientKind::Codex]
        .into_iter()
        .filter(|client| {
            let executable = match client {
                ClientKind::ClaudeCode => "claude",
                ClientKind::Codex => "codex",
                ClientKind::Cursor => return false,
            };
            helixir::installer::clients::resolve_command(executable).is_some()
                || (*client == ClientKind::Codex
                    && Path::new("/Applications/Codex.app/Contents/Resources/codex").exists())
        })
        .collect()
}

pub(crate) fn native_client_executable(client: helixir::installer::ClientKind) -> Option<PathBuf> {
    let command = match client {
        helixir::installer::ClientKind::ClaudeCode => "claude",
        helixir::installer::ClientKind::Codex => "codex",
        helixir::installer::ClientKind::Cursor => return None,
    };
    helixir::installer::clients::resolve_command(command).or_else(|| {
        (client == helixir::installer::ClientKind::Codex)
            .then(|| PathBuf::from("/Applications/Codex.app/Contents/Resources/codex"))
            .filter(|path| path.exists())
    })
}

/// Whether a native client already owns a server with this name.
pub(crate) fn native_registration_exists(
    client: helixir::installer::ClientKind,
    server_name: &str,
) -> bool {
    let Some(executable) = native_client_executable(client) else {
        return false;
    };
    Command::new(executable)
        .args(["mcp", "get", server_name])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Register Claude Code/Codex using their native CLI commands. No provider
/// environment is passed; the command points at a stable MCP binary and the
/// future central config path will be the only allowed non-secret env value.
pub(crate) fn wire_native_clients(
    server: &helixir::installer::clients::StdioServer,
    interactive: bool,
    dry_run: bool,
) -> Result<()> {
    let available = native_client_targets();
    if available.is_empty() {
        return Ok(());
    }
    let selected = if interactive {
        let labels: Vec<_> = available.iter().map(|client| client.label()).collect();
        let picks = MultiSelect::new()
            .with_prompt("Register Helixir through native client CLIs?")
            .items(&labels)
            .defaults(&vec![true; available.len()])
            .interact()?;
        picks
            .into_iter()
            .map(|idx| available[idx])
            .collect::<Vec<_>>()
    } else {
        available
    };
    for client in selected {
        if native_registration_exists(client, "helixir-local") {
            println!(
                "  ✓ {}: helixir-local already exists; leaving it untouched",
                client.label()
            );
            continue;
        }
        let command =
            helixir::installer::clients::native_add_command(client, "helixir-local", server);
        if dry_run {
            println!(
                "  [dry-run] {}: {}",
                client.label(),
                command.argv().join(" ")
            );
            continue;
        }
        let Some(executable) = native_client_executable(client) else {
            println!(
                "  ✗ {}: executable disappeared during setup",
                client.label()
            );
            continue;
        };
        let status = Command::new(executable)
            .args(&command.args)
            .status()
            .with_context(|| format!("run {} MCP registration", client.label()))?;
        anyhow::ensure!(
            status.success(),
            "{} MCP registration exited with {status}",
            client.label()
        );
        anyhow::ensure!(
            native_registration_exists(client, "helixir-local"),
            "{} registration could not be verified",
            client.label()
        );
        println!("  ✓ {}: helixir-local registered", client.label());
    }
    Ok(())
}

/// Merge the `helixir-local` MCP entry into a client's config JSON (creating
/// `mcpServers` if absent), backing the file up first. Non-destructive: other
/// servers and keys are preserved.
pub(crate) fn wire_client(
    name: &str,
    path: &Path,
    entry: &serde_json::Value,
    dry_run: bool,
) -> Result<()> {
    let existing = if path.exists() {
        Some(std::fs::read_to_string(path)?)
    } else {
        None
    };
    let merged = helixir::installer::client_config::merge_mcp_server(
        existing.as_deref(),
        "helixir-local",
        entry,
    )
    .with_context(|| format!("refusing unsafe update of {}", path.display()))?;

    if !merged.changed {
        println!(
            "  ✓ {name}: helixir-local already matches {}",
            path.display()
        );
        return Ok(());
    }

    if dry_run {
        println!(
            "  [dry-run] {name}: would set helixir-local in {}",
            path.display()
        );
        return Ok(());
    }
    if path.exists() {
        std::fs::copy(path, PathBuf::from(format!("{}.bak", path.display())))
            .with_context(|| format!("back up {}", path.display()))?;
    } else if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&merged.document)?)?;
    println!(
        "  ✓ {name}: wired helixir-local → {} (backup .bak)",
        path.display()
    );
    Ok(())
}

// Backend probing and setup execution live in the adjacent module.
