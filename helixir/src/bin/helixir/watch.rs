use super::*;

pub(crate) async fn watch_run(
    client: &HelixirClient,
    once: bool,
    interval: Option<u64>,
) -> Result<()> {
    let admin = privileged(client).await?;
    let tooling = admin.tooling();
    let watchdog = client.config().watchdog.clone();
    let period = interval.unwrap_or(watchdog.sample_interval_secs);
    let mut hygieia = helixir::agents::hygieia::Hygieia::new(tooling);
    println!(
        "hygieia: watching every {period}s (container: {}) — journal {}",
        if watchdog.container_name.is_empty() {
            "none configured"
        } else {
            &watchdog.container_name
        },
        helixir::agents::hygieia::journal_path().display()
    );
    loop {
        let db_ok = hygieia.check_db().await;
        hygieia.check_memory().await;
        hygieia.check_orphan_daemons().await;
        hygieia.check_storage_persistence().await;
        hygieia.run_backup_duty().await;
        if once {
            println!("tick: db={}", if db_ok { "ok" } else { "DOWN" });
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(period)).await;
    }
}

fn watchdog_actor(home: &std::path::Path) -> Result<String> {
    watchdog_actor_from(home, std::env::var("HELIXIR_RBAC_ACTOR").ok())
}

fn watchdog_actor_from(home: &std::path::Path, explicit: Option<String>) -> Result<String> {
    if let Some(actor) = explicit.filter(|actor| !actor.trim().is_empty()) {
        return Ok(actor);
    }
    let manifest = helixir::installer::manifest::read(&home.join(".helixir/install.json"))
        .context("read install manifest for watchdog RBAC identity")?;
    manifest
        .and_then(|manifest| manifest.rbac)
        .map(|rbac| rbac.operator_id)
        .filter(|actor| !actor.trim().is_empty())
        .context("watchdog RBAC identity is unknown; run onboarding or set HELIXIR_RBAC_ACTOR")
}

fn service_path() -> String {
    "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin".to_string()
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Detach `watch run` as a background service (pid file + log, like the daemon).
/// #75: install the watchdog as a login service so it survives reboots.
/// macOS: a launchd agent at ~/Library/LaunchAgents; Linux: a systemd user
/// unit. The service runs `helixir watch run` in the FOREGROUND — the init
/// system owns the lifecycle, so no pid file is involved.
pub(crate) fn watch_install() -> Result<()> {
    let exe = std::env::current_exe().context("resolve helixir binary path")?;
    let home = std::env::var("HOME").context("HOME not set")?;
    let home_path = std::path::PathBuf::from(&home);
    let actor = watchdog_actor(&home_path)?;
    let path = service_path();
    // The service pins THIS binary path. A target/ path gets overwritten by
    // rebuilds — and on macOS replacing a running executable in place gets
    // it SIGKILLed (the 2026-07-02 incident). Install from the promoted
    // binary instead.
    if exe.components().any(|c| c.as_os_str() == "target") {
        anyhow::bail!(
            "refusing to install a service pinned to a build directory ({}) — \
             install the promoted binary instead: ~/.helixir/bin/helixir watch install",
            exe.display()
        );
    }

    #[cfg(target_os = "macos")]
    {
        let dir = home_path.join("Library/LaunchAgents");
        std::fs::create_dir_all(&dir)?;
        let plist = dir.join("com.helixir.watchdog.plist");
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.helixir.watchdog</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>watch</string>
    <string>run</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HELIXIR_RBAC_ACTOR</key><string>{actor}</string>
    <key>PATH</key><string>{path}</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>{home}/.helixir/watchdog.out.log</string>
  <key>StandardErrorPath</key><string>{home}/.helixir/watchdog.err.log</string>
</dict>
</plist>
"#,
            exe = xml_escape(&exe.display().to_string()),
            actor = xml_escape(&actor),
            path = xml_escape(&path),
            home = xml_escape(&home),
        );
        std::fs::write(&plist, body)?;
        let loaded = std::process::Command::new("launchctl")
            .args(["load", "-w"])
            .arg(&plist)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        println!(
            "Installed launchd agent: {}\nlaunchctl load: {}\nLogs: ~/.helixir/watchdog.{{out,err}}.log",
            plist.display(),
            if loaded {
                "OK (runs now and at every login)"
            } else {
                "FAILED — run manually: launchctl load -w <plist>"
            }
        );
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        let dir = home_path.join(".config/systemd/user");
        std::fs::create_dir_all(&dir)?;
        let unit = dir.join("helixir-watchdog.service");
        let body = format!(
            "[Unit]\nDescription=Helixir health watchdog\n\n[Service]\nExecStart={} watch run\nEnvironment=HELIXIR_RBAC_ACTOR={}\nEnvironment=PATH={}\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
            exe.display(),
            actor,
            path
        );
        std::fs::write(&unit, body)?;
        let ok = std::process::Command::new("systemctl")
            .args(["--user", "enable", "--now", "helixir-watchdog.service"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        println!(
            "Installed systemd user unit: {}\nsystemctl enable --now: {}",
            unit.display(),
            if ok {
                "OK"
            } else {
                "FAILED — run manually: systemctl --user enable --now helixir-watchdog"
            }
        );
        return Ok(());
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("watch install supports macOS (launchd) and Linux (systemd user units)");
    }
}

/// #75: remove the login service installed by `watch install`.
pub(crate) fn watch_uninstall() -> Result<()> {
    let home = std::env::var("HOME").context("HOME not set")?;

    #[cfg(target_os = "macos")]
    {
        let plist =
            std::path::PathBuf::from(&home).join("Library/LaunchAgents/com.helixir.watchdog.plist");
        if !plist.exists() {
            println!("Nothing installed ({} not found).", plist.display());
            return Ok(());
        }
        let _ = std::process::Command::new("launchctl")
            .args(["unload", "-w"])
            .arg(&plist)
            .status();
        std::fs::remove_file(&plist)?;
        println!("Removed {}.", plist.display());
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        let unit =
            std::path::PathBuf::from(&home).join(".config/systemd/user/helixir-watchdog.service");
        if !unit.exists() {
            println!("Nothing installed ({} not found).", unit.display());
            return Ok(());
        }
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "disable", "--now", "helixir-watchdog.service"])
            .status();
        std::fs::remove_file(&unit)?;
        println!("Removed {}.", unit.display());
        return Ok(());
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("watch uninstall supports macOS and Linux");
    }
}

pub(crate) fn watch_start(interval: Option<u64>) -> Result<()> {
    let mut args: Vec<String> = vec!["watch".into(), "run".into()];
    if let Some(i) = interval {
        args.push("--interval".into());
        args.push(i.to_string());
    }
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
    let (pid, log) = spawn_detached("watch", &args_ref, serde_json::json!({}))?;
    println!("watch started (pid {pid}); log: {}", log.display());
    Ok(())
}

/// Pretty-print the tail of Hygieia's health journal.
pub(crate) fn health_tail(n: usize) -> Result<()> {
    let path = helixir::agents::hygieia::journal_path();
    let body = std::fs::read_to_string(&path)
        .with_context(|| format!("no health journal yet at {}", path.display()))?;
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    println!(
        "health events (last {} of {}):",
        lines.len() - start,
        lines.len()
    );
    for line in &lines[start..] {
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(v) => println!(
                "  {}  {:>5}  {:<20}  {}",
                v.get("at")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .get(..16)
                    .unwrap_or(""),
                v.get("severity").and_then(|x| x.as_str()).unwrap_or(""),
                v.get("kind").and_then(|x| x.as_str()).unwrap_or(""),
                v.get("summary").and_then(|x| x.as_str()).unwrap_or("")
            ),
            Err(_) => println!("  {line}"),
        }
    }
    Ok(())
}

pub(crate) fn daemon_stop() -> Result<()> {
    stop_process("daemon")
}

pub(crate) fn daemon_status() -> Result<()> {
    let Some(state) = read_pid_state("daemon") else {
        println!("daemon: stopped (no pid file)");
        return Ok(());
    };
    let pid = state.get("pid").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    println!(
        "daemon: {}  pid={pid} user={} interval={}s started={}",
        if is_alive(pid) {
            "running"
        } else {
            "STALE (process gone)"
        },
        state.get("user").and_then(|v| v.as_str()).unwrap_or("?"),
        state.get("interval").and_then(|v| v.as_u64()).unwrap_or(0),
        state
            .get("started_at")
            .and_then(|v| v.as_str())
            .unwrap_or("?"),
    );
    if let Some(l) = state.get("log").and_then(|v| v.as_str()) {
        println!("  log: {l}");
    }
    if let Ok(body) = std::fs::read_to_string(journal_path())
        && let Some(last) = body
            .lines()
            .filter(|l| l.contains("\"agent\":\"daemon\""))
            .next_back()
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(last)
    {
        println!(
            "  last pass: {} — {}",
            v.get("ts").and_then(|x| x.as_str()).unwrap_or("?"),
            v.get("detail").and_then(|x| x.as_str()).unwrap_or("")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn watchdog_uses_manifest_operator_without_interactive_environment() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("helixir-watchdog-{stamp}"));
        let manifest_path = home.join(".helixir/install.json");
        let manifest = helixir::installer::manifest::InstallManifest {
            version: "test".to_string(),
            install_dir: home.join("current"),
            backend_volume: String::new(),
            backend: Default::default(),
            models: Vec::new(),
            clients: Vec::new(),
            rbac: Some(helixir::installer::rbac::RbacManifest {
                enabled: true,
                operator_id: "codex".to_string(),
                group_id: helixir::core::DEFAULT_GROUP_ID.to_string(),
                principals: vec!["codex".to_string()],
            }),
            last_backup: None,
        };
        helixir::installer::manifest::write(&manifest_path, &manifest).unwrap();

        assert_eq!(watchdog_actor_from(&home, None).unwrap(), "codex");

        std::fs::remove_dir_all(home).unwrap();
    }
}
