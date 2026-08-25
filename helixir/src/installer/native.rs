//! Native, read-only machine discovery shared by every installer frontend.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use super::manifest::BackendManifest;
use super::{BackendState, ClientKind, OllamaState, SystemDetector, SystemState};
use crate::core::{HelixirConfig, RbacManager};

/// Host detector used by both the CLI and the browser control plane.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeSystemDetector;

impl NativeSystemDetector {
    /// Create the platform detector. Detection is read-only and idempotent.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SystemDetector for NativeSystemDetector {
    async fn detect(&self) -> Result<SystemState, String> {
        Ok(detect_system_state().await)
    }
}

/// Inspect the current host without applying installation changes.
pub async fn detect_system_state() -> SystemState {
    let config = HelixirConfig::from_env();
    let mut backend = detect_backend_state(&config.host, config.port);
    let endpoint = backend_endpoint(&backend);

    if let Some((host, port)) = endpoint.as_ref()
        && backend_reachable(host, *port)
        && probe_backend_schema_contract(host, *port).await
    {
        mark_backend_schema_compatible(&mut backend);
    }

    let (installed, running, models) = detect_ollama().await;
    let rbac = match endpoint {
        Some((host, port)) if backend_reachable(&host, port) => {
            detect_rbac_install_state(&host, port).await
        }
        _ => super::rbac::RbacInstallState::default(),
    };
    let client_registered = [
        ClientKind::ClaudeCode,
        ClientKind::Codex,
        ClientKind::Cursor,
    ]
    .into_iter()
    .filter(|client| client_available(*client))
    .map(|client| (client, false))
    .collect::<BTreeMap<_, _>>();

    SystemState {
        backend,
        ollama: OllamaState {
            installed,
            running,
            models,
        },
        nli_installed: nli_installed(),
        central_config_matches: false,
        client_registered,
        rbac,
    }
}

async fn detect_rbac_install_state(host: &str, port: u16) -> super::rbac::RbacInstallState {
    let Ok(db) = crate::db::HelixClient::new(host, port) else {
        return super::rbac::RbacInstallState::default();
    };
    super::rbac::inspect(&RbacManager::new(Arc::new(db)))
        .await
        .unwrap_or_default()
}

fn backend_endpoint(state: &BackendState) -> Option<(String, u16)> {
    match state {
        BackendState::Missing => None,
        BackendState::ManagedLocal { host, port, .. }
        | BackendState::ExistingLocal { host, port, .. }
        | BackendState::Remote { host, port, .. } => Some((host.clone(), *port)),
    }
}

fn mark_backend_schema_compatible(state: &mut BackendState) {
    match state {
        // Managed-local compatibility includes the maintained engine revision
        // and container identity checked during discovery. A matching HQL
        // string alone must not bless an unpatched upstream v2.3.5 image.
        BackendState::ManagedLocal { .. } => {}
        BackendState::ExistingLocal {
            schema_compatible, ..
        }
        | BackendState::Remote {
            schema_compatible, ..
        } => *schema_compatible = true,
        BackendState::Missing => {}
    }
}

/// Verify that a reachable backend exposes the schema contract for this build.
pub async fn probe_backend_schema_contract(host: &str, port: u16) -> bool {
    let Ok(db) = crate::db::HelixClient::new(host, port) else {
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
        serde_json::Value::String(value) => value == super::backend::SCHEMA_CONTRACT_VERSION,
        serde_json::Value::Array(values) => values.iter().any(schema_contract_matches),
        serde_json::Value::Object(values) => values.values().any(schema_contract_matches),
        _ => false,
    }
}

fn detect_backend_state(host: &str, port: u16) -> BackendState {
    let healthy = backend_reachable(host, port);
    let manifest = installed_backend_manifest();
    let schema_compatible = manifest.as_ref().is_some_and(|backend| {
        backend.host == host
            && backend.port == port
            && backend.helix_cli_version == super::backend::HELIX_CLI_VERSION
            && backend.engine_revision == super::backend::ENGINE_REVISION
            && super::backend::schema_fingerprint(&schema_dir_for_install())
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

fn discover_managed_container(host: &str, port: u16) -> Option<BackendManifest> {
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
    let image = row.pointer("/Config/Image")?.as_str()?.to_string();
    let engine_revision = row
        .pointer("/Config/Labels/io.helixir.engine-revision")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let schema_fingerprint = row
        .pointer("/Config/Labels/io.helixir.schema-fingerprint")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let volume = row
        .get("Mounts")?
        .as_array()?
        .iter()
        .find(|mount| {
            mount.get("Destination").and_then(serde_json::Value::as_str) == Some("/data")
        })?
        .get("Name")?
        .as_str()?
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
                            .and_then(|value| value.parse::<u16>().ok())
                            == Some(port)
                    })
                })
            })
        });
    published.then_some(BackendManifest {
        kind: "managed_local".to_string(),
        host: host.to_string(),
        port,
        container: "helixdb".to_string(),
        image,
        volume,
        engine_revision,
        schema_fingerprint,
        ..Default::default()
    })
}

fn installed_backend_manifest() -> Option<BackendManifest> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    super::manifest::read(&home.join(".helixir/install.json"))
        .ok()
        .flatten()
        .map(|manifest| manifest.backend)
}

fn managed_container_matches(backend: &BackendManifest) -> bool {
    let Ok(output) = Command::new("docker")
        .args(["inspect", &backend.container])
        .output()
    else {
        return false;
    };
    let Ok(rows) = serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout) else {
        return false;
    };
    output.status.success()
        && rows.first().is_some_and(|row| {
            let image_matches = row
                .pointer("/Config/Image")
                .and_then(serde_json::Value::as_str)
                == Some(backend.image.as_str());
            let volume_matches = row
                .get("Mounts")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|mounts| {
                    mounts.iter().any(|mount| {
                        mount.get("Destination").and_then(serde_json::Value::as_str)
                            == Some("/data")
                            && mount.get("Name").and_then(serde_json::Value::as_str)
                                == Some(backend.volume.as_str())
                    })
                });
            let engine_matches = row
                .pointer("/Config/Labels/io.helixir.engine-revision")
                .and_then(serde_json::Value::as_str)
                == Some(backend.engine_revision.as_str());
            let schema_matches = row
                .pointer("/Config/Labels/io.helixir.schema-fingerprint")
                .and_then(serde_json::Value::as_str)
                == Some(backend.schema_fingerprint.as_str());
            image_matches && volume_matches && engine_matches && schema_matches
        })
}

fn is_local_host(host: &str) -> bool {
    matches!(
        host.trim().to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1"
    )
}

/// Check whether the configured backend accepts TCP connections.
pub fn detect_local_backend_tcp() -> Option<u16> {
    let config = HelixirConfig::from_env();
    backend_reachable(&config.host, config.port).then_some(config.port)
}

/// Inspect the local Ollama binary, API, and installed model inventory.
pub async fn detect_ollama() -> (bool, bool, BTreeSet<String>) {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let Some(binary) = super::models::OllamaAdapter::resolve_binary(home.as_deref()) else {
        return (false, false, BTreeSet::new());
    };
    let installed = Command::new(binary)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if !installed {
        return (false, false, BTreeSet::new());
    }
    let models = super::models::OllamaAdapter::list_api(crate::DEFAULT_OLLAMA_URL).await;
    (true, models.is_ok(), models.unwrap_or_default())
}

/// Return whether a supported MCP client is installed or configured locally.
pub fn client_available(kind: ClientKind) -> bool {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    match kind {
        ClientKind::ClaudeCode => super::clients::resolve_command("claude").is_some(),
        ClientKind::Codex => {
            super::clients::resolve_command("codex").is_some()
                || Path::new("/Applications/ChatGPT.app/Contents/Resources/codex").exists()
                || Path::new("/Applications/Codex.app/Contents/Resources/codex").exists()
        }
        ClientKind::Cursor => {
            super::clients::resolve_command("cursor").is_some() || home.join(".cursor").exists()
        }
    }
}

/// Return whether the mandatory local NLI judge is installed and loadable.
pub fn nli_installed() -> bool {
    crate::llm::nli::status().installed && crate::llm::nli::verify_readiness().is_ok()
}

/// Lightweight TCP reachability used during read-only discovery.
pub fn backend_reachable(host: &str, port: u16) -> bool {
    (host, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.next())
        .and_then(|address| TcpStream::connect_timeout(&address, Duration::from_millis(500)).ok())
        .is_some()
}

/// Resolve the schema directory beside a release binary or in a source tree.
pub fn schema_dir_for_install() -> PathBuf {
    if let Ok(executable) = std::env::current_exe() {
        for parent in executable.ancestors().skip(1) {
            let candidate = parent.join("schema");
            if candidate.join("schema.hx").is_file() && candidate.join("queries.hx").is_file() {
                return candidate;
            }
        }
    }
    let current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for candidate in [current.join("schema"), current.join("helixir/schema")] {
        if candidate.join("schema.hx").is_file() && candidate.join("queries.hx").is_file() {
            return candidate;
        }
    }
    PathBuf::from("schema")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_contract_accepts_wrapped_live_response_only() {
        assert!(schema_contract_matches(&serde_json::json!({
            "version": super::super::backend::SCHEMA_CONTRACT_VERSION
        })));
        assert!(!schema_contract_matches(
            &serde_json::json!({"version": "legacy"})
        ));
    }

    #[test]
    fn packaged_hql_reports_the_runtime_schema_contract() {
        let queries = include_str!("../../schema/queries.hx");
        let expected = format!(
            "RETURN \"{}\"",
            super::super::backend::SCHEMA_CONTRACT_VERSION
        );
        assert!(queries.contains(&expected));
    }
}
