//! Minimal synchronous MCP streamable-HTTP client used during bootstrap and doctor.

use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

const MCP_SESSION_ID: &str = "mcp-session-id";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Enrollment {
    pub principal_id: String,
    pub group_id: String,
    pub roles: Vec<String>,
    pub created: bool,
}

pub struct McpClient {
    http: Client,
    gateway_url: String,
    session_id: String,
    token: Option<String>,
    server_version: String,
    next_id: u64,
}

impl McpClient {
    pub fn connect(gateway_url: &str, token: Option<String>) -> Result<Self> {
        let gateway_url = normalize_gateway_url(gateway_url)?;
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()?;
        let response = send(
            &http,
            &gateway_url,
            token.as_deref(),
            None,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "helixir-client", "version": env!("CARGO_PKG_VERSION")}
                }
            }),
        )?;
        let session_id = response
            .headers()
            .get(MCP_SESSION_ID)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("gateway initialize response omitted mcp-session-id"))?;
        let initialized = parse_rpc_response(response)?;
        let server_version = initialized
            .get("serverInfo")
            .and_then(|info| info.get("version"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("gateway initialize omitted serverInfo.version"))?
            .to_string();
        if !compatible_gateway_version(env!("CARGO_PKG_VERSION"), &server_version) {
            bail!(
                "gateway version {server_version} is incompatible with helixir-client {}; install matching major/minor versions",
                env!("CARGO_PKG_VERSION")
            );
        }
        send(
            &http,
            &gateway_url,
            token.as_deref(),
            Some(&session_id),
            &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )?;
        Ok(Self {
            http,
            gateway_url,
            session_id,
            token,
            server_version,
            next_id: 2,
        })
    }

    pub fn server_version(&self) -> &str {
        &self.server_version
    }

    pub fn tool_names(&mut self) -> Result<BTreeSet<String>> {
        let result = self.request("tools/list", json!({}))?;
        Ok(result
            .get("tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .map(str::to_owned)
            .collect())
    }

    pub fn enroll_client(&mut self, principal_id: &str) -> Result<Enrollment> {
        let result = self.request(
            "tools/call",
            json!({
                "name": "enroll_client",
                "arguments": {"actor_id": principal_id}
            }),
        )?;
        let text = result
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find_map(|item| item.get("text").and_then(Value::as_str))
            .ok_or_else(|| anyhow::anyhow!("enroll_client returned no text payload"))?;
        serde_json::from_str(text).context("parse enroll_client payload")
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let response = send(
            &self.http,
            &self.gateway_url,
            self.token.as_deref(),
            Some(&self.session_id),
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
        )?;
        parse_rpc_response(response)
    }
}

fn compatible_gateway_version(client: &str, server: &str) -> bool {
    fn major_minor(version: &str) -> Option<(u64, u64)> {
        let mut parts = version.trim_start_matches('v').split('.');
        Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
    }
    major_minor(client).is_some_and(|version| Some(version) == major_minor(server))
}

pub fn normalize_gateway_url(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("gateway is required (for example 10.0.0.12:8765)");
    }
    let candidate = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("http://{raw}")
    };
    let mut url = Url::parse(&candidate).context("parse gateway URL")?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("gateway URL must use http or https");
    }
    if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        bail!("gateway URL must contain a host and must not embed credentials");
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("gateway URL must not contain query parameters or a fragment");
    }
    if url.path() == "/" || url.path().is_empty() {
        url.set_path("/mcp");
    } else if url.path().trim_end_matches('/') != "/mcp" {
        bail!("gateway URL path must be /mcp");
    } else {
        url.set_path("/mcp");
    }
    Ok(url.to_string())
}

fn send(
    client: &Client,
    url: &str,
    token: Option<&str>,
    session_id: Option<&str>,
    body: &Value,
) -> Result<Response> {
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Some(session_id) = session_id {
        headers.insert(MCP_SESSION_ID, HeaderValue::from_str(session_id)?);
    }
    if let Some(token) = token {
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))?,
        );
    }
    let response = client.post(url).headers(headers).json(body).send()?;
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().unwrap_or_default();
        bail!("gateway returned HTTP {status}: {detail}");
    }
    Ok(response)
}

fn parse_rpc_response(response: Response) -> Result<Value> {
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body = response.text()?;
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }
    let envelope: Value = if content_type.contains("text/event-stream") {
        let data = body
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .ok_or_else(|| anyhow::anyhow!("gateway returned an empty SSE event"))?;
        serde_json::from_str(data)?
    } else {
        serde_json::from_str(&body)?
    };
    if let Some(error) = envelope.get("error") {
        bail!("MCP error: {error}");
    }
    Ok(envelope.get("result").cloned().unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::{compatible_gateway_version, normalize_gateway_url};

    #[test]
    fn normalizes_host_port_and_rejects_unsafe_urls() {
        assert_eq!(
            normalize_gateway_url("10.0.0.2:8765").unwrap(),
            "http://10.0.0.2:8765/mcp"
        );
        assert_eq!(
            normalize_gateway_url("https://memory.example/mcp/").unwrap(),
            "https://memory.example/mcp"
        );
        for invalid in [
            "",
            "file:///tmp/socket",
            "http://u:p@host:8765",
            "http://host:8765/other",
        ] {
            assert!(
                normalize_gateway_url(invalid).is_err(),
                "accepted {invalid}"
            );
        }
    }

    #[test]
    fn gateway_requires_a_matching_release_line() {
        assert!(compatible_gateway_version("0.17.0", "0.17.4"));
        assert!(compatible_gateway_version("0.17.0", "v0.17.1"));
        assert!(!compatible_gateway_version("0.17.0", "0.16.9"));
        assert!(!compatible_gateway_version("0.17.0", "not-a-version"));
    }
}
