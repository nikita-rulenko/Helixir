//! FastThink fast-commit oracle (the "фаст синк нифига не фаст" fix).
//!
//! `think_commit` used to re-run LLM extraction over conclusions the session
//! already held as structure, and glued recalled evidence into the content as
//! `[Evidence: ...]` text — 40-96 s per commit on remote providers. The fix:
//! conclusions go to the pipeline as PREPARED atoms (no extraction call),
//! evidence becomes SUPPORTS provenance edges, and entity discovery moves to
//! a background task. This suite asserts both halves: the latency budget and
//! the evidence edge in the graph.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test fast_commit_e2e -- --ignored --nocapture
//! ```

use std::time::{Instant, SystemTime, UNIX_EPOCH};

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
fn think_commit_fast_path_latency_and_evidence_edge() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );

    let (mut mcp, _boot) = McpClient::spawn();
    let run = token();
    let user = format!("fastc_{run}");

    // Seed the evidence memory the session will recall.
    let evidence = format!("Fastc {run}: the aurora ingestion service reads from the kappa queue.");
    let (seeded, _) = mcp.call_tool("add_memory", json!({"message": evidence, "user_id": user}));
    let evidence_id = seeded["memory_ids"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| panic!("seed write must return a memory id: {seeded}"));

    // Build a session: root → observation → recall (pulls the evidence in).
    let s = format!("fastc_think_{run}");
    let (started, _) = mcp.call_tool(
        "think_start",
        json!({"session_id": s, "initial_thought": format!("Pick a retry policy for the aurora service ({run})")}),
    );
    let root_idx = started["root_thought_idx"].as_u64().unwrap_or(0);
    mcp.call_tool(
        "think_add",
        json!({
            "session_id": s,
            "content": "transient kappa outages last under a minute",
            "thought_type": "observation",
            "parent_idx": root_idx
        }),
    );
    let (recalled, _) = mcp.call_tool(
        "think_recall",
        json!({"session_id": s, "query": format!("aurora kappa queue {run}"), "parent_idx": root_idx, "user_id": user}),
    );
    assert!(
        recalled["recalled_count"].as_u64().unwrap_or(0) >= 1,
        "the seeded evidence must be recalled into the session: {recalled}"
    );

    // Conclusion is deliberately DISTANT from the evidence text: no near
    // neighbours → the deterministic gate decides ADD with zero LLM calls,
    // which is the fast path's headline property.
    let conclusion = format!(
        "Retry policy fastc {run}: exponential backoff capped at ninety seconds with jitter."
    );
    mcp.call_tool(
        "think_conclude",
        json!({"session_id": s, "conclusion": conclusion, "supporting_idx": [root_idx]}),
    );

    let t0 = Instant::now();
    let (committed, _) = mcp.call_tool("think_commit", json!({"session_id": s, "user_id": user}));
    let commit_ms = t0.elapsed().as_millis();

    let memory_id = committed["memory_id"]
        .as_str()
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| panic!("think_commit must return the stored id: {committed}"))
        .to_string();

    // Latency budget: no extraction call and (for this novel conclusion) no
    // decision call either — generous 25 s bound still catches a regression
    // to the 40-96 s re-extraction path.
    assert!(
        commit_ms < 25_000,
        "fast commit must stay under 25s, took {commit_ms}ms: {committed}"
    );

    // Evidence provenance: the recalled memory SUPPORTS the conclusion as a
    // graph edge, not as text pasted into the content.
    let (graph, _) = mcp.call_tool(
        "get_memory_graph",
        json!({"user_id": user, "memory_id": memory_id, "depth": 1}),
    );
    let edges = graph["edges"].as_array().cloned().unwrap_or_default();
    let has_supports = edges.iter().any(|e| {
        e["edge_type"]
            .as_str()
            .or_else(|| e["relation_type"].as_str())
            .map(|t| t.eq_ignore_ascii_case("SUPPORTS"))
            .unwrap_or(false)
    });
    assert!(
        has_supports,
        "the evidence memory {evidence_id} must SUPPORT the committed conclusion in the graph: {graph}"
    );

    // And the content itself must stay clean of the old [Evidence: ...] glue.
    let (results, _) = mcp.call_tool(
        "search_memory",
        json!({"query": format!("retry policy fastc {run}"), "user_id": user, "mode": "full", "limit": 3}),
    );
    let found = results
        .as_array()
        .and_then(|a| a.first())
        .and_then(|r| r["content"].as_str())
        .unwrap_or_default();
    assert!(
        !found.contains("[Evidence:"),
        "committed content must not carry [Evidence:] text: {found}"
    );

    println!("\n==== fast_commit_e2e ====");
    println!("commit took {commit_ms}ms; SUPPORTS provenance edge present ✓");
}
