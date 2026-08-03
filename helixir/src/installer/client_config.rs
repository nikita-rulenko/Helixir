//! Safe, schema-aware merge helpers for JSON-based MCP client configs.
//!
//! File discovery, backup policy and native client CLIs belong to platform
//! adapters. This module owns the pure document transformation so malformed
//! client configuration is rejected before any write is attempted.

use serde_json::Value;
use thiserror::Error;

/// Result of merging one MCP server entry into a client document.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeResult {
    /// Complete updated client configuration.
    pub document: Value,
    /// Whether the requested entry differs from the existing document.
    pub changed: bool,
}

/// Structural problems that make an automatic client-config write unsafe.
#[derive(Debug, Error)]
pub enum ClientConfigError {
    /// Existing bytes are not valid JSON.
    #[error("existing client config is not valid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    /// The client config root is not an object.
    #[error("existing client config root must be a JSON object")]
    RootNotObject,
    /// `mcpServers` exists but has an incompatible type.
    #[error("existing mcpServers value must be a JSON object")]
    ServersNotObject,
}

/// Merge `server_entry` under `mcpServers.server_name` without losing any
/// unrelated keys or servers.
///
/// Passing `None` creates a new object. Invalid or structurally incompatible
/// existing JSON returns an error; it is never silently replaced with `{}`.
pub fn merge_mcp_server(
    existing: Option<&str>,
    server_name: &str,
    server_entry: &Value,
) -> Result<MergeResult, ClientConfigError> {
    let mut root = match existing {
        Some(raw) => serde_json::from_str::<Value>(raw)?,
        None => Value::Object(serde_json::Map::new()),
    };
    let root_object = root
        .as_object_mut()
        .ok_or(ClientConfigError::RootNotObject)?;
    let servers = root_object
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let servers_object = servers
        .as_object_mut()
        .ok_or(ClientConfigError::ServersNotObject)?;
    let changed = servers_object.get(server_name) != Some(server_entry);
    if changed {
        servers_object.insert(server_name.to_string(), server_entry.clone());
    }
    Ok(MergeResult {
        document: root,
        changed,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn merge_preserves_unrelated_keys_and_servers() {
        let existing = r#"{
            "theme": "dark",
            "mcpServers": {"other": {"command": "other-mcp"}}
        }"#;
        let entry = json!({"command": "/stable/helixir-mcp"});

        let merged = merge_mcp_server(Some(existing), "helixir-local", &entry).unwrap();

        assert!(merged.changed);
        assert_eq!(merged.document["theme"], "dark");
        assert_eq!(
            merged.document["mcpServers"]["other"]["command"],
            "other-mcp"
        );
        assert_eq!(merged.document["mcpServers"]["helixir-local"], entry);
    }

    #[test]
    fn identical_entry_is_an_idempotent_noop() {
        let entry = json!({"command": "/stable/helixir-mcp"});
        let existing = json!({"mcpServers": {"helixir-local": entry}}).to_string();

        let merged = merge_mcp_server(Some(&existing), "helixir-local", &entry).unwrap();

        assert!(!merged.changed);
    }

    #[test]
    fn malformed_json_is_rejected_instead_of_replaced() {
        let error = merge_mcp_server(Some("{broken"), "helixir-local", &json!({}))
            .expect_err("invalid JSON must abort");

        assert!(matches!(error, ClientConfigError::InvalidJson(_)));
    }

    #[test]
    fn incompatible_shapes_are_rejected() {
        assert!(matches!(
            merge_mcp_server(Some("[]"), "helixir-local", &json!({})),
            Err(ClientConfigError::RootNotObject)
        ));
        assert!(matches!(
            merge_mcp_server(Some(r#"{"mcpServers": []}"#), "helixir-local", &json!({})),
            Err(ClientConfigError::ServersNotObject)
        ));
    }
}
