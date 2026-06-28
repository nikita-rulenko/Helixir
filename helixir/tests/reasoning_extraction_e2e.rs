//! Use-case guard: reasoning-relation extraction on a FRESH write.
//!
//! Every other reasoning assertion (read_path, longest_chain, mcp_read) reads
//! the pre-seeded `bench`/`claude` corpus — which already has BECAUSE/IMPLIES
//! edges. So if the extractor silently stopped emitting reasoning edges on new
//! writes, *no test would fail* and the "graph of why" value prop would rot
//! undetected. This test feeds a causal "X because Y" message and asserts the
//! edge is created live, on the just-written memories.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test reasoning_extraction_e2e -- --ignored --nocapture
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

const REASONING_EDGES: [&str; 4] = ["BECAUSE", "IMPLIES", "SUPPORTS", "CONTRADICTS"];

#[test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + working LLM"]
fn reasoning_edge_created_on_fresh_write() {
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
    let user = format!("reason_{run}");

    // An explicit causal statement: two facts joined by "because". A healthy
    // extractor splits them and links them with a typed reasoning edge.
    let causal = format!(
        "Reasoning e2e {run}: the team migrated datastore kappa_{run} from SQLite to Postgres \
         because SQLite could not handle the concurrent write load at peak traffic."
    );
    let (added, _) = mcp.call_tool("add_memory", json!({"message": causal, "user_id": user}));
    assert!(
        added["memories_added"].as_u64().unwrap_or(0) >= 1,
        "the causal statement must store at least one memory: {added}"
    );

    // The reasoning edge connects the two extracted facts on the writer's own
    // sub-graph — read it back via get_memory_graph.
    let (graph, _) = mcp.call_tool("get_memory_graph", json!({"user_id": user}));
    let edges = graph["edges"].as_array().cloned().unwrap_or_default();
    let reasoning_edges: Vec<&str> = edges
        .iter()
        .filter_map(|e| e["edge_type"].as_str())
        .filter(|t| REASONING_EDGES.iter().any(|r| t.contains(r)))
        .collect();

    assert!(
        !reasoning_edges.is_empty(),
        "a 'X because Y' write must create a typed reasoning edge between the \
         extracted facts (the graph-of-why must not silently stop emitting edges); \
         got edges: {edges:?}"
    );

    // Cross-check the same edge is reachable through the reasoning-chain tool —
    // the surface an agent actually uses to ask "why".
    let (chain, _) = mcp.call_tool(
        "search_reasoning_chain",
        json!({"query": format!("why was kappa_{run} migrated to Postgres"), "user_id": user}),
    );
    assert!(
        chain["chains"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "the freshly-created reasoning edge must surface in search_reasoning_chain: {chain}"
    );

    println!("\n==== reasoning_extraction_e2e ====");
    println!("reasoning edges created on fresh write: {reasoning_edges:?}");
}
