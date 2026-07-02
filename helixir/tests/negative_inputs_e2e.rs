//! Robustness floor: malformed / degenerate inputs must be handled gracefully
//! — a well-formed response (clean result OR clean error), never a panic, a
//! crash, or a hang. No negative test existed before, so a panic on bad input
//! could ship undetected. The decisive check is that the server still serves a
//! normal request AFTER being fed garbage.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test negative_inputs_e2e -- --ignored --nocapture
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

mod common;
use common::McpClient;

fn token() -> String {
    format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

/// A response is "well-formed" if it carries either a JSON-RPC `result` or a
/// JSON-RPC `error` — i.e. the server answered rather than dying or hanging.
fn well_formed(env: &serde_json::Value) -> bool {
    env.get("result").is_some() || env.get("error").is_some()
}

#[test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + working LLM"]
fn malformed_inputs_are_handled_gracefully() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );

    let (mut mcp, _boot) = McpClient::spawn();
    let run = token();
    let user = format!("neg_{run}");

    // Degenerate add_memory inputs: each must produce a well-formed envelope
    // (extracting zero facts is a fine outcome; crashing is not).
    let bad_inputs = [
        ("empty", String::new()),
        ("whitespace", "   \n\t  ".to_string()),
        ("punctuation", "!!!??? ... ;;;".to_string()),
        ("control_chars", "a\u{0000}b\u{0007}c".to_string()),
        ("very_long", "the cat sat. ".repeat(1500)),
    ];
    for (label, msg) in &bad_inputs {
        let env = mcp.request_raw(
            "tools/call",
            json!({"name": "add_memory", "arguments": {"message": msg, "user_id": user}}),
        );
        assert!(
            well_formed(&env),
            "add_memory({label}) must return a well-formed response, not crash: {env}"
        );
    }

    // A bogus tool name must be a clean JSON-RPC error, not a panic.
    let err = mcp.call_tool_expect_error("no_such_tool", json!({"x": 1}));
    assert!(
        !err.is_empty(),
        "an unknown tool must yield a structured error"
    );

    // Decisive: after all that garbage, the server is still alive and serving a
    // normal request — proving none of the bad inputs wedged it.
    let (added, _) = mcp.call_tool(
        "add_memory",
        json!({"message": format!("neg {run}: the canary fact survived the garbage."), "user_id": user}),
    );
    assert!(
        added["memories_added"].as_u64().unwrap_or(0) >= 1,
        "server must still accept a valid write after malformed inputs: {added}"
    );
    let (results, _) = mcp.call_tool(
        "search_memory",
        json!({"query": format!("neg {run} canary"), "user_id": user, "mode": "full", "limit": 3}),
    );
    assert!(
        results.as_array().map(|a| !a.is_empty()).unwrap_or(false),
        "server must still serve reads after malformed inputs: {results}"
    );

    println!("\n==== negative_inputs_e2e ====\nall malformed inputs handled; server survived");
}
