use super::*;

pub(crate) fn helixir_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let dir = PathBuf::from(home).join(".helixir");
    std::fs::create_dir_all(&dir).ok();
    Ok(dir)
}

pub(crate) fn pid_file(name: &str) -> Result<PathBuf> {
    Ok(helixir_dir()?.join(format!("{name}.pid")))
}

pub(crate) fn read_pid_state(name: &str) -> Option<serde_json::Value> {
    let body = std::fs::read_to_string(pid_file(name).ok()?).ok()?;
    serde_json::from_str(&body).ok()
}

/// Signal 0 probes a pid's existence without delivering anything.
#[cfg(unix)]
pub(crate) fn is_alive(pid: i32) -> bool {
    pid > 0 && unsafe { libc::kill(pid, 0) == 0 }
}

/// Windows has no signal 0; the detached-process machinery is unix-only, so
/// any recorded pid is treated as gone (stale state files self-clean).
#[cfg(not(unix))]
pub(crate) fn is_alive(_pid: i32) -> bool {
    false
}

/// Spawn `helixir <args>` as a detached background process (setsid), logging to
/// `~/.helixir/<name>.log` and recording a `<name>.pid` state file. Shared by
/// the daemon (#43) and the gateway (#42). Returns the child pid.
#[cfg(unix)]
pub(crate) fn spawn_detached(
    name: &str,
    args: &[&str],
    extra: serde_json::Value,
) -> Result<(u32, PathBuf)> {
    if let Some(pid) = read_pid_state(name).and_then(|s| s.get("pid").and_then(|v| v.as_i64()))
        && is_alive(pid as i32)
    {
        anyhow::bail!("{name} already running (pid {pid}); `helixir {name} stop` first");
    }
    let exe = std::env::current_exe().context("current_exe")?;
    let log = helixir_dir()?.join(format!("{name}.log"));
    let out = OpenOptions::new().create(true).append(true).open(&log)?;
    let err = out.try_clone()?;

    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));
    // Detach from the controlling terminal so it survives the shell closing.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let pid = cmd.spawn().context("spawn detached process")?.id();

    let mut state = serde_json::json!({
        "pid": pid,
        "started_at": chrono::Utc::now().to_rfc3339(),
        "log": log.display().to_string(),
    });
    if let (Some(obj), Some(more)) = (state.as_object_mut(), extra.as_object()) {
        for (k, v) in more {
            obj.insert(k.clone(), v.clone());
        }
    }
    std::fs::write(pid_file(name)?, serde_json::to_string_pretty(&state)?)?;
    Ok((pid, log))
}

/// Detached background processes need setsid/pre_exec — unix-only. On
/// Windows the foreground variants (`helixir daemon run`, `helixir gateway`
/// in its own terminal) cover the same ground.
#[cfg(not(unix))]
pub(crate) fn spawn_detached(
    name: &str,
    _args: &[&str],
    _extra: serde_json::Value,
) -> Result<(u32, PathBuf)> {
    anyhow::bail!(
        "`helixir {name} start` (detached background process) is unix-only; on Windows run the foreground variant (e.g. `helixir daemon run`) in its own terminal"
    )
}

/// SIGTERM the named background process and clean up its pid file.
#[cfg(unix)]
pub(crate) fn stop_process(name: &str) -> Result<()> {
    let Some(state) = read_pid_state(name) else {
        println!("{name} not running (no pid file)");
        return Ok(());
    };
    let pid = state
        .get("pid")
        .and_then(|v| v.as_i64())
        .context("pid file has no pid")? as i32;
    if is_alive(pid) {
        unsafe { libc::kill(pid, libc::SIGTERM) };
        println!("{name} stopped (pid {pid})");
    } else {
        println!("{name} already gone (stale pid {pid}); cleaned up");
    }
    std::fs::remove_file(pid_file(name)?).ok();
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn stop_process(name: &str) -> Result<()> {
    // No detached processes exist on Windows (see spawn_detached); just
    // clear any stale state file copied over from a unix machine.
    std::fs::remove_file(pid_file(name)?).ok();
    println!("{name} not running (background processes are unix-only on this platform)");
    Ok(())
}

pub(crate) fn daemon_start(
    user: &str,
    interval: u64,
    threshold: f64,
    max_seeds: usize,
    max_hops: usize,
    cadence: [(&str, Option<u64>); 4],
) -> Result<()> {
    let interval_s = interval.to_string();
    let threshold_s = threshold.to_string();
    let max_seeds_s = max_seeds.to_string();
    let max_hops_s = max_hops.to_string();
    let mut args: Vec<String> = vec![
        "daemon".into(),
        "run".into(),
        "--user".into(),
        user.into(),
        "--interval".into(),
        interval_s,
        "--threshold".into(),
        threshold_s,
        "--max-seeds".into(),
        max_seeds_s,
        "--max-hops".into(),
        max_hops_s,
    ];
    for (flag, v) in cadence {
        if let Some(v) = v {
            args.push(flag.into());
            args.push(v.to_string());
        }
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (pid, log) = spawn_detached(
        "daemon",
        &arg_refs,
        serde_json::json!({
            "user": user, "interval": interval, "threshold": threshold,
            "max_seeds": max_seeds, "max_hops": max_hops,
        }),
    )?;
    println!(
        "daemon started (pid {pid}) for '{user}', every {interval}s; log: {}",
        log.display()
    );
    Ok(())
}

// Foreground watchdog execution lives in the adjacent module.
