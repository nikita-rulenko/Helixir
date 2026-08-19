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

fn is_secret_key(key: &str) -> bool {
    let normalized = key.trim().to_ascii_lowercase().replace('-', "_");
    normalized == "key"
        || normalized == "token"
        || normalized == "password"
        || normalized == "secret"
        || normalized == "credential"
        || normalized.ends_with("_key")
        || normalized.ends_with("_token")
        || normalized.ends_with("_password")
        || normalized.ends_with("_secret")
        || normalized.ends_with("_credential")
}

fn redact_secrets(value: &mut toml::Value) {
    match value {
        toml::Value::Table(table) => {
            for (key, value) in table.iter_mut() {
                if is_secret_key(key) {
                    *value = toml::Value::String("<redacted>".to_string());
                } else {
                    redact_secrets(value);
                }
            }
        }
        toml::Value::Array(values) => values.iter_mut().for_each(redact_secrets),
        _ => {}
    }
}

fn redacted_toml(content: &str) -> Result<String> {
    let mut value: toml::Value = toml::from_str(content).context("parse helixir.toml")?;
    redact_secrets(&mut value);
    toml::to_string_pretty(&value).context("serialize redacted helixir.toml")
}

pub(crate) fn config_get(raw: bool) -> Result<()> {
    if raw {
        let p = config_target_path()?;
        match std::fs::read_to_string(&p) {
            Ok(s) => print!("{}", redacted_toml(&s)?),
            Err(_) => println!(
                "# {} does not exist — everything is at defaults",
                p.display()
            ),
        }
        return Ok(());
    }
    let resolved = helixir::core::config::HelixirConfig::from_env();
    let serialized = toml::to_string_pretty(&resolved).context("serialize resolved config")?;
    println!(
        "# RESOLVED config: defaults -> helixir.toml -> env (env wins)\n{}",
        redacted_toml(&serialized)?
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
    let displayed_value = if key.rsplit('.').next().is_some_and(is_secret_key) {
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

    let receipt = helixir::installer::settings_reload::reload()?;
    println!(
        "reload signals: {} succeeded, {} failed",
        receipt.signalled_processes, receipt.failed_signals
    );
    for process in receipt.restart_required {
        println!("{process}: restart required to apply deeper configuration snapshots");
    }
    println!("note: active FastThink sessions keep their pre-reload memory handle by design");
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

#[cfg(test)]
mod redaction_tests {
    use super::*;

    #[test]
    fn classifies_current_and_future_secret_fields() {
        for key in [
            "api_key",
            "llm_api_key",
            "deepseek_api_key",
            "embedding_api_key",
            "auth_token",
            "service-password",
            "client_secret",
            "credential",
        ] {
            assert!(is_secret_key(key), "{key} must be secret");
        }
        for key in ["model", "max_tokens", "base_url", "monkey"] {
            assert!(!is_secret_key(key), "{key} must remain inspectable");
        }
    }

    #[test]
    fn redacts_nested_secrets_and_preserves_normal_values() {
        let redacted = redacted_toml(
            r#"
llm_api_key = "llm-secret"
embedding_api_key = "embedding-secret"
deepseek_api_key = "deepseek-secret"
model = "gpt-oss-120b"

[gateway]
auth_token = "gateway-secret"

[[fallback_chain]]
provider = "remote"
api_key = "fallback-secret"
"#,
        )
        .expect("redact config");

        for secret in [
            "llm-secret",
            "embedding-secret",
            "deepseek-secret",
            "gateway-secret",
            "fallback-secret",
        ] {
            assert!(!redacted.contains(secret));
        }
        assert_eq!(redacted.matches("<redacted>").count(), 5);
        assert!(redacted.contains("gpt-oss-120b"));
        assert!(redacted.contains("remote"));
    }
}
