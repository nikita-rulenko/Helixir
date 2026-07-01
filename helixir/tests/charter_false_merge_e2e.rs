//! #46 guard: the decision must NOT merge/supersede two memories that merely
//! share keywords or a theme but describe DIFFERENT specific subjects. Before
//! the SAME-SUBJECT GATE, topically-adjacent facts in the 0.70–0.98 gray zone
//! were over-eagerly UPDATE/SUPERSEDE'd, destroying information.
//!
//! ```text
//! HELIX_E2E=1 cargo test -p helixir --test charter_false_merge_e2e -- --ignored --nocapture
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

#[test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + working LLM"]
fn related_but_distinct_facts_both_persist_46() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );
    assert_ne!(
        std::env::var("HELIXIR_INGEST_BUFFER").unwrap_or_default(),
        "1",
        "this test runs the synchronous path — do NOT set HELIXIR_INGEST_BUFFER"
    );

    let (mut mcp, _boot) = McpClient::spawn();
    let run = token();
    let user = format!("fm46_{run}");

    // Two facts that SHARE the keyword "deduplication" but are about DIFFERENT
    // specific subjects (one is the algorithm, the other is the test that runs
    // it). They land in the gray-similarity zone, so the decision LLM is
    // consulted — and must choose ADD for the second, not supersede the first.
    let a = format!(
        "FM46 {run}: the deduplication algorithm fingerprints each memory with a sha256 content_key."
    );
    let b = format!(
        "FM46 {run}: the deduplication test suite for run {run} executes against the Cerebras provider."
    );

    let (ra, _) = mcp.call_tool("add_memory", json!({"message": a, "user_id": user}));
    assert!(ra["ok"].as_bool().unwrap_or(false), "first write ok: {ra}");
    let (rb, _) = mcp.call_tool("add_memory", json!({"message": b, "user_id": user}));
    assert!(rb["ok"].as_bool().unwrap_or(false), "second write ok: {rb}");

    // The second write must NOT have been swallowed as a dedup/supersede of the
    // first: its `deduped` must not point at the first write's ids, and the
    // store must hold BOTH distinct facts.
    let (listed, _) = mcp.call_tool("list_memories", json!({"user_id": user, "limit": 50}));
    let rows = listed.as_array().cloned().unwrap_or_default();
    let has_algo = rows.iter().any(|m| {
        m["content"]
            .as_str()
            .map(|c| c.contains("content_key") || c.contains("fingerprint"))
            .unwrap_or(false)
    });
    let has_tests = rows.iter().any(|m| {
        m["content"]
            .as_str()
            .map(|c| c.contains("test suite") || c.contains("Cerebras"))
            .unwrap_or(false)
    });

    assert!(
        has_algo && has_tests,
        "both distinct facts must persist — the second must NOT be falsely \
         merged/superseded into the first just for sharing 'deduplication'. \
         got {} rows: {rows:?}",
        rows.len()
    );

    println!("\n==== related_but_distinct_facts_both_persist_46 ====");
    println!("both subjects survived (algo={has_algo}, tests={has_tests}); no false merge ✓");
}
