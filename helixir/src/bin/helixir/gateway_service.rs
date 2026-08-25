//! Reboot-safe lifecycle for the shared HTTP MCP gateway.

use super::*;
#[path = "gateway_service_platform.rs"]
mod platform;
use platform::*;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::fs;
use std::net::{SocketAddr, TcpStream};

#[cfg(any(target_os = "macos", test))]
const LAUNCHD_LABEL: &str = "com.helixir.gateway";
#[cfg(target_os = "linux")]
const SYSTEMD_UNIT: &str = "helixir-gateway.service";
#[cfg(any(target_os = "macos", target_os = "linux"))]
const TRANSIENT_CONFIG_ENV: &[&str] = &[
    "HELIX_HOST",
    "HELIX_PORT",
    "HELIX_INSTANCE",
    "HELIXIR_MODE",
    "HELIXIR_GATEWAY_TOKEN",
    "HELIXIR_GATEWAY_PUBLIC_URL",
    "HELIX_LLM_PROVIDER",
    "HELIX_LLM_MODEL",
    "HELIX_LLM_API_KEY",
    "HELIX_LLM_BASE_URL",
    "HELIX_LLM_FALLBACK_CHAIN",
    "HELIX_DEEPSEEK_API_KEY",
    "HELIX_DEEPSEEK_MODEL",
    "HELIX_EMBEDDING_PROVIDER",
    "HELIX_EMBEDDING_MODEL",
    "HELIX_EMBEDDING_URL",
    "HELIX_EMBEDDING_API_KEY",
    "HELIX_MAX_FACTS_PER_CALL",
];

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn gateway_service_start(bind: &str, require_auth: bool) -> Result<()> {
    let address = bind
        .parse::<SocketAddr>()
        .with_context(|| format!("invalid gateway bind address {bind}"))?;
    stop_legacy_detached_gateway()?;
    let exe = promoted_gateway_executable()?;
    let home = gateway_home()?;
    let config_path = durable_gateway_config(require_auth)?;
    install_gateway_service(&exe, &home, bind, require_auth, config_path.as_deref())?;
    wait_for_gateway(address, &exe, bind, require_auth)?;
    println!(
        "gateway service started at http://{bind}/mcp ({})",
        gateway_service_recovery()
    );
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn wait_for_gateway(
    address: SocketAddr,
    executable: &Path,
    bind: &str,
    require_auth: bool,
) -> Result<()> {
    for _ in 0..100 {
        if let Some(pid) = managed_gateway_pid()
            && process_command(pid).is_some_and(|command| {
                gateway_command_matches_exact(&command, executable, bind, require_auth)
            })
            && TcpStream::connect_timeout(&address, std::time::Duration::from_millis(50)).is_ok()
        {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    anyhow::bail!("gateway service did not own a healthy listener at {address} within 5 seconds")
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn gateway_service_start(bind: &str, require_auth: bool) -> Result<()> {
    gateway_start_detached(bind, require_auth)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn gateway_service_stop() -> Result<()> {
    uninstall_gateway_service()?;
    if read_pid_state("gateway").is_some() {
        stop_process("gateway")?;
    }
    println!("gateway service stopped and disabled");
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn gateway_service_stop() -> Result<()> {
    stop_process("gateway")
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn gateway_service_status() -> Result<()> {
    let status = managed_gateway_status();
    println!("gateway service: {status}");
    if managed_gateway_status_is_healthy(&status) {
        return Ok(());
    }
    if read_pid_state("gateway").is_some() {
        println!("legacy detached process:");
        gateway_status_detached()?;
    }
    anyhow::bail!("gateway has no healthy reboot-safe service owner; run `helixir gateway start`")
}

#[cfg(any(target_os = "macos", target_os = "linux", test))]
fn managed_gateway_status_is_healthy(status: &str) -> bool {
    status.starts_with("active (")
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn gateway_service_status() -> Result<()> {
    gateway_status_detached()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn stop_legacy_detached_gateway() -> Result<()> {
    let Some(state) = read_pid_state("gateway") else {
        return Ok(());
    };
    let pid = state
        .get("pid")
        .and_then(serde_json::Value::as_i64)
        .context("gateway pid file has no pid")?;
    if !is_alive(pid as i32) {
        std::fs::remove_file(pid_file("gateway")?).ok();
        return Ok(());
    }
    if state.get("executable").is_some() && state.get("args").is_some() {
        stop_process("gateway")?;
    } else if legacy_gateway_process_matches(pid as i32, &state) {
        let result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        anyhow::ensure!(result == 0, "failed to signal legacy gateway process {pid}");
        std::fs::remove_file(pid_file("gateway")?).ok();
        println!("legacy gateway stopped (pid {pid})");
    } else {
        std::fs::remove_file(pid_file("gateway")?).ok();
        eprintln!(
            "ignored stale gateway pid {pid}: the live process identity does not match a Helixir gateway"
        );
        return Ok(());
    }
    for _ in 0..40 {
        if !is_alive(pid as i32) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    anyhow::bail!("legacy gateway process {pid} did not stop")
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn legacy_gateway_process_matches(pid: i32, state: &serde_json::Value) -> bool {
    let Some(command) = process_command(pid) else {
        return false;
    };
    legacy_gateway_command_matches(&command, state)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn legacy_gateway_command_matches(command: &str, state: &serde_json::Value) -> bool {
    let Some(bind) = state.get("bind").and_then(serde_json::Value::as_str) else {
        return false;
    };
    let auth = state
        .get("auth_required")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let suffix = format!(
        " gateway run --bind {bind}{}",
        if auth { " --require-auth" } else { "" }
    );
    let Some(executable) = command.strip_suffix(&suffix) else {
        return false;
    };
    Path::new(executable)
        .file_name()
        .is_some_and(|name| name == "helixir")
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn gateway_command_matches_exact(
    command: &str,
    executable: &Path,
    bind: &str,
    require_auth: bool,
) -> bool {
    command
        == format!(
            "{} gateway run --bind {bind}{}",
            executable.display(),
            if require_auth { " --require-auth" } else { "" }
        )
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn gateway_command_matches_any(command: &str) -> bool {
    let Some((executable, arguments)) = command.split_once(" gateway run --bind ") else {
        return false;
    };
    Path::new(executable)
        .file_name()
        .is_some_and(|name| name == "helixir")
        && !arguments.trim().is_empty()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn promoted_gateway_executable() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("resolve helixir binary path")?;
    if exe
        .components()
        .any(|component| component.as_os_str() == "target")
    {
        anyhow::bail!(
            "refusing to install a gateway service from a build directory ({}); run the promoted ~/.helixir/bin/helixir",
            exe.display()
        );
    }
    Ok(exe)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn gateway_home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn durable_gateway_config(require_auth: bool) -> Result<Option<PathBuf>> {
    let transient = TRANSIENT_CONFIG_ENV
        .iter()
        .copied()
        .filter(|name| std::env::var_os(name).is_some())
        .collect::<Vec<_>>();
    anyhow::ensure!(
        transient.is_empty(),
        "managed gateway cannot preserve transient config overrides ({}); persist them with `helixir config set` before installing the service",
        transient.join(", ")
    );
    let effective_has_token = helixir::core::config::HelixirConfig::from_env()
        .gateway
        .auth_token
        .is_some_and(|token| !token.is_empty());
    let path = helixir::core::config::HelixirConfig::config_file_path();
    let persistent_has_token = path
        .as_deref()
        .map(persistent_gateway_token_exists)
        .transpose()?
        .unwrap_or(false);
    anyhow::ensure!(
        !effective_has_token || persistent_has_token,
        "gateway authentication exists only in the current environment; persist gateway.auth_token with `helixir config set` before installing the service"
    );
    anyhow::ensure!(
        !require_auth || persistent_has_token,
        "--require-auth needs gateway.auth_token in the protected central config"
    );
    Ok(path)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn persistent_gateway_token_exists(path: &Path) -> Result<bool> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("read gateway config {}", path.display()))?;
    let value = toml::from_str::<toml::Value>(&content)
        .with_context(|| format!("parse gateway config {}", path.display()))?;
    Ok(value
        .get("gateway")
        .and_then(|gateway| gateway.get("auth_token"))
        .and_then(toml::Value::as_str)
        .is_some_and(|token| !token.is_empty()))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn gateway_command_success(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn gateway_command_output(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("run {program}"))?;
    anyhow::ensure!(
        output.status.success(),
        "{program} returned {}",
        output.status
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn gateway_run_checked(program: &str, args: &[&str], context: &str) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| context.to_string())?;
    anyhow::ensure!(status.success(), "{context} failed with {status}");
    Ok(())
}

#[cfg(target_os = "macos")]
fn wait_for_launchd_absent(target: &str) -> Result<()> {
    for _ in 0..40 {
        if !gateway_command_success("launchctl", &["print", target]) {
            // launchd may report the job gone slightly before it permits the
            // same label to be bootstrapped again.
            std::thread::sleep(std::time::Duration::from_millis(250));
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    anyhow::bail!("gateway LaunchAgent {target} did not finish stopping")
}

#[cfg(target_os = "macos")]
fn gateway_run_retry(program: &str, args: &[&str], context: &str) -> Result<()> {
    let mut last = None;
    for _ in 0..20 {
        let status = Command::new(program)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| context.to_string())?;
        if status.success() {
            return Ok(());
        }
        last = Some(status);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    anyhow::bail!(
        "{context} failed after retries with {}",
        last.context("gateway service command did not run")?
    )
}

#[cfg(test)]
#[path = "gateway_service_tests.rs"]
mod tests;
