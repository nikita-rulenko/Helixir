//! Shared MCP stdio harness for e2e suites: spawns the real `helixir-mcp`
//! binary and speaks JSON-RPC the way Claude Desktop / Claude Code does.

#![allow(dead_code)] // each test crate uses a subset

pub mod golden;

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Instant;

use futures::FutureExt;
use serde_json::{Value, json};
use std::future::Future;
use std::panic::{AssertUnwindSafe, resume_unwind};

/// Global administrator used by the canonical live-E2E runner.
///
/// Ignored E2E tests must not silently fall back to the memory owner as the
/// authenticated principal: permanent RBAC treats those identities as
/// different things. The runner always sets this variable explicitly.
pub fn e2e_actor() -> String {
    std::env::var("HELIXIR_RBAC_ACTOR")
        .expect("HELIXIR_RBAC_ACTOR must name the disposable E2E global admin")
}

/// Concrete workspace used by ordinary live-E2E writes.
///
/// `default` is a reserved compatibility workspace, but the runner still
/// passes it explicitly so no fixture exercises the removed unscoped path.
pub fn e2e_group() -> String {
    std::env::var("HELIXIR_E2E_GROUP")
        .expect("HELIXIR_E2E_GROUP must name the disposable E2E workspace")
}

/// Always run asynchronous fixture cleanup, including after an assertion panic.
///
/// Cleanup reports every failed best-effort operation instead of stopping at
/// the first one. A successful test fails on cleanup drift; an already failing
/// test preserves its original panic after printing cleanup diagnostics.
pub async fn with_cleanup<Test, Cleanup, T>(test: Test, cleanup: Cleanup) -> T
where
    Test: Future<Output = T>,
    Cleanup: Future<Output = Vec<String>>,
{
    let outcome = AssertUnwindSafe(test).catch_unwind().await;
    let cleanup_errors = cleanup.await;
    match outcome {
        Ok(value) => {
            assert!(
                cleanup_errors.is_empty(),
                "E2E fixture cleanup failed: {}",
                cleanup_errors.join("; ")
            );
            value
        }
        Err(payload) => {
            if !cleanup_errors.is_empty() {
                eprintln!(
                    "E2E fixture cleanup also failed: {}",
                    cleanup_errors.join("; ")
                );
            }
            resume_unwind(payload)
        }
    }
}

/// Query the live HelixDB directly (bypassing the MCP layer) so a test can
/// assert the GROUND TRUTH of what was persisted — e.g. that reasoning edges
/// exist with the right `relation_type`. Uses `curl` to avoid an HTTP dep.
/// Host/port come from HELIX_HOST/HELIX_PORT (default localhost:6970).
pub fn db_query(query: &str, body: &Value) -> Value {
    let host = std::env::var("HELIX_HOST").unwrap_or_else(|_| "localhost".into());
    let port = std::env::var("HELIX_PORT").unwrap_or_else(|_| "6970".into());
    let url = format!("http://{host}:{port}/{query}");
    let out = Command::new("curl")
        .args([
            "-s",
            "-m",
            "15",
            "-X",
            "POST",
            &url,
            "-H",
            "Content-Type: application/json",
            "-d",
            &body.to_string(),
        ])
        .output()
        .unwrap_or_else(|e| panic!("curl {query} failed to spawn: {e}"));
    let text = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("db_query {query}: non-JSON response ({e}): {text}"))
}

/// Collect every outgoing typed edge `(relation_type)` for a memory straight
/// from HelixDB: the dedicated IMPLIES/BECAUSE buckets plus the generic
/// MEMORY_RELATION edges (SUPPORTS/RELATES_TO/PART_OF/IS_A) keyed by their
/// `relation_type` property.
pub fn db_edge_types_out(memory_id: &str) -> Vec<String> {
    let r = db_query(
        "getMemoryOutgoingRelations",
        &json!({ "memory_id": memory_id }),
    );
    let mut types = Vec::new();
    if r["implies_out"]
        .as_array()
        .map(|a| !a.is_empty())
        .unwrap_or(false)
    {
        for _ in r["implies_out"].as_array().unwrap() {
            types.push("IMPLIES".to_string());
        }
    }
    if let Some(arr) = r["because_out"].as_array() {
        for _ in arr {
            types.push("BECAUSE".to_string());
        }
    }
    if let Some(arr) = r["relations_out"].as_array() {
        for e in arr {
            if let Some(t) = e["relation_type"].as_str() {
                types.push(t.to_string());
            }
        }
    }
    types
}

pub struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    actor_id: String,
    group_id: String,
    /// Server-initiated notifications captured while waiting for responses.
    pub notifications: Vec<Value>,
}

impl McpClient {
    pub fn spawn() -> (Self, f64) {
        Self::spawn_with_env(&[])
    }

    /// Spawn a fresh `helixir-mcp` process (a distinct MCP consumer) with extra
    /// environment overrides — e.g. `HELIXIR_INGEST_BUFFER=1` for one consumer
    /// while another runs the sync path. Each call is a separate OS process
    /// against the shared HelixDB, which is the real multi-consumer topology.
    pub fn spawn_with_env(envs: &[(&str, &str)]) -> (Self, f64) {
        let t0 = Instant::now();
        let actor_id = envs
            .iter()
            .rev()
            .find_map(|(key, value)| (*key == "HELIXIR_RBAC_ACTOR").then(|| (*value).to_string()))
            .unwrap_or_else(e2e_actor);
        let group_id = envs
            .iter()
            .rev()
            .find_map(|(key, value)| (*key == "HELIXIR_E2E_GROUP").then(|| (*value).to_string()))
            .unwrap_or_else(e2e_group);
        let mut child = Command::new(env!("CARGO_BIN_EXE_helixir-mcp"))
            .env("HELIXIR_RBAC_ACTOR", &actor_id)
            .env("HELIXIR_E2E_GROUP", &group_id)
            .envs(envs.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn helixir-mcp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        let mut client = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
            actor_id,
            group_id,
            notifications: Vec::new(),
        };

        let _init = client.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "read-e2e", "version": "0.0.1"}
            }),
        );
        client.notify("notifications/initialized", json!({}));
        let boot_ms = t0.elapsed().as_secs_f64() * 1000.0;
        (client, boot_ms)
    }

    /// Send a request and return the FULL JSON-RPC envelope (result OR error),
    /// asserting nothing about success. The shared basis for the happy-path
    /// [`Self::request`] and the negative-path `*_expect_error` helpers — so
    /// "this call SHOULD fail" is expressible, not structurally impossible.
    pub fn request_raw(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let params = self.authorize_tool_call(method, params);
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{msg}").expect("write request");
        self.stdin.flush().expect("flush");

        let mut line = String::new();
        loop {
            line.clear();
            let n = self.stdout.read_line(&mut line).expect("read response");
            assert!(
                n > 0,
                "helixir-mcp closed stdout while waiting for {method}"
            );
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                return value;
            }
            // Server-initiated notification (no matching id): capture it so
            // tests can assert on best-effort pushes, then keep waiting.
            if value.get("method").is_some() && value.get("id").is_none() {
                self.notifications.push(value);
            }
        }
    }

    fn authorize_tool_call(&self, method: &str, mut params: Value) -> Value {
        if method != "tools/call" {
            return params;
        }
        let Some(call) = params.as_object_mut() else {
            return params;
        };
        let tool_name = call
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let Some(arguments) = call.get_mut("arguments").and_then(Value::as_object_mut) else {
            return params;
        };

        // Explicit test arguments always win. This preserves negative actor
        // mismatch coverage while making legacy happy-path fixtures RBAC aware.
        arguments
            .entry("actor_id")
            .or_insert_with(|| Value::String(self.actor_id.clone()));
        if matches!(tool_name.as_str(), "add_memory" | "think_commit") {
            arguments
                .entry("group_id")
                .or_insert_with(|| Value::String(self.group_id.clone()));
        }
        params
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
        let value = self.request_raw(method, params);
        assert!(
            value.get("error").is_none(),
            "{method} returned error: {value}"
        );
        value["result"].clone()
    }

    /// Assert the request fails at the JSON-RPC layer; return the `error` object.
    pub fn request_expect_error(&mut self, method: &str, params: Value) -> Value {
        let value = self.request_raw(method, params);
        assert!(
            value.get("error").is_some(),
            "{method} was expected to error but succeeded: {value}"
        );
        value["error"].clone()
    }

    /// Call a tool expecting failure — either a JSON-RPC error or a tool result
    /// flagged `isError`. Returns the error text so the test can assert on it.
    pub fn call_tool_expect_error(&mut self, name: &str, arguments: Value) -> String {
        let value = self.request_raw("tools/call", json!({"name": name, "arguments": arguments}));
        if let Some(err) = value.get("error") {
            return err.to_string();
        }
        let result = &value["result"];
        let is_error = result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        assert!(
            is_error,
            "{name} was expected to error but returned ok: {result}"
        );
        result["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string()
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        let msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
        writeln!(self.stdin, "{msg}").expect("write notification");
        self.stdin.flush().expect("flush");
    }

    /// Read an MCP resource and return its text content (#34 2b tests).
    #[allow(dead_code)] // each e2e binary compiles common/ separately
    pub fn read_resource(&mut self, uri: &str) -> String {
        let result = self.request("resources/read", json!({"uri": uri}));
        result["contents"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("resource {uri}: no text content in {result}"))
            .to_string()
    }

    /// Calls a tool and returns (parsed inner JSON payload, wall ms).
    pub fn call_tool(&mut self, name: &str, arguments: Value) -> (Value, f64) {
        let t0 = Instant::now();
        let result = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: no text content in {result}"));
        let payload: Value = serde_json::from_str(text)
            .unwrap_or_else(|e| panic!("{name}: payload is not JSON ({e}): {text}"));
        (payload, ms)
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
