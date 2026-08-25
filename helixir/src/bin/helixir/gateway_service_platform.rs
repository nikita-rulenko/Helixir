//! Platform-specific gateway service definitions and ownership inspection.

use super::*;

#[cfg(target_os = "macos")]
pub(super) fn gateway_service_recovery() -> &'static str {
    "restarts after login/reboot"
}

#[cfg(target_os = "linux")]
pub(super) fn gateway_service_recovery() -> &'static str {
    "restarts after user login; enable systemd linger for pre-login boot"
}

#[cfg(target_os = "macos")]
pub(super) fn install_gateway_service(
    exe: &Path,
    home: &Path,
    bind: &str,
    require_auth: bool,
    config_path: Option<&Path>,
) -> Result<()> {
    let dir = home.join("Library/LaunchAgents");
    fs::create_dir_all(&dir)?;
    let plist = dir.join(format!("{LAUNCHD_LABEL}.plist"));
    let domain = format!("gui/{}", gateway_command_output("id", &["-u"])?);
    let target = format!("{domain}/{LAUNCHD_LABEL}");
    if gateway_command_success("launchctl", &["print", &target]) {
        gateway_run_checked(
            "launchctl",
            &["bootout", &target],
            "stop existing gateway LaunchAgent",
        )?;
    }
    wait_for_launchd_absent(&target)?;
    fs::write(
        &plist,
        gateway_launchd_plist(exe, home, bind, require_auth, config_path),
    )?;
    gateway_run_retry(
        "launchctl",
        &[
            "bootstrap",
            &domain,
            plist.to_str().context("gateway plist path")?,
        ],
        "register gateway LaunchAgent",
    )?;
    gateway_run_checked(
        "launchctl",
        &["kickstart", "-k", &target],
        "start gateway LaunchAgent",
    )
}

#[cfg(target_os = "linux")]
pub(super) fn install_gateway_service(
    exe: &Path,
    home: &Path,
    bind: &str,
    require_auth: bool,
    config_path: Option<&Path>,
) -> Result<()> {
    let dir = home.join(".config/systemd/user");
    fs::create_dir_all(&dir)?;
    if gateway_command_success("systemctl", &["--user", "is-active", SYSTEMD_UNIT]) {
        gateway_run_checked(
            "systemctl",
            &["--user", "stop", SYSTEMD_UNIT],
            "stop existing gateway user service",
        )?;
    }
    fs::write(
        dir.join(SYSTEMD_UNIT),
        gateway_systemd_unit(exe, bind, require_auth, config_path),
    )?;
    gateway_run_checked(
        "systemctl",
        &["--user", "daemon-reload"],
        "reload gateway user service",
    )?;
    gateway_run_checked(
        "systemctl",
        &["--user", "enable", "--now", SYSTEMD_UNIT],
        "enable gateway user service",
    )
}

#[cfg(target_os = "macos")]
pub(super) fn uninstall_gateway_service() -> Result<()> {
    let home = gateway_home()?;
    let plist = home.join(format!("Library/LaunchAgents/{LAUNCHD_LABEL}.plist"));
    let domain = format!("gui/{}", gateway_command_output("id", &["-u"])?);
    let target = format!("{domain}/{LAUNCHD_LABEL}");
    if gateway_command_success("launchctl", &["print", &target]) {
        gateway_run_checked(
            "launchctl",
            &["bootout", &target],
            "stop gateway LaunchAgent",
        )?;
        wait_for_launchd_absent(&target)?;
    }
    if plist.exists() {
        fs::remove_file(plist)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn uninstall_gateway_service() -> Result<()> {
    let unit = gateway_home()?.join(format!(".config/systemd/user/{SYSTEMD_UNIT}"));
    if unit.exists() {
        gateway_run_checked(
            "systemctl",
            &["--user", "disable", "--now", SYSTEMD_UNIT],
            "disable gateway user service",
        )?;
        fs::remove_file(unit)?;
    }
    gateway_run_checked(
        "systemctl",
        &["--user", "daemon-reload"],
        "reload gateway user service",
    )
}

#[cfg(target_os = "macos")]
pub(super) fn managed_gateway_status() -> String {
    managed_gateway_pid()
        .and_then(process_command)
        .filter(|command| gateway_command_matches_any(command))
        .map_or_else(
            || "inactive or unhealthy".to_string(),
            |_| "active (launchd)".to_string(),
        )
}

#[cfg(target_os = "linux")]
pub(super) fn managed_gateway_status() -> String {
    managed_gateway_pid()
        .and_then(process_command)
        .filter(|command| gateway_command_matches_any(command))
        .map_or_else(
            || "inactive or unhealthy".to_string(),
            |_| "active (systemd user)".to_string(),
        )
}

#[cfg(target_os = "macos")]
pub(super) fn managed_gateway_pid() -> Option<i32> {
    let uid = gateway_command_output("id", &["-u"]).ok()?;
    let output = gateway_command_output(
        "launchctl",
        &["print", &format!("gui/{uid}/{LAUNCHD_LABEL}")],
    )
    .ok()?;
    parse_launchd_pid(&output)
}

#[cfg(target_os = "linux")]
pub(super) fn managed_gateway_pid() -> Option<i32> {
    let active = gateway_command_output(
        "systemctl",
        &[
            "--user",
            "show",
            "-p",
            "ActiveState",
            "--value",
            SYSTEMD_UNIT,
        ],
    )
    .ok()?;
    if active != "active" {
        return None;
    }
    gateway_command_output(
        "systemctl",
        &["--user", "show", "-p", "MainPID", "--value", SYSTEMD_UNIT],
    )
    .ok()?
    .parse::<i32>()
    .ok()
    .filter(|pid| *pid > 0)
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn parse_launchd_pid(output: &str) -> Option<i32> {
    let running = output.lines().any(|line| line.trim() == "state = running");
    if !running {
        return None;
    }
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("pid = ")?
            .parse::<i32>()
            .ok()
            .filter(|pid| *pid > 0)
    })
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn gateway_launchd_plist(
    exe: &Path,
    home: &Path,
    bind: &str,
    require_auth: bool,
    config_path: Option<&Path>,
) -> String {
    let auth_arg = if require_auth {
        "<string>--require-auth</string>"
    } else {
        ""
    };
    let config_env = config_path.map_or_else(String::new, |path| {
        format!(
            "<key>HELIXIR_CONFIG</key><string>{}</string>",
            gateway_xml_escape(&path.display().to_string())
        )
    });
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{LAUNCHD_LABEL}</string>
<key>ProgramArguments</key><array><string>{}</string><string>gateway</string><string>run</string><string>--bind</string><string>{}</string>{auth_arg}</array>
<key>EnvironmentVariables</key><dict><key>HOME</key><string>{}</string>{config_env}<key>PATH</key><string>/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin</string></dict>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
<key>ThrottleInterval</key><integer>2</integer>
<key>StandardOutPath</key><string>{}/.helixir/gateway.out.log</string>
<key>StandardErrorPath</key><string>{}/.helixir/gateway.err.log</string>
</dict></plist>
"#,
        gateway_xml_escape(&exe.display().to_string()),
        gateway_xml_escape(bind),
        gateway_xml_escape(&home.display().to_string()),
        gateway_xml_escape(&home.display().to_string()),
        gateway_xml_escape(&home.display().to_string())
    )
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn gateway_systemd_unit(
    exe: &Path,
    bind: &str,
    require_auth: bool,
    config_path: Option<&Path>,
) -> String {
    let auth_arg = if require_auth { " --require-auth" } else { "" };
    let config_env = config_path.map_or_else(String::new, |path| {
        format!(
            "Environment=HELIXIR_CONFIG=\"{}\"\n",
            gateway_systemd_escape(&path.display().to_string())
        )
    });
    format!(
        "[Unit]\nDescription=Helixir shared HTTP MCP gateway\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\n{config_env}ExecStart=\"{}\" gateway run --bind \"{}\"{}\nRestart=always\nRestartSec=2\n\n[Install]\nWantedBy=default.target\n",
        gateway_systemd_escape(&exe.display().to_string()),
        gateway_systemd_escape(bind),
        auth_arg
    )
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn gateway_xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn gateway_systemd_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
