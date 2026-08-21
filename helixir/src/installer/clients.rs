//! Client-specific MCP registration primitives.
//!
//! Native clients own their configuration formats. Claude Code and Codex must
//! therefore be changed through their CLI, while Cursor is handled by a strict
//! JSON merge. This module builds commands and performs atomic JSON replacement
//! without ever accepting provider secrets as part of a client entry.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;
use thiserror::Error;

use super::ClientKind;
use super::client_config::{ClientConfigError, merge_mcp_server};

/// Minimal stdio server description shared by all client adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StdioServer {
    /// Absolute path (or a deliberately resolved PATH command) to the MCP binary.
    pub command: String,
    /// Arguments passed after the command.
    pub args: Vec<String>,
    /// Non-secret process environment, such as the stable RBAC principal id.
    pub env: BTreeMap<String, String>,
}

/// Streamable-HTTP MCP endpoint owned by one long-lived gateway process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpServer {
    /// Full MCP endpoint URL, normally `http://127.0.0.1:8765/mcp`.
    pub url: String,
}

impl HttpServer {
    /// Construct an HTTP server description.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }

    /// Render the portable entry used by JSON-configured MCP clients.
    #[must_use]
    pub fn json_entry(&self) -> Value {
        serde_json::json!({
            "type": "http",
            "url": self.url,
        })
    }
}

/// Transport-aware MCP registration requested from a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpServer {
    /// A child process connected through stdin/stdout.
    Stdio(StdioServer),
    /// A shared streamable-HTTP gateway.
    Http(HttpServer),
}

impl McpServer {
    /// Render the canonical portable registration shape.
    #[must_use]
    pub fn json_entry(&self) -> Value {
        match self {
            Self::Stdio(server) => server.json_entry(),
            Self::Http(server) => server.json_entry(),
        }
    }
}

impl From<StdioServer> for McpServer {
    fn from(value: StdioServer) -> Self {
        Self::Stdio(value)
    }
}

impl From<HttpServer> for McpServer {
    fn from(value: HttpServer) -> Self {
        Self::Http(value)
    }
}

impl StdioServer {
    /// Construct a server entry with no environment variables.
    #[must_use]
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
        }
    }

    /// Add one non-secret environment value to the MCP process description.
    #[must_use]
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Render the minimal JSON entry used by file-based MCP clients.
    #[must_use]
    pub fn json_entry(&self) -> Value {
        let mut entry = serde_json::Map::new();
        entry.insert("command".to_string(), Value::String(self.command.clone()));
        if !self.args.is_empty() {
            entry.insert(
                "args".to_string(),
                Value::Array(self.args.iter().cloned().map(Value::String).collect()),
            );
        }
        if !self.env.is_empty() {
            entry.insert(
                "env".to_string(),
                serde_json::to_value(&self.env).unwrap_or_default(),
            );
        }
        Value::Object(entry)
    }
}

/// Native CLI invocation for a client adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeCommand {
    /// Executable to invoke (`claude` or `codex`).
    pub executable: String,
    /// Arguments, excluding the executable itself.
    pub args: Vec<String>,
}

impl NativeCommand {
    /// Return a shell-free argv vector suitable for `std::process::Command`.
    #[must_use]
    pub fn argv(&self) -> Vec<String> {
        std::iter::once(self.executable.clone())
            .chain(self.args.iter().cloned())
            .collect()
    }
}

/// Build the native registration command for Claude Code or Codex.
///
/// The command contains only the server name and stdio command. Provider keys,
/// model settings and other runtime configuration deliberately do not appear.
#[must_use]
pub fn native_add_command(
    client: ClientKind,
    server_name: &str,
    server: &StdioServer,
) -> NativeCommand {
    let mut args = Vec::new();
    match client {
        ClientKind::ClaudeCode => {
            args.extend(
                ["mcp", "add", "--scope", "user"]
                    .into_iter()
                    .map(str::to_string),
            );
            args.push(server_name.to_string());
        }
        ClientKind::Codex => {
            args.extend(["mcp", "add"].into_iter().map(str::to_string));
            args.push(server_name.to_string());
        }
        ClientKind::Cursor => {
            // Cursor is file-configured and should never reach the native path.
            args.push("unsupported-json-client".to_string());
        }
    }
    for (key, value) in &server.env {
        args.push("--env".to_string());
        args.push(format!("{key}={value}"));
    }
    args.push("--".to_string());
    args.push(server.command.clone());
    args.extend(server.args.iter().cloned());
    NativeCommand {
        executable: match client {
            ClientKind::ClaudeCode => "claude".to_string(),
            ClientKind::Codex => "codex".to_string(),
            ClientKind::Cursor => "cursor".to_string(),
        },
        args,
    }
}

/// Build a native HTTP registration command for Claude Code or Codex.
///
/// HTTP registration prevents clients that retain abandoned stdio pipes from
/// accumulating sleeping `helixir-mcp` children: every client talks to the
/// same explicitly managed gateway process instead.
#[must_use]
pub fn native_add_http_command(
    client: ClientKind,
    server_name: &str,
    server: &HttpServer,
) -> NativeCommand {
    let args = match client {
        ClientKind::ClaudeCode => vec![
            "mcp".to_string(),
            "add".to_string(),
            "--transport".to_string(),
            "http".to_string(),
            "--scope".to_string(),
            "user".to_string(),
            server_name.to_string(),
            server.url.clone(),
        ],
        ClientKind::Codex => vec![
            "mcp".to_string(),
            "add".to_string(),
            server_name.to_string(),
            "--url".to_string(),
            server.url.clone(),
        ],
        ClientKind::Cursor => vec!["unsupported-json-client".to_string()],
    };
    NativeCommand {
        executable: match client {
            ClientKind::ClaudeCode => "claude".to_string(),
            ClientKind::Codex => "codex".to_string(),
            ClientKind::Cursor => "cursor".to_string(),
        },
        args,
    }
}

/// Build the client-owned CLI command for either supported transport.
#[must_use]
pub fn native_add_server_command(
    client: ClientKind,
    server_name: &str,
    server: &McpServer,
) -> NativeCommand {
    match server {
        McpServer::Stdio(server) => native_add_command(client, server_name, server),
        McpServer::Http(server) => native_add_http_command(client, server_name, server),
    }
}

/// Errors produced before a client config is mutated.
#[derive(Debug, Error)]
pub enum ClientAdapterError {
    /// Existing JSON could not be merged safely.
    #[error(transparent)]
    Config(#[from] ClientConfigError),
    /// Filesystem operation failed.
    #[error("filesystem operation failed: {0}")]
    Io(#[from] io::Error),
    /// JSON serialization failed.
    #[error("serialize client config: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Result of an idempotent Cursor-style JSON registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonRegistration {
    /// Complete document that was validated and, if needed, written.
    pub document: Value,
    /// Whether the requested server differed from the existing entry.
    pub changed: bool,
    /// Timestamped backup path, when an existing file was replaced.
    pub backup: Option<PathBuf>,
}

/// Merge and atomically write one MCP server entry for a JSON-configured client.
///
/// Invalid JSON or incompatible shapes return before any backup/write. Existing
/// files are backed up with a timestamp and the replacement is performed via a
/// sibling temporary file followed by rename.
pub fn register_json_client(
    path: &Path,
    server_name: &str,
    server: &StdioServer,
) -> Result<JsonRegistration, ClientAdapterError> {
    register_json_server(path, server_name, &McpServer::Stdio(server.clone()))
}

/// Merge and atomically write one transport-aware MCP server entry.
pub fn register_json_server(
    path: &Path,
    server_name: &str,
    server: &McpServer,
) -> Result<JsonRegistration, ClientAdapterError> {
    let existing = if path.exists() {
        Some(fs::read_to_string(path)?)
    } else {
        None
    };
    let merged = merge_mcp_server(existing.as_deref(), server_name, &server.json_entry())?;
    if !merged.changed {
        return Ok(JsonRegistration {
            document: merged.document,
            changed: false,
            backup: None,
        });
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S%.3f");
    let backup = if path.exists() {
        let backup = PathBuf::from(format!("{}.bak.{stamp}", path.display()));
        fs::copy(path, &backup)?;
        Some(backup)
    } else {
        None
    };
    let temporary = PathBuf::from(format!("{}.tmp.{}", path.display(), std::process::id()));
    let rendered = serde_json::to_vec_pretty(&merged.document)?;
    fs::write(&temporary, rendered)?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(ClientAdapterError::Io(error));
    }
    Ok(JsonRegistration {
        document: merged.document,
        changed: true,
        backup,
    })
}

/// Resolve a command from PATH without invoking a shell.
#[must_use]
pub fn resolve_command(command: &str) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.is_file().then(|| path.to_path_buf());
    }
    let paths = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&paths) {
        let candidate = directory.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Return the standard config path for file-configured clients.
#[must_use]
pub fn default_json_config_path(client: ClientKind, home: &Path) -> Option<PathBuf> {
    match client {
        ClientKind::Cursor => Some(home.join(".cursor/mcp.json")),
        ClientKind::ClaudeCode | ClientKind::Codex => None,
    }
}

/// Remove environment keys from a legacy MCP entry before migrating it.
///
/// This helper is intentionally conservative: it only keeps the command and
/// args fields, so a caller cannot accidentally carry API keys into a client.
#[must_use]
pub fn minimal_entry(entry: &Value) -> Value {
    let mut result = serde_json::Map::new();
    if let Some(command) = entry.get("command") {
        result.insert("command".to_string(), command.clone());
    }
    if let Some(args) = entry.get("args") {
        result.insert("args".to_string(), args.clone());
    }
    Value::Object(result)
}

/// Small helper for deterministic adapter tests and diagnostics.
#[must_use]
pub fn env_is_empty(entry: &Value) -> bool {
    entry
        .get("env")
        .and_then(Value::as_object)
        .map(|map| map.is_empty())
        .unwrap_or(true)
}

#[cfg(test)]
#[path = "clients_tests.rs"]
mod tests;
