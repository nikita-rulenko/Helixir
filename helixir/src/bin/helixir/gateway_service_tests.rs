use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn service_definitions_are_reboot_safe_and_shell_free() {
    let exe = Path::new("/opt/helixir/current/helixir");
    let plist = gateway_launchd_plist(
        exe,
        Path::new("/Users/operator"),
        "127.0.0.1:8765",
        true,
        Some(Path::new("/Users/operator/.helixir/helixir.toml")),
    );
    assert!(plist.contains("<key>RunAtLoad</key><true/>"));
    assert!(plist.contains("<key>KeepAlive</key><true/>"));
    assert!(plist.contains("<string>--require-auth</string>"));
    assert!(plist.contains("<key>HELIXIR_CONFIG</key>"));
    assert!(!plist.contains("sh -c"));

    let unit = gateway_systemd_unit(
        exe,
        "127.0.0.1:8765",
        false,
        Some(Path::new("/home/operator/.helixir/helixir.toml")),
    );
    assert!(unit.contains("Restart=always"));
    assert!(unit.contains("gateway run --bind \"127.0.0.1:8765\""));
    assert!(unit.contains("Environment=HELIXIR_CONFIG="));
    assert!(!unit.contains("--require-auth"));
}

#[test]
fn service_values_are_escaped() {
    assert_eq!(
        gateway_xml_escape("a&<b>\"'"),
        "a&amp;&lt;b&gt;&quot;&apos;"
    );
    assert_eq!(gateway_systemd_escape("a\\b\"c"), "a\\\\b\\\"c");
}

#[test]
fn persistent_auth_detection_never_needs_the_token_value() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("helixir-gateway-config-{stamp}.toml"));
    fs::write(&path, "[gateway]\nauth_token = \"test-only\"\n").unwrap();
    assert!(persistent_gateway_token_exists(&path).unwrap());
    fs::write(&path, "[gateway]\nauth_token = \"\"\n").unwrap();
    assert!(!persistent_gateway_token_exists(&path).unwrap());
    fs::remove_file(path).unwrap();
}

#[test]
fn legacy_pid_must_still_belong_to_the_recorded_gateway() {
    let state = serde_json::json!({
        "pid": 42,
        "bind": "127.0.0.1:8765",
        "auth_required": false,
    });
    assert!(legacy_gateway_command_matches(
        "/opt/helixir/helixir gateway run --bind 127.0.0.1:8765",
        &state
    ));
    assert!(!legacy_gateway_command_matches("/bin/sleep 99", &state));
    assert!(!legacy_gateway_command_matches(
        "/opt/helixir/helixir gateway run --bind 127.0.0.1:9999",
        &state
    ));
}

#[test]
fn status_requires_running_service_pid_and_gateway_command() {
    assert!(managed_gateway_status_is_healthy("active (launchd)"));
    assert!(managed_gateway_status_is_healthy("active (systemd user)"));
    assert!(!managed_gateway_status_is_healthy("inactive or unhealthy"));

    let running = "state = running\npid = 4242\n";
    assert_eq!(parse_launchd_pid(running), Some(4242));
    assert_eq!(parse_launchd_pid("state = waiting\npid = 4242\n"), None);
    assert_eq!(parse_launchd_pid("state = running\n"), None);

    let exe = Path::new("/opt/helixir/current/helixir");
    assert!(gateway_command_matches_exact(
        "/opt/helixir/current/helixir gateway run --bind 127.0.0.1:8765",
        exe,
        "127.0.0.1:8765",
        false,
    ));
    assert!(!gateway_command_matches_exact(
        "/bin/sleep 8765",
        exe,
        "127.0.0.1:8765",
        false,
    ));
    assert!(gateway_command_matches_any(
        "/opt/helixir/current/helixir gateway run --bind 127.0.0.1:8765"
    ));
    assert!(!gateway_command_matches_any("/bin/sleep 8765"));
}
