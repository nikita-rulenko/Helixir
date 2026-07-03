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

use common::golden::{GOLDEN_USER as USER, ensure_seeded, golden_set};

fn percentile(sorted_ms: &[f64], p: f64) -> f64 {
    if sorted_ms.is_empty() {
        return 0.0;
    }
    let k = ((sorted_ms.len() - 1) as f64 * p / 100.0).round() as usize;
    sorted_ms[k.min(sorted_ms.len() - 1)]
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

    // #76: seed the deterministic corpus through the LIBRARY client (LLM-free
    // add_prepared) before spawning the MCP binary under test.
    {
        let rt = tokio::runtime::Runtime::new().expect("rt");
        rt.block_on(async {
            let client = helixir::core::HelixirClient::from_env().expect("HelixirClient::from_env");
            client.initialize().await.expect("initialize");
            let seeded = ensure_seeded(&client).await;
            println!("golden corpus: {seeded} atoms added this run");
        });
    }

    let (mut mcp, boot_ms) = McpClient::spawn();

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
        let rank = results.iter().position(|r| {
            r["content"]
                .as_str()
                .is_some_and(|c| expected.iter().any(|m| c.contains(m)))
        });
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
            "query": "why did payments migrate from sqlite to postgres",
            "user_id": USER, "chain_mode": "both", "max_depth": 5, "limit": 5
        }),
    );
    let chain_count = chains["chains"].as_array().map_or(0, Vec::len);
    assert!(chain_count > 0, "MCP chains must not be empty: {chains}");

    // ---------- search_by_concept ----------
    let (concepts, concept_ms) = mcp.call_tool(
        "search_by_concept",
        json!({
            "query": "payments service migrated postgres", "user_id": USER,
            "concept_type": "action", "limit": 5
        }),
    );
    let concept_count = concepts.as_array().map_or(0, Vec::len);
    assert!(concept_count > 0, "MCP concept search must not be empty");

    // ---------- get_memory_graph ----------
    // Anchor resolved at runtime: ids are random per seed, content is ours.
    let (anchor_rs, _) = mcp.call_tool(
        "search_memory",
        json!({"query": "payments service migrated sqlite postgres", "user_id": USER, "mode": "full", "limit": 3}),
    );
    let anchor = anchor_rs
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|r| r["content"].as_str().unwrap_or("").contains("GA1"))
        })
        .and_then(|r| r["id"].as_str())
        .expect("GA1 must anchor the graph probe")
        .to_string();
    let (graph, graph_ms) = mcp.call_tool(
        "get_memory_graph",
        json!({"user_id": USER, "memory_id": anchor, "depth": 2}),
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
