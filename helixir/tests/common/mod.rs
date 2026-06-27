//! Shared MCP stdio harness for e2e suites: spawns the real `helixir-mcp`
//! binary and speaks JSON-RPC the way Claude Desktop / Claude Code does.

#![allow(dead_code)] // each test crate uses a subset

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Instant;

use serde_json::{Value, json};

pub struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
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
        let mut child = Command::new(env!("CARGO_BIN_EXE_helixir-mcp"))
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
