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

/// Detach `watch run` as a background service (pid file + log, like the daemon).
/// #75: install the watchdog as a login service so it survives reboots.
/// macOS: a launchd agent at ~/Library/LaunchAgents; Linux: a systemd user
/// unit. The service runs `helixir watch run` in the FOREGROUND — the init
/// system owns the lifecycle, so no pid file is involved.
pub(crate) fn watch_install() -> Result<()> {
    let exe = std::env::current_exe().context("resolve helixir binary path")?;
    let home = std::env::var("HOME").context("HOME not set")?;
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
        let dir = std::path::PathBuf::from(&home).join("Library/LaunchAgents");
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
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>{home}/.helixir/watchdog.out.log</string>
  <key>StandardErrorPath</key><string>{home}/.helixir/watchdog.err.log</string>
</dict>
</plist>
"#,
            exe = exe.display(),
            home = home,
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
        let dir = std::path::PathBuf::from(&home).join(".config/systemd/user");
        std::fs::create_dir_all(&dir)?;
        let unit = dir.join("helixir-watchdog.service");
        let body = format!(
            "[Unit]\nDescription=Helixir health watchdog\n\n[Service]\nExecStart={} watch run\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
            exe.display()
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
