//! MCP-transport end-to-end suite: drives the **real** `helixir-mcp` binary
//! over stdio JSON-RPC, exactly the way Claude Desktop / Claude Code does.
//!
//! Companion to `read_path_e2e.rs` (library-level). The same golden queries go
//! through both suites; the latency delta between them is the overhead of
//! Helixir-as-an-MCP-server (process, JSON serialization, transport framing).
//!
//! **Not run by default** (`#[ignore]`). Requires the same live infrastructure
//! as `read_path_e2e.rs`; the LLM key may (and should) be dead.
//!
//! Run:
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt HELIX_LLM_API_KEY=dead-key-on-purpose \
//!   cargo test -p helixir mcp_read_e2e -- --ignored --nocapture
//! ```

use serde_json::json;

mod common;
use common::McpClient;

const USER: &str = "bench";

fn percentile(sorted_ms: &[f64], p: f64) -> f64 {
    if sorted_ms.is_empty() {
        return 0.0;
    }
    let k = ((sorted_ms.len() - 1) as f64 * p / 100.0).round() as usize;
    sorted_ms[k.min(sorted_ms.len() - 1)]
}

/// Same golden set as read_path_e2e.rs — keep the two in sync by hand.
fn golden_set() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "flaky test Cyrillic",
            vec!["mem_6d0c00cbb797", "mem_02e89bafeed2", "raw_02b063cbbd7a"],
        ),
        ("TestIntegrationProductSearch", vec!["mem_02e89bafeed2"]),
        (
            "repository interfaces",
            vec!["mem_14f614cee843", "mem_c100418279dc", "mem_74c82048e8a9"],
        ),
        (
            "ICU extension SQLite",
            vec!["raw_3c52decc7930", "raw_02b063cbbd7a"],
        ),
        (
            "Clean Architecture test isolation",
            vec!["raw_97ec3e9ac5f9", "mem_4d3b50638e96"],
        ),
        ("test coverage repository sqlite", vec!["mem_c100418279dc"]),
        (
            "interfaces.go ProductRepository methods",
            vec!["mem_14f614cee843", "mem_491ed67a50f4", "mem_c100418279dc"],
        ),
        (
            "boilerplate trade-off",
            vec!["mem_c100418279dc", "raw_97ec3e9ac5f9"],
        ),
        (
            "setupTestDB isolated in-memory database",
            vec!["mem_02e89bafeed2"],
        ),
        (
            "SQLite LIKE case sensitivity Unicode",
            vec!["mem_7ed1df043686", "mem_02e89bafeed2", "raw_3c52decc7930"],
        ),
    ]
}

#[test]
#[ignore = "needs HELIX_E2E=1 and live HelixDB (bench corpus) + embeddings; see module doc"]
fn mcp_read_e2e() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );
    assert_eq!(
        std::env::var("HELIXIR_RETRIEVAL_PROFILE").unwrap_or_default(),
        "algo_opt",
        "This suite validates the algo_opt read path"
    );

    let (mut mcp, boot_ms) = McpClient::spawn();

    // Fixture guard: the golden set pins EXACT memory ids from the recorded
    // bench corpus. That corpus was lost with the bench data dir on
    // 2026-06-30; without it every miss is a false alarm, not a read-path
    // regression. Skip gracefully until the fixtures are re-recorded.
    let (probe, _) = mcp.call_tool("list_memories", json!({"user_id": USER, "limit": 1}));
    if probe.as_array().map(|a| a.is_empty()).unwrap_or(true) {
        println!("\n==== mcp_read_e2e ====");
        println!(
            "SKIP: user '{USER}' has no corpus (historic golden fixtures lost with the bench data, 2026-06-30). Re-record golden_set() against a fresh corpus to re-enable."
        );
        return;
    }

    // ---------- search_memory over the golden set ----------
    let golden = golden_set();
    let mut hits_at_5 = 0usize;
    let mut reciprocal_ranks: Vec<f64> = Vec::new();
    let mut cold_ms: Vec<f64> = Vec::new();
    let mut first_query_ms = 0.0f64;

    for (i, (query, expected)) in golden.iter().enumerate() {
        let (payload, ms) = mcp.call_tool(
            "search_memory",
            json!({"query": query, "user_id": USER, "mode": "full", "limit": 5}),
        );
        if i == 0 {
            first_query_ms = ms;
        }
        cold_ms.push(ms);

        let results = payload.as_array().cloned().unwrap_or_default();
        let rank = results
            .iter()
            .position(|r| r["id"].as_str().is_some_and(|id| expected.contains(&id)));
        match rank {
            Some(r) => {
                hits_at_5 += 1;
                reciprocal_ranks.push(1.0 / (r as f64 + 1.0));
            }
            None => {
                reciprocal_ranks.push(0.0);
                eprintln!("  MISS '{query}' via MCP");
            }
        }
    }
    let hit_rate = hits_at_5 as f64 / golden.len() as f64;
    let mrr = reciprocal_ranks.iter().sum::<f64>() / reciprocal_ranks.len() as f64;

    // warm pass (same queries again — exercises the traversal cache through MCP)
    let mut warm_ms: Vec<f64> = Vec::new();
    for (query, _) in &golden {
        let (_, ms) = mcp.call_tool(
            "search_memory",
            json!({"query": query, "user_id": USER, "mode": "full", "limit": 5}),
        );
        warm_ms.push(ms);
    }
    cold_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    warm_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());

    // ---------- search_reasoning_chain ----------
    let (chains, chain_ms) = mcp.call_tool(
        "search_reasoning_chain",
        json!({
            "query": "repository interfaces clean architecture",
            "user_id": USER, "chain_mode": "both", "max_depth": 5, "limit": 5
        }),
    );
    let chain_count = chains["chains"].as_array().map_or(0, Vec::len);
    assert!(chain_count > 0, "MCP chains must not be empty: {chains}");

    // ---------- search_by_concept ----------
    let (concepts, concept_ms) = mcp.call_tool(
        "search_by_concept",
        json!({
            "query": "flaky test decision", "user_id": USER,
            "concept_type": "action", "limit": 5
        }),
    );
    let concept_count = concepts.as_array().map_or(0, Vec::len);
    assert!(concept_count > 0, "MCP concept search must not be empty");

    // ---------- get_memory_graph ----------
    let (graph, graph_ms) = mcp.call_tool(
        "get_memory_graph",
        json!({"user_id": USER, "memory_id": "mem_c100418279dc", "depth": 2}),
    );
    let node_count = graph["nodes"].as_array().map_or(0, Vec::len);
    let edge_count = graph["edges"].as_array().map_or(0, Vec::len);
    assert!(
        node_count > 0 && edge_count > 0,
        "MCP graph must not be empty: {graph}"
    );

    // ---------- summary ----------
    println!("\n==== mcp_read_e2e summary (user={USER}) ====");
    println!("server boot (spawn→initialized): {boot_ms:.1}ms");
    println!(
        "search_memory   quality: hit@5 {hits_at_5}/{} ({:.0}%), MRR {:.3}",
        golden.len(),
        hit_rate * 100.0,
        mrr
    );
    println!(
        "search_memory   latency: session-first {:.1}ms | cold p50 {:.1}ms p95 {:.1}ms | warm p50 {:.1}ms p95 {:.1}ms",
        first_query_ms,
        percentile(&cold_ms, 50.0),
        percentile(&cold_ms, 95.0),
        percentile(&warm_ms, 50.0),
        percentile(&warm_ms, 95.0)
    );
    println!("reasoning_chain: {chain_count} chains, {chain_ms:.1}ms");
    println!("search_concept : {concept_count} results, {concept_ms:.1}ms");
    println!("get_graph      : {node_count} nodes / {edge_count} edges, {graph_ms:.1}ms");

    assert!(
        hit_rate >= 0.8,
        "context restoration via MCP degraded: hit@5 {hit_rate:.2} < 0.8"
    );
    assert!(
        mrr >= 0.5,
        "ranking via MCP degraded: MRR {mrr:.3} < 0.5 (baseline 0.582)"
    );
}
