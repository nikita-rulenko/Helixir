use super::*;

pub(crate) fn mode_gate(cmd: &Cmd, mode: MemoryMode) -> Result<()> {
    let needs_insights = matches!(
        cmd,
        Cmd::Clotho { .. }
            | Cmd::Lachesis { .. }
            | Cmd::Atropos { .. }
            | Cmd::Pipeline { .. }
            | Cmd::Daemon {
                cmd: DaemonCmd::Run { .. }
            }
    );
    let needs_collective = matches!(
        cmd,
        Cmd::Swarm { .. } | Cmd::Heartbeat { .. } | Cmd::Debt { .. }
    );
    if needs_insights && !mode.insights_enabled() {
        anyhow::bail!(
            "`{}` needs HELIXIR_MODE=insights (current: {}); the generative Moirai are off by default",
            cmd_name(cmd),
            mode.label()
        );
    }
    if needs_collective && !mode.collective_enabled() {
        anyhow::bail!(
            "`{}` needs HELIXIR_MODE=collective or insights (current: {}); cross-user features are off by default",
            cmd_name(cmd),
            mode.label()
        );
    }
    Ok(())
}

// ============ helixir config (#52) ============

/// The file `config set/edit/apply` operates on: the resolved existing file,
/// else `~/.helixir/helixir.toml` (created on first `set`).
pub(crate) fn config_target_path() -> Result<PathBuf> {
    if let Some(p) = helixir::core::config::HelixirConfig::config_file_path() {
        return Ok(p);
    }
    Ok(helixir_dir()?.join("helixir.toml"))
}

pub(crate) fn config_get(raw: bool) -> Result<()> {
    if raw {
        let p = config_target_path()?;
        match std::fs::read_to_string(&p) {
            Ok(s) => {
                let mut doc: toml_edit::DocumentMut = s.parse().context("parse helixir.toml")?;
                if let Some(token) = doc
                    .get_mut("gateway")
                    .and_then(toml_edit::Item::as_table_mut)
                    .and_then(|gateway| gateway.get_mut("auth_token"))
                {
                    *token = toml_edit::value("<redacted>");
                }
                print!("{doc}");
            }
            Err(_) => println!(
                "# {} does not exist — everything is at defaults",
                p.display()
            ),
        }
        return Ok(());
    }
    let mut resolved = helixir::core::config::HelixirConfig::from_env();
    if resolved.gateway.auth_token.is_some() {
        resolved.gateway.auth_token = Some("<redacted>".to_string());
    }
    println!(
        "# RESOLVED config: defaults -> helixir.toml -> env (env wins)\n{}",
        toml::to_string_pretty(&resolved).context("serialize resolved config")?
    );
    Ok(())
}

/// Validate a helixir.toml body the same way the loader consumes it.
pub(crate) fn config_validate(content: &str) -> Result<()> {
    toml::from_str::<helixir::core::config::HelixirConfig>(content)
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("invalid helixir.toml: {e}"))
}

pub(crate) fn config_set(key: &str, value: &str) -> Result<()> {
    let p = config_target_path()?;
    let content = std::fs::read_to_string(&p).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = content.parse().context("parse helixir.toml")?;

    let segs: Vec<&str> = key.split('.').collect();
    anyhow::ensure!(!segs.is_empty(), "empty key");
    let mut node = doc.as_table_mut();
    for seg in &segs[..segs.len() - 1] {
        node = node
            .entry(seg)
            .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("{seg} exists and is not a table"))?;
    }
    // Try native TOML typing first (5, 5.0, true, [..]); fall back to string.
    let item: toml_edit::Value = value
        .parse()
        .unwrap_or_else(|_| toml_edit::Value::from(value));
    node[segs[segs.len() - 1]] = toml_edit::Item::Value(item);

    let out = doc.to_string();
    config_validate(&out)?; // never persist a file the loader would reject
    std::fs::write(&p, out).with_context(|| format!("write {}", p.display()))?;
    let displayed_value = if key == "gateway.auth_token" {
        "<redacted>"
    } else {
        value
    };
    println!("{} = {} -> {}", key, displayed_value, p.display());
    println!("run `helixir config apply` to hot-reload running processes");
    Ok(())
}

pub(crate) fn config_edit() -> Result<()> {
    let p = config_target_path()?;
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = std::process::Command::new(&editor).arg(&p).status()?;
    anyhow::ensure!(status.success(), "{editor} exited with {status}");
    match std::fs::read_to_string(&p) {
        Ok(content) => match config_validate(&content) {
            Ok(()) => println!("valid — run `helixir config apply` to hot-reload"),
            Err(e) => {
                println!("WARNING: {e}\n(the loader will fall back to DEFAULTS on this file)")
            }
        },
        Err(_) => println!("no file written"),
    }
    Ok(())
}

/// kubectl-apply for the memory (#52): validate, then SIGHUP every process
/// with real reload semantics. The MCP server and the gateway rebuild their
/// client from the re-read file and swap atomically; daemon/watch hold
/// deeper config snapshots and are listed as restart-to-apply.
pub(crate) fn config_apply() -> Result<()> {
    let p = config_target_path()?;
    match std::fs::read_to_string(&p) {
        Ok(content) => config_validate(&content)?,
        Err(_) => println!(
            "note: {} does not exist — defaults + env apply",
            p.display()
        ),
    }
    println!("config valid: {}", p.display());

    #[cfg(unix)]
    {
        let out = std::process::Command::new("pgrep")
            .args(["-f", "helixir-mcp|helixir gateway"])
            .output()?;
        let pids: Vec<i32> = String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .filter(|pid| *pid != std::process::id() as i32)
            .collect();
        if pids.is_empty() {
            println!("no running MCP/gateway processes found — nothing to signal");
        }
        for pid in pids {
            let ok = std::process::Command::new("kill")
                .args(["-HUP", &pid.to_string()])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            println!(
                "SIGHUP -> pid {pid}: {}",
                if ok {
                    "reloading (client rebuilt + swapped)"
                } else {
                    "FAILED"
                }
            );
        }
        for name in ["daemon", "watch"] {
            if let Some(state) = read_pid_state(name) {
                let pid = state.get("pid").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                if is_alive(pid) {
                    println!(
                        "{name} (pid {pid}): restart to apply — `helixir {}`",
                        if name == "daemon" {
                            "daemon stop && helixir daemon start"
                        } else {
                            "watch stop && helixir watch start"
                        }
                    );
                }
            }
        }
        println!("note: active FastThink sessions keep their pre-reload memory handle by design");
        println!(
            "note: processes running a binary OLDER than the hot-reload feature EXIT on SIGHUP\n      (no handler installed) — their supervisor/client restarts them with the new config"
        );
    }
    #[cfg(not(unix))]
    println!("hot-reload signaling is unix-only; restart processes to apply");
    Ok(())
}

pub(crate) fn cmd_name(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::Clotho { .. } => "clotho",
        Cmd::Lachesis { .. } => "lachesis",
        Cmd::Atropos { .. } => "atropos",
        Cmd::Pipeline { .. } => "pipeline",
        Cmd::Daemon { .. } => "daemon run",
        Cmd::Swarm { .. } => "swarm",
        Cmd::Heartbeat { .. } => "heartbeat",
        Cmd::Debt { .. } => "debt",
        Cmd::Watch { .. } => "watch",
        Cmd::Charter => "charter",
        Cmd::PruneAgent { .. } => "prune-agent",
        Cmd::Health { .. } => "health",
        _ => "command",
    }
}

/// Print the effective privilege tier and what it permits.
pub(crate) fn print_mode() -> Result<()> {
    // Layered config (toml + env), same as the gates — a raw env read here
    // showed "solo" while every gate honored the toml's Insights.
    let mode = helixir::core::config::HelixirConfig::from_env().mode;
    let on = |b: bool| if b { "ON" } else { "off" };
    println!("Privilege tier: {} (HELIXIR_MODE)", mode.label());
    println!(
        "  cross-user collective (link / contradict / collective reads): {}",
        on(mode.collective_enabled())
    );
    println!(
        "  generative insights (Clotho/Lachesis/Atropos, daemon):        {}",
        on(mode.insights_enabled())
    );
    if !mode.insights_enabled() {
        println!("\nRaise it: HELIXIR_MODE=collective|insights, or `helixir setup --mode <tier>`.");
    }
    Ok(())
}
