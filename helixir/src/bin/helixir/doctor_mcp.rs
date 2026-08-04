use super::*;

use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc;
use std::time::Instant;

pub(crate) fn doctor_mcp_smoke(binary: &Path) -> Result<()> {
    anyhow::ensure!(binary.is_file(), "helixir-mcp binary is missing");
    let mut child = Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawn {}", binary.display()))?;
    let mut stdin = child.stdin.take().context("open helixir-mcp stdin")?;
    let stdout = child.stdout.take().context("open helixir-mcp stdout")?;
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                let _ = sender.send(value);
            }
        }
    });

    let result = (|| -> Result<()> {
        writeln!(
            stdin,
            "{}",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "helixir-doctor", "version": env!("CARGO_PKG_VERSION")}
                }
            })
        )?;
        writeln!(
            stdin,
            "{}",
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {}
            })
        )?;
        writeln!(
            stdin,
            "{}",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list",
                "params": {}
            })
        )?;
        stdin.flush()?;

        let timeout = Duration::from_secs(15);
        let started = Instant::now();
        let mut initialized = false;
        let mut listed_tools = false;
        while started.elapsed() < timeout && !(initialized && listed_tools) {
            let remaining = timeout.saturating_sub(started.elapsed());
            let value = receiver
                .recv_timeout(remaining)
                .context("helixir-mcp smoke timed out")?;
            match value.get("id").and_then(serde_json::Value::as_u64) {
                Some(1) => {
                    anyhow::ensure!(value.get("error").is_none(), "MCP initialize failed");
                    initialized = value.get("result").is_some();
                }
                Some(2) => {
                    anyhow::ensure!(value.get("error").is_none(), "MCP tools/list failed");
                    listed_tools = value
                        .pointer("/result/tools")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|tools| !tools.is_empty());
                }
                _ => {}
            }
        }
        anyhow::ensure!(initialized, "MCP initialize response was not observed");
        anyhow::ensure!(listed_tools, "MCP tools/list returned no tools");
        Ok(())
    })();

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();
    result
}
