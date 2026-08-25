use super::*;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn gateway_start_detached(bind: &str, require_auth: bool) -> Result<()> {
    let mut args = vec!["gateway", "run", "--bind", bind];
    if require_auth {
        args.push("--require-auth");
    }
    let auth_enabled = helixir::core::config::HelixirConfig::from_env()
        .gateway
        .auth_token
        .is_some_and(|token| !token.is_empty());
    let (pid, log) = spawn_detached(
        "gateway",
        &args,
        serde_json::json!({
            "bind": bind,
            "auth_enabled": auth_enabled,
            "auth_required": require_auth,
        }),
    )?;
    println!(
        "gateway started (pid {pid}) at http://{bind}/mcp; log: {}",
        log.display()
    );
    Ok(())
}

pub(crate) fn gateway_status_detached() -> Result<()> {
    let Some(state) = read_pid_state("gateway") else {
        println!("gateway: stopped (no pid file)");
        return Ok(());
    };
    let pid = state.get("pid").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let bind = state.get("bind").and_then(|v| v.as_str()).unwrap_or("?");
    println!(
        "gateway: {}  pid={pid} url=http://{bind}/mcp auth={} started={}",
        if is_alive(pid) {
            "running"
        } else {
            "STALE (process gone)"
        },
        if state
            .get("auth_required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            "required"
        } else if state
            .get("auth_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            "enabled"
        } else {
            "disabled"
        },
        state
            .get("started_at")
            .and_then(|v| v.as_str())
            .unwrap_or("?"),
    );
    if let Some(l) = state.get("log").and_then(|v| v.as_str()) {
        println!("  log: {l}");
    }
    Ok(())
}

// --- activity journal (append-only JSONL; the daemon will share it) ---
