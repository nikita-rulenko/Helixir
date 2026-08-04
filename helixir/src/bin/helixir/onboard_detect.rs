use super::*;

pub(crate) async fn detect_onboard_state() -> helixir::installer::SystemState {
    use helixir::installer::{BackendState, ClientKind, OllamaState, SystemState};
    use std::collections::BTreeMap;

    let detected_port = detect_local_backend_tcp();
    let backend = match detected_port {
        Some(_) => BackendState::Local {
            healthy: true,
            // Schema compatibility is verified by the backend executor. Treat
            // it as unknown here so a future apply phase always protects data
            // before a schema-affecting transition.
            schema_compatible: false,
        },
        None => BackendState::Missing,
    };

    let (ollama_installed, ollama_running, models) = detect_ollama().await;
    let rbac = match detected_port {
        Some(port) => detect_rbac_install_state(port).await,
        None => helixir::installer::rbac::RbacInstallState::default(),
    };
    let mut clients = BTreeMap::new();
    if client_available(ClientKind::ClaudeCode) {
        clients.insert(ClientKind::ClaudeCode, false);
    }
    if client_available(ClientKind::Codex) {
        clients.insert(ClientKind::Codex, false);
    }
    if client_available(ClientKind::Cursor) {
        clients.insert(ClientKind::Cursor, false);
    }

    SystemState {
        backend,
        ollama: OllamaState {
            installed: ollama_installed,
            running: ollama_running,
            models,
        },
        nli_installed: onboard_nli_installed(),
        central_config_matches: false,
        client_registered: clients,
        rbac,
    }
}

async fn detect_rbac_install_state(port: u16) -> helixir::installer::rbac::RbacInstallState {
    use helixir::core::RbacManager;
    use helixir::installer::rbac::RbacInstallState;
    use std::sync::Arc;

    let host = std::env::var("HELIX_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let Ok(db) = helixir::db::HelixClient::new(&host, port) else {
        return RbacInstallState::default();
    };
    let manager = RbacManager::new(Arc::new(db));
    helixir::installer::rbac::inspect(&manager)
        .await
        .unwrap_or_default()
}

/// Lightweight backend discovery for onboarding. Using a TCP connect here is
/// intentional: constructing the full reqwest/Helix client consults macOS
/// system proxy state and makes a supposedly read-only plan depend on that
/// platform service. Schema and health verification belong to the executor.
pub(crate) fn detect_local_backend_tcp() -> Option<u16> {
    use std::net::{TcpStream, ToSocketAddrs};

    let host = std::env::var("HELIX_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    for port in [
        std::env::var("HELIX_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok()),
        Some(helixir::DEFAULT_HELIX_PORT),
        Some(6970),
    ]
    .into_iter()
    .flatten()
    {
        let address = (host.as_str(), port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut addresses| addresses.next());
        if let Some(address) = address
            && TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok()
        {
            return Some(port);
        }
    }
    None
}

pub(crate) async fn detect_ollama() -> (bool, bool, std::collections::BTreeSet<String>) {
    use std::collections::BTreeSet;

    let home = std::env::var_os("HOME").map(PathBuf::from);
    let Some(binary) = helixir::installer::models::OllamaAdapter::resolve_binary(home.as_deref())
    else {
        return (false, false, BTreeSet::new());
    };
    let installed = Command::new(binary)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !installed {
        return (false, false, BTreeSet::new());
    }

    let models =
        helixir::installer::models::OllamaAdapter::list_api(helixir::DEFAULT_OLLAMA_URL).await;
    let running = models.is_ok();
    let models = models.unwrap_or_default();
    (true, running, models)
}

pub(crate) fn client_available(kind: helixir::installer::ClientKind) -> bool {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let command_available = |name: &str| {
        Command::new("sh")
            .args(["-c", &format!("command -v {name}")])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    };
    match kind {
        helixir::installer::ClientKind::ClaudeCode => {
            command_available("claude") || home.join(".claude.json").exists()
        }
        helixir::installer::ClientKind::Codex => {
            command_available("codex")
                || home.join(".codex/config.toml").exists()
                || Path::new("/Applications/Codex.app/Contents/Resources/codex").exists()
        }
        helixir::installer::ClientKind::Cursor => {
            command_available("cursor") || home.join(".cursor").exists()
        }
    }
}

pub(crate) fn onboard_nli_installed() -> bool {
    helixir::llm::nli::status().installed && helixir::llm::nli::verify_readiness().is_ok()
}
