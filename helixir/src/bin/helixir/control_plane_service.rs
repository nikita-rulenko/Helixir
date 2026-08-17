//! Reboot-safe lifecycle for the isolated browser control plane.

use super::*;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", test))]
const SERVICE_LABEL: &str = "com.helixir.supervisor";
#[cfg(target_os = "linux")]
const SYSTEMD_UNIT: &str = "helixir-supervisor.service";
const CONTAINER: &str = "helixir-control-plane";
const DEFAULT_WEB_PORT: &str = "6971";

pub(crate) fn control_plane_install(image: Option<&str>) -> Result<()> {
    let exe = promoted_executable()?;
    let home = home_dir()?;
    let actor = control_plane_actor(&home)?;
    let config = helixir::core::config::HelixirConfig::from_env();
    let browser_token = helixir::control_plane::session::default_token_path();
    let supervisor_token = helixir::installer::supervisor::default_token_path();
    helixir::control_plane::session::load_or_create_token(&browser_token)?;
    helixir::installer::supervisor::load_or_create_token(&supervisor_token)?;

    install_supervisor_service(&exe, &home, &actor)?;
    let image = image
        .map(str::to_owned)
        .or_else(|| std::env::var("HELIXIR_CONTROL_PLANE_IMAGE").ok())
        .unwrap_or_else(|| {
            format!(
                "ghcr.io/nikita-rulenko/helixir-control-plane:v{}",
                env!("CARGO_PKG_VERSION")
            )
        });
    ensure_image(&image)?;
    remove_container_if_present()?;
    run_container(
        &image,
        &actor,
        config.host.as_str(),
        config.port,
        config.mode.label(),
        &browser_token,
        &supervisor_token,
    )?;
    let web_port = std::env::var("HELIXIR_WEB_PORT").unwrap_or_else(|_| DEFAULT_WEB_PORT.into());
    println!("Control plane installed: http://127.0.0.1:{web_port}");
    println!("Browser token: {}", browser_token.display());
    println!("Both the supervisor and UI recover automatically after login/reboot.");
    Ok(())
}

pub(crate) fn control_plane_status() -> Result<()> {
    let service = supervisor_service_active();
    let container = command_output("docker", &["inspect", "-f", "{{.State.Status}}", CONTAINER])
        .unwrap_or_else(|_| "not installed".to_string());
    println!("supervisor: {service}");
    println!("control-plane container: {}", container.trim());
    Ok(())
}

pub(crate) fn control_plane_uninstall() -> Result<()> {
    remove_container_if_present()?;
    uninstall_supervisor_service()?;
    println!("Control-plane services removed; browser and supervisor tokens were preserved.");
    Ok(())
}

fn promoted_executable() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("resolve helixir binary path")?;
    if exe
        .components()
        .any(|component| component.as_os_str() == "target")
    {
        anyhow::bail!(
            "refusing to install a service from a build directory ({}); run the promoted ~/.helixir/bin/helixir",
            exe.display()
        );
    }
    Ok(exe)
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

fn control_plane_actor(home: &Path) -> Result<String> {
    if let Ok(actor) = std::env::var("HELIXIR_RBAC_ACTOR")
        && !actor.trim().is_empty()
    {
        return Ok(actor);
    }
    helixir::installer::manifest::read(&home.join(".helixir/install.json"))?
        .and_then(|manifest| manifest.rbac)
        .map(|rbac| rbac.operator_id)
        .filter(|actor| !actor.trim().is_empty())
        .context("control-plane operator is unknown; complete onboarding first")
}

fn ensure_image(image: &str) -> Result<()> {
    if command_success("docker", &["image", "inspect", image]) {
        return Ok(());
    }
    run_checked("docker", &["pull", image], "pull the control-plane image")
}

fn remove_container_if_present() -> Result<()> {
    if command_success("docker", &["container", "inspect", CONTAINER]) {
        run_checked(
            "docker",
            &["rm", "-f", CONTAINER],
            "replace the managed control-plane container",
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_container(
    image: &str,
    actor: &str,
    host: &str,
    port: u16,
    mode: &str,
    browser_token: &Path,
    supervisor_token: &Path,
) -> Result<()> {
    let browser_mount = format!(
        "type=bind,src={},dst=/run/secrets/helixir-control-plane-token,readonly",
        browser_token.display()
    );
    let supervisor_mount = format!(
        "type=bind,src={},dst=/run/secrets/helixir-supervisor-token,readonly",
        supervisor_token.display()
    );
    let port_value = port.to_string();
    let web_port = std::env::var("HELIXIR_WEB_PORT").unwrap_or_else(|_| DEFAULT_WEB_PORT.into());
    let publish = format!("127.0.0.1:{web_port}:6971");
    let backend_host = if matches!(host, "localhost" | "127.0.0.1" | "::1") {
        "host.docker.internal"
    } else {
        host
    };
    let args = vec![
        "run",
        "-d",
        "--name",
        CONTAINER,
        "--restart",
        "unless-stopped",
        "--read-only",
        "--tmpfs",
        "/tmp:rw,noexec,nosuid,size=32m",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges:true",
        "--pids-limit",
        "256",
        "--memory",
        "768m",
        "--log-opt",
        "max-size=10m",
        "--log-opt",
        "max-file=3",
        "--add-host",
        "host.docker.internal:host-gateway",
        "-p",
        &publish,
        "--mount",
        &browser_mount,
        "--mount",
        &supervisor_mount,
        "-e",
        "HELIX_HOST",
        "-e",
        "HELIX_PORT",
        "-e",
        "HELIXIR_RBAC_ACTOR",
        "-e",
        "HELIXIR_MODE",
        "-e",
        "HELIXIR_SUPERVISOR_URL=http://host.docker.internal:6972",
        "-e",
        "HELIXIR_SUPERVISOR_TOKEN_FILE=/run/secrets/helixir-supervisor-token",
        "-e",
        "HELIXIR_CONTROL_PLANE_TOKEN_FILE=/run/secrets/helixir-control-plane-token",
        image,
    ];
    let mut command = std::process::Command::new("docker");
    command
        .args(args)
        .env("HELIX_HOST", backend_host)
        .env("HELIX_PORT", port_value)
        .env("HELIXIR_RBAC_ACTOR", actor)
        .env("HELIXIR_MODE", mode);
    let status = command.status().context("start control-plane container")?;
    anyhow::ensure!(
        status.success(),
        "docker failed to start the control-plane container"
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_supervisor_service(exe: &Path, home: &Path, actor: &str) -> Result<()> {
    let dir = home.join("Library/LaunchAgents");
    fs::create_dir_all(&dir)?;
    let plist = dir.join(format!("{SERVICE_LABEL}.plist"));
    fs::write(&plist, launchd_plist(exe, home, actor))?;
    let domain = format!("gui/{}", command_output("id", &["-u"])?);
    let target = format!("{domain}/{SERVICE_LABEL}");
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &target])
        .status();
    run_checked(
        "launchctl",
        &["bootstrap", &domain, plist.to_str().context("plist path")?],
        "register supervisor LaunchAgent",
    )?;
    run_checked(
        "launchctl",
        &["kickstart", "-k", &target],
        "start supervisor LaunchAgent",
    )
}

#[cfg(target_os = "linux")]
fn install_supervisor_service(exe: &Path, home: &Path, actor: &str) -> Result<()> {
    let dir = home.join(".config/systemd/user");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(SYSTEMD_UNIT), systemd_unit(exe, actor))?;
    run_checked(
        "systemctl",
        &["--user", "daemon-reload"],
        "reload user services",
    )?;
    run_checked(
        "systemctl",
        &["--user", "enable", "--now", SYSTEMD_UNIT],
        "enable supervisor service",
    )
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn install_supervisor_service(_: &Path, _: &Path, _: &str) -> Result<()> {
    anyhow::bail!("control-plane service installation currently supports macOS and Linux")
}

#[cfg(target_os = "macos")]
fn uninstall_supervisor_service() -> Result<()> {
    let home = home_dir()?;
    let plist = home.join(format!("Library/LaunchAgents/{SERVICE_LABEL}.plist"));
    if plist.exists() {
        let domain = format!("gui/{}", command_output("id", &["-u"])?);
        let target = format!("{domain}/{SERVICE_LABEL}");
        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &target])
            .status();
        fs::remove_file(plist)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall_supervisor_service() -> Result<()> {
    let unit = home_dir()?.join(format!(".config/systemd/user/{SYSTEMD_UNIT}"));
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "--now", SYSTEMD_UNIT])
        .status();
    if unit.exists() {
        fs::remove_file(unit)?;
    }
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn uninstall_supervisor_service() -> Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn supervisor_service_active() -> String {
    let uid = command_output("id", &["-u"]).unwrap_or_default();
    if command_success(
        "launchctl",
        &["print", &format!("gui/{uid}/{SERVICE_LABEL}")],
    ) {
        "active (launchd)"
    } else {
        "inactive"
    }
    .into()
}

#[cfg(target_os = "linux")]
fn supervisor_service_active() -> String {
    command_output("systemctl", &["--user", "is-active", SYSTEMD_UNIT])
        .unwrap_or_else(|_| "inactive".into())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn supervisor_service_active() -> String {
    "unsupported".into()
}

#[cfg(any(target_os = "macos", test))]
fn launchd_plist(exe: &Path, home: &Path, actor: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{SERVICE_LABEL}</string>
<key>ProgramArguments</key><array><string>{}</string><string>supervisor</string></array>
<key>EnvironmentVariables</key><dict><key>HELIXIR_RBAC_ACTOR</key><string>{}</string><key>PATH</key><string>/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin</string></dict>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
<key>StandardOutPath</key><string>{}/.helixir/supervisor.out.log</string>
<key>StandardErrorPath</key><string>{}/.helixir/supervisor.err.log</string>
</dict></plist>
"#,
        xml_escape(&exe.display().to_string()),
        xml_escape(actor),
        xml_escape(&home.display().to_string()),
        xml_escape(&home.display().to_string())
    )
}

#[cfg(any(target_os = "linux", test))]
fn systemd_unit(exe: &Path, actor: &str) -> String {
    format!(
        "[Unit]\nDescription=Helixir control-plane host supervisor\nAfter=default.target\n\n[Service]\nExecStart={} supervisor\nEnvironment=HELIXIR_RBAC_ACTOR={}\nEnvironment=PATH=/usr/local/bin:/usr/bin:/bin\nRestart=always\nRestartSec=2\n\n[Install]\nWantedBy=default.target\n",
        exe.display(),
        actor
    )
}

#[cfg(any(target_os = "macos", test))]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn command_success(program: &str, args: &[&str]) -> bool {
    std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn command_output(program: &str, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new(program)
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

fn run_checked(program: &str, args: &[&str], context: &str) -> Result<()> {
    let status = std::process::Command::new(program)
        .args(args)
        .status()
        .with_context(|| context.to_string())?;
    anyhow::ensure!(status.success(), "{context} failed with {status}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_definitions_restart_and_never_use_a_shell() {
        let exe = Path::new("/opt/helixir/current/helixir");
        let plist = launchd_plist(exe, Path::new("/Users/operator"), "operator");
        assert!(plist.contains("<key>KeepAlive</key><true/>"));
        assert!(plist.contains("<string>supervisor</string>"));
        assert!(!plist.contains("sh -c"));
        let unit = systemd_unit(exe, "operator");
        assert!(unit.contains("Restart=always"));
        assert!(unit.contains("ExecStart=/opt/helixir/current/helixir supervisor"));
    }

    #[test]
    fn launchd_values_are_xml_escaped() {
        assert_eq!(xml_escape("a&<b>\"'"), "a&amp;&lt;b&gt;&quot;&apos;");
    }
}
