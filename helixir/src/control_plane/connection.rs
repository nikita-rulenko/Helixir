//! Safe projection of the MCP gateway coordinates used by remote clients.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use super::dto::{ApiProblem, GatewayConnectionProjection};
use super::server::{AppState, require_host_operations, supervisor_error};

type Problem = (StatusCode, Json<ApiProblem>);

pub(super) async fn gateway(
    State(state): State<AppState>,
) -> Result<Json<GatewayConnectionProjection>, Problem> {
    let settings = match require_host_operations(&state)? {
        Some(supervisor) => supervisor.settings().await.map_err(supervisor_error)?,
        None => crate::installer::settings::load(),
    };
    Ok(Json(project(&settings.gateway)))
}

fn project(settings: &crate::installer::settings::GatewaySettings) -> GatewayConnectionProjection {
    let advertised = !settings.public_url.trim().is_empty();
    let candidate = if advertised {
        settings.public_url.trim()
    } else {
        settings.bind.trim()
    };
    let client_url = normalize_gateway_url(candidate);
    let shareable = reqwest::Url::parse(&client_url)
        .ok()
        .and_then(|url| url.host_str().map(is_shareable_host))
        .unwrap_or(false);
    let warning = (!shareable).then(|| {
        "Set gateway.public_url (or HELIXIR_GATEWAY_PUBLIC_URL) to a network-reachable host before sharing this endpoint."
            .to_string()
    });
    GatewayConnectionProjection {
        bind: settings.bind.clone(),
        client_url,
        advertised,
        shareable,
        auth_enabled: settings.auth_enabled,
        warning,
    }
}

fn normalize_gateway_url(value: &str) -> String {
    let value = value.trim().trim_end_matches('/');
    let with_scheme = if value.starts_with("http://") || value.starts_with("https://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    if with_scheme.ends_with("/mcp") {
        with_scheme
    } else {
        format!("{with_scheme}/mcp")
    }
}

fn is_shareable_host(host: &str) -> bool {
    !matches!(host, "0.0.0.0" | "::" | "[::]" | "localhost") && !host.starts_with("127.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertised_gateway_is_normalized_for_client_handoff() {
        let projection = project(&crate::installer::settings::GatewaySettings {
            bind: "0.0.0.0:8765".into(),
            public_url: "https://memory.example.test".into(),
            auth_enabled: true,
        });
        assert_eq!(projection.client_url, "https://memory.example.test/mcp");
        assert!(projection.advertised);
        assert!(projection.shareable);
        assert!(projection.auth_enabled);
        assert!(projection.warning.is_none());
    }

    #[test]
    fn wildcard_bind_is_visible_but_never_claimed_as_shareable() {
        let projection = project(&crate::installer::settings::GatewaySettings {
            bind: "0.0.0.0:8765".into(),
            public_url: String::new(),
            auth_enabled: false,
        });
        assert_eq!(projection.client_url, "http://0.0.0.0:8765/mcp");
        assert!(!projection.advertised);
        assert!(!projection.shareable);
        assert!(projection.warning.is_some());
    }
}
