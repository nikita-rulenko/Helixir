use super::*;

pub(crate) async fn detect_onboard_state() -> helixir::installer::SystemState {
    use helixir::installer::{ClientKind, OllamaState, SystemState};
    use std::collections::BTreeMap;

    let config = helixir::core::config::HelixirConfig::from_env();
    let mut backend = detect_backend_state(&config.host, config.port);
    let endpoint = backend_endpoint(&backend);

    if let Some((host, port)) = endpoint.as_ref()
        && backend_reachable(host, *port)
        && probe_backend_schema_contract(host, *port).await
    {
        mark_backend_schema_compatible(&mut backend);
    }

    let (ollama_installed, ollama_running, models) = detect_ollama().await;
    let rbac = match endpoint {
        Some((host, port)) if backend_reachable(&host, port) => {
            detect_rbac_install_state(&host, port).await
        }
        None => helixir::installer::rbac::RbacInstallState::default(),
        Some(_) => helixir::installer::rbac::RbacInstallState::default(),
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

async fn detect_rbac_install_state(
    host: &str,
    port: u16,
) -> helixir::installer::rbac::RbacInstallState {
    use helixir::core::RbacManager;
    use helixir::installer::rbac::RbacInstallState;
    use std::sync::Arc;

    let Ok(db) = helixir::db::HelixClient::new(host, port) else {
        return RbacInstallState::default();
    };
    let manager = RbacManager::new(Arc::new(db));
    helixir::installer::rbac::inspect(&manager)
        .await
        .unwrap_or_default()
}

fn backend_endpoint(state: &helixir::installer::BackendState) -> Option<(String, u16)> {
    use helixir::installer::BackendState;
    match state {
        BackendState::Missing => None,
        BackendState::ManagedLocal { host, port, .. }
        | BackendState::ExistingLocal { host, port, .. }
        | BackendState::Remote { host, port, .. } => Some((host.clone(), *port)),
    }
}

fn mark_backend_schema_compatible(state: &mut helixir::installer::BackendState) {
    use helixir::installer::BackendState;
    match state {
        BackendState::ManagedLocal {
            schema_compatible, ..
        }
        | BackendState::ExistingLocal {
            schema_compatible, ..
        }
        | BackendState::Remote {
            schema_compatible, ..
        } => *schema_compatible = true,
        BackendState::Missing => {}
    }
}

pub(crate) async fn probe_backend_schema_contract(host: &str, port: u16) -> bool {
    let Ok(db) = helixir::db::HelixClient::new(host, port) else {
        return false;
    };
    db.execute_query_no_retry::<serde_json::Value, _>(
        "getHelixirSchemaVersion",
        &serde_json::json!({}),
    )
    .await
    .is_ok_and(|value| schema_contract_matches(&value))
}

fn schema_contract_matches(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(value) => {
            value == helixir::installer::backend::SCHEMA_CONTRACT_VERSION
        }
        serde_json::Value::Array(values) => values.iter().any(schema_contract_matches),
        serde_json::Value::Object(values) => values.values().any(schema_contract_matches),
        _ => false,
    }
}

fn detect_backend_state(host: &str, port: u16) -> helixir::installer::BackendState {
    use helixir::installer::BackendState;

    let healthy = backend_reachable(host, port);
    let manifest = installed_backend_manifest();
    let schema_compatible = manifest.as_ref().is_some_and(|backend| {
        backend.host == host
            && backend.port == port
            && backend.helix_cli_version == helixir::installer::backend::HELIX_CLI_VERSION
            && helixir::installer::backend::schema_fingerprint(&schema_dir_for_install())
                .is_ok_and(|fingerprint| fingerprint == backend.schema_fingerprint)
    });
    let managed = manifest
        .filter(|backend| {
            backend.kind == "managed_local"
                && backend.host == host
                && backend.port == port
                && managed_container_matches(backend)
        })
        .or_else(|| discover_managed_container(host, port));
    if let Some(backend) = managed {
        return BackendState::ManagedLocal {
            host: host.to_string(),
            port,
            container: backend.container,
            volume: backend.volume,
            image: backend.image,
            healthy,
            schema_compatible,
        };
    }
    if !healthy {
        return BackendState::Missing;
    }
    if is_local_host(host) {
        BackendState::ExistingLocal {
            host: host.to_string(),
            port,
            healthy,
            schema_compatible,
        }
    } else {
        BackendState::Remote {
            host: host.to_string(),
            port,
            healthy,
            schema_compatible,
        }
    }
}

fn discover_managed_container(
    host: &str,
    port: u16,
) -> Option<helixir::installer::manifest::BackendManifest> {
    if !is_local_host(host) {
        return None;
    }
    let output = Command::new("docker")
        .args(["inspect", "helixdb"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let rows = serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout).ok()?;
    let row = rows.first()?;
    let image = row
        .pointer("/Config/Image")
        .and_then(serde_json::Value::as_str)?
        .to_string();
    let volume = row
        .get("Mounts")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .find(|mount| {
            mount.get("Destination").and_then(serde_json::Value::as_str) == Some("/data")
        })?
        .get("Name")
        .and_then(serde_json::Value::as_str)?
        .to_string();
    let published = row
        .pointer("/NetworkSettings/Ports")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|ports| {
            ports.values().any(|bindings| {
                bindings.as_array().is_some_and(|bindings| {
                    bindings.iter().any(|binding| {
                        binding
                            .get("HostPort")
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|value| value.parse::<u16>().ok() == Some(port))
                    })
                })
            })
        });
    if !published {
        return None;
    }
    Some(helixir::installer::manifest::BackendManifest {
        kind: "managed_local".to_string(),
        host: host.to_string(),
        port,
        container: "helixdb".to_string(),
        image,
        volume,
        ..Default::default()
    })
}

fn installed_backend_manifest() -> Option<helixir::installer::manifest::BackendManifest> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    helixir::installer::manifest::read(&home.join(".helixir/install.json"))
        .ok()
        .flatten()
        .map(|manifest| manifest.backend)
}

fn managed_container_matches(backend: &helixir::installer::manifest::BackendManifest) -> bool {
    let output = Command::new("docker")
        .args(["inspect", &backend.container])
        .output();
    let Ok(output) = output else { return false };
    if !output.status.success() {
        return false;
    }
    let Ok(rows) = serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout) else {
        return false;
    };
    rows.first().is_some_and(|row| {
        let image_matches = row
            .pointer("/Config/Image")
            .and_then(serde_json::Value::as_str)
            == Some(backend.image.as_str());
        let volume_matches = row
            .get("Mounts")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|mounts| {
                mounts.iter().any(|mount| {
                    mount.get("Destination").and_then(serde_json::Value::as_str) == Some("/data")
                        && mount.get("Name").and_then(serde_json::Value::as_str)
                            == Some(backend.volume.as_str())
                })
            });
        image_matches && volume_matches
    })
}

fn is_local_host(host: &str) -> bool {
    matches!(
        host.trim().to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1"
    )
}

/// Lightweight backend discovery for onboarding. Using a TCP connect here is
/// intentional: constructing the full reqwest/Helix client consults macOS
/// system proxy state and makes a supposedly read-only plan depend on that
/// platform service. Schema and health verification belong to the executor.
pub(crate) fn detect_local_backend_tcp() -> Option<u16> {
    let config = helixir::core::config::HelixirConfig::from_env();
    backend_reachable(&config.host, config.port).then_some(config.port)
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
        helixir::installer::ClientKind::ClaudeCode => command_available("claude"),
        helixir::installer::ClientKind::Codex => {
            command_available("codex")
                || Path::new("/Applications/ChatGPT.app/Contents/Resources/codex").exists()
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

#[cfg(test)]
mod schema_contract_tests {
    use super::*;

    #[test]
    fn schema_contract_accepts_wrapped_live_response_only() {
        assert!(schema_contract_matches(&serde_json::json!({
            "version": helixir::installer::backend::SCHEMA_CONTRACT_VERSION
        })));
        assert!(!schema_contract_matches(&serde_json::json!({
            "version": "legacy"
        })));
    }

    #[test]
    fn packaged_hql_reports_the_runtime_schema_contract() {
        let queries = include_str!("../../../schema/queries.hx");
        let expected = format!(
            "RETURN \"{}\"",
            helixir::installer::backend::SCHEMA_CONTRACT_VERSION
        );

        assert!(
            queries.contains(&expected),
            "queries.hx must report the runtime schema contract {expected}"
        );
    }
}
