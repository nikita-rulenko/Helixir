use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_path(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "helixir-{name}-{}-{stamp}.json",
        std::process::id()
    ))
}

#[test]
fn native_commands_use_client_owned_cli_and_no_env() {
    let server = StdioServer::new("/stable/helixir-mcp");
    let claude = native_add_command(ClientKind::ClaudeCode, "helixir-local", &server);
    assert_eq!(
        claude.argv(),
        vec![
            "claude",
            "mcp",
            "add",
            "--scope",
            "user",
            "helixir-local",
            "--",
            "/stable/helixir-mcp"
        ]
    );
    let codex = native_add_command(ClientKind::Codex, "helixir-local", &server);
    assert_eq!(
        codex.argv(),
        vec![
            "codex",
            "mcp",
            "add",
            "helixir-local",
            "--",
            "/stable/helixir-mcp"
        ]
    );
}

#[test]
fn native_http_commands_use_one_shared_gateway() {
    let server = HttpServer::new("http://127.0.0.1:6972/mcp");
    assert_eq!(
        native_add_http_command(ClientKind::Codex, "helixir-local", &server).argv(),
        vec![
            "codex",
            "mcp",
            "add",
            "helixir-local",
            "--url",
            "http://127.0.0.1:6972/mcp"
        ]
    );
    assert_eq!(
        native_add_http_command(ClientKind::ClaudeCode, "helixir-local", &server).argv(),
        vec![
            "claude",
            "mcp",
            "add",
            "--transport",
            "http",
            "--scope",
            "user",
            "helixir-local",
            "http://127.0.0.1:6972/mcp"
        ]
    );
    assert_eq!(server.json_entry()["type"], "http");
}

#[test]
fn actor_environment_is_rendered_without_provider_secrets() {
    let server = StdioServer::new("/stable/helixir-mcp").with_env("HELIXIR_RBAC_ACTOR", "codex");
    let codex = native_add_command(ClientKind::Codex, "helixir-local", &server);
    assert_eq!(
        codex.argv(),
        vec![
            "codex",
            "mcp",
            "add",
            "helixir-local",
            "--env",
            "HELIXIR_RBAC_ACTOR=codex",
            "--",
            "/stable/helixir-mcp"
        ]
    );
    assert_eq!(server.json_entry()["env"]["HELIXIR_RBAC_ACTOR"], "codex");
    assert!(!server.json_entry().to_string().contains("API_KEY"));
}

#[test]
fn json_registration_is_idempotent_and_backed_up() {
    let path = unique_temp_path("cursor");
    fs::write(
        &path,
        r#"{"theme":"dark","mcpServers":{"other":{"command":"x"}}}"#,
    )
    .unwrap();
    let server = StdioServer::new("/stable/helixir-mcp");
    let first = register_json_client(&path, "helixir-local", &server).unwrap();
    assert!(first.changed);
    assert!(first.backup.is_some());
    assert_eq!(first.document["theme"], "dark");
    let second = register_json_client(&path, "helixir-local", &server).unwrap();
    assert!(!second.changed);
    let _ = fs::remove_file(path);
    if let Some(backup) = first.backup {
        let _ = fs::remove_file(backup);
    }
}

#[test]
fn json_gateway_registration_is_http_and_secret_free() {
    let path = unique_temp_path("cursor-gateway");
    let server = McpServer::Http(HttpServer::new("http://127.0.0.1:8765/mcp"));
    let first = register_json_server(&path, "helixir-local", &server).unwrap();
    assert!(first.changed);
    assert_eq!(
        first.document["mcpServers"]["helixir-local"]["type"],
        "http"
    );
    assert_eq!(
        first.document["mcpServers"]["helixir-local"]["url"],
        "http://127.0.0.1:8765/mcp"
    );
    assert!(env_is_empty(&first.document["mcpServers"]["helixir-local"]));
    let second = register_json_server(&path, "helixir-local", &server).unwrap();
    assert!(!second.changed);
    let _ = fs::remove_file(path);
}

#[test]
fn malformed_json_is_never_replaced() {
    let path = unique_temp_path("malformed");
    fs::write(&path, "{broken").unwrap();
    let before = fs::read(&path).unwrap();
    let error = register_json_client(&path, "helixir-local", &StdioServer::new("x"))
        .expect_err("malformed config must abort");
    assert!(matches!(error, ClientAdapterError::Config(_)));
    assert_eq!(fs::read(&path).unwrap(), before);
    let _ = fs::remove_file(path);
}

#[test]
fn minimal_entry_drops_provider_environment() {
    let entry = serde_json::json!({
        "command": "/stable/helixir-mcp",
        "args": ["--stdio"],
        "env": {"HELIX_LLM_API_KEY": "secret"}
    });
    let minimal = minimal_entry(&entry);
    assert!(minimal.get("env").is_none());
    assert!(env_is_empty(&minimal));
    assert_eq!(minimal["command"], "/stable/helixir-mcp");
}
