use super::*;

#[test]
fn native_registration_comparison_ignores_client_owned_fields() {
    let normalized = normalized_registration(&serde_json::json!({
        "command": "/stable/helixir-mcp",
        "args": [],
        "env": {"HELIXIR_RBAC_ACTOR": "codex"},
        "enabled": true,
        "startup_timeout_sec": 15
    }));
    assert_eq!(
        normalized,
        helixir::installer::clients::StdioServer::new("/stable/helixir-mcp")
            .with_env("HELIXIR_RBAC_ACTOR", "codex")
            .json_entry()
    );
}

#[test]
fn conflict_summary_never_prints_environment_values() {
    let summary = safe_registration_summary(&serde_json::json!({
        "command": "helixir-mcp",
        "env": {"HELIX_LLM_API_KEY": "must-not-leak"}
    }));
    assert!(!summary.to_string().contains("must-not-leak"));
    assert_eq!(
        summary["env_keys"],
        serde_json::json!(["HELIX_LLM_API_KEY"])
    );
}

#[test]
fn non_interactive_conflict_fails_before_mutation() {
    let error = approve_registration_change(
        helixir::installer::ClientKind::Codex,
        Some(&serde_json::json!({"command": "old"})),
        &helixir::installer::clients::McpServer::Stdio(
            helixir::installer::clients::StdioServer::new("new"),
        ),
        false,
    )
    .expect_err("conflict must require approval");
    assert!(error.contains("explicit replacement approval"));
}

#[test]
fn reviewed_non_interactive_conflict_is_approved() {
    approve_registration_change(
        helixir::installer::ClientKind::Codex,
        Some(&serde_json::json!({"command": "old"})),
        &helixir::installer::clients::McpServer::Stdio(
            helixir::installer::clients::StdioServer::new("new"),
        ),
        true,
    )
    .expect("typed control-plane consent should approve replacement");
}

#[cfg(unix)]
#[test]
fn registration_comparison_accepts_equivalent_symlink_commands() {
    use std::os::unix::fs::symlink;

    let temp = std::env::temp_dir().join(format!(
        "helixir-registration-match-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&temp).expect("temporary directory");
    let binary = temp.join("helixir-mcp-v3");
    let stable = temp.join("helixir-mcp");
    std::fs::write(&binary, b"test binary").expect("binary fixture");
    symlink(&binary, &stable).expect("stable symlink");
    let existing = serde_json::json!({
        "command": stable,
        "env": {"HELIXIR_RBAC_ACTOR": "codex"}
    });
    let expected = serde_json::json!({
        "command": binary,
        "env": {"HELIXIR_RBAC_ACTOR": "codex"}
    });
    assert!(registrations_match(&existing, &expected));
    std::fs::remove_dir_all(&temp).expect("remove temporary directory");
}

#[test]
fn codex_http_registration_normalizes_implicit_transport_type() {
    let existing = serde_json::json!({"url": "http://127.0.0.1:6972/mcp"});
    let expected =
        helixir::installer::clients::HttpServer::new("http://127.0.0.1:6972/mcp").json_entry();
    assert!(registrations_match(&existing, &expected));
    assert!(valid_http_registration(&existing));
}
