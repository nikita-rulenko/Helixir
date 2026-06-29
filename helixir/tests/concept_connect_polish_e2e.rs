//! Polish guards: #23 (connect_memories must populate node content for id
//! anchors) and #62 (search_by_concept must not leak adjacent ontology types).
//!
//! ```text
//! HELIX_E2E=1 cargo test -p helixir --test concept_connect_polish_e2e -- --ignored --nocapture
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

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

fn require_e2e() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );
}

/// #23: connecting two memories by their ids must return a path whose endpoint
/// nodes carry their actual content, not blank strings.
#[test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + working LLM"]
fn connect_by_id_populates_node_content_23() {
    require_e2e();
    let (mut mcp, _boot) = McpClient::spawn();
    let run = token();
    let user = format!("c23_{run}");

    // One causal input → two linked atoms (a BECAUSE edge between them).
    let (added, _) = mcp.call_tool(
        "add_memory",
        json!({
            "message": format!("C23 {run}: service zeta_{run} adopted event sourcing because it needed a full audit trail of every state change."),
            "user_id": user
        }),
    );
    let ids: Vec<String> = added["memory_ids"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    assert!(ids.len() >= 2, "need two linked atoms to connect: {added}");

    let (conn, _) = mcp.call_tool(
        "connect_memories",
        json!({"query_a": ids[0], "query_b": ids[1], "user_id": user}),
    );
    assert_eq!(conn["found"].as_bool(), Some(true), "anchors must connect: {conn}");
    let nodes = conn["nodes"].as_array().cloned().unwrap_or_default();
    assert!(!nodes.is_empty(), "path must have nodes: {conn}");
    let all_have_content = nodes
        .iter()
        .all(|n| n["content"].as_str().map(|c| !c.trim().is_empty()).unwrap_or(false));
    assert!(
        all_have_content,
        "#23: every node in a by-id connect path must carry its content (not blank): {nodes:?}"
    );

    println!("\n==== connect_by_id_populates_node_content_23 ====");
    println!("path nodes all carry content ✓");
}

/// #62: search_by_concept(concept_type=preference) must return preference-typed
/// memories only — NOT a fact on the same topic pulled in via graph expansion.
#[test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + working LLM"]
fn concept_filter_excludes_adjacent_types_62() {
    require_e2e();
    let (mut mcp, _boot) = McpClient::spawn();
    let run = token();
    let user = format!("c62_{run}");

    // A clear PREFERENCE and a clear FACT on the same topic (same keywords →
    // they'd graph-expand into each other).
    mcp.call_tool(
        "add_memory",
        json!({"message": format!("C62 {run}: I strongly prefer the Kraken_{run} database for my own projects."), "user_id": user}),
    );
    mcp.call_tool(
        "add_memory",
        json!({"message": format!("C62 {run}: the Kraken_{run} database stores data in a log-structured merge tree."), "user_id": user}),
    );

    let (res, _) = mcp.call_tool(
        "search_by_concept",
        json!({"query": format!("Kraken_{run} database"), "user_id": user, "concept_type": "preference", "mode": "full", "limit": 10}),
    );
    let rows: Vec<Value> = res.as_array().cloned().unwrap_or_default();
    // The preference must be found; the fact ("log-structured merge tree") must NOT.
    let pref_found = rows
        .iter()
        .any(|m| m["content"].as_str().map(|c| c.contains("prefer")).unwrap_or(false));
    let fact_leaked = rows
        .iter()
        .any(|m| m["content"].as_str().map(|c| c.contains("log-structured") || c.contains("merge tree")).unwrap_or(false));

    assert!(pref_found, "the preference must be returned for concept_type=preference: {rows:?}");
    assert!(
        !fact_leaked,
        "#62: a FACT on the same topic must NOT leak into a concept_type=preference search: {rows:?}"
    );

    println!("\n==== concept_filter_excludes_adjacent_types_62 ====");
    println!("preference returned, adjacent fact excluded ✓");
}
