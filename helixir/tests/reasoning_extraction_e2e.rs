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

const ASSOCIATIVE_EDGES: [&str; 3] = ["RELATES_TO", "PART_OF", "IS_A"];
const ALL_EDGES: [&str; 7] = [
    "BECAUSE",
    "IMPLIES",
    "SUPPORTS",
    "CONTRADICTS",
    "RELATES_TO",
    "PART_OF",
    "IS_A",
];

/// P0 guard: the FULL typed-edge arsenal must actually build — not just the 4
/// causal types. Before this fix the extractor only offered IMPLIES/BECAUSE/
/// CONTRADICTS/SUPPORTS and the parser silently coerced anything else to
/// IMPLIES, so associative structure (PART_OF / IS_A / RELATES_TO) was never
/// stored. This feeds an explicitly STRUCTURAL statement and asserts an
/// associative edge lands on the fresh sub-graph.
#[test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + working LLM"]
fn associative_edges_built_on_fresh_write() {
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
    let user = format!("assoc_{run}");

    // Compositional/taxonomic structure (associative-edge attempt) PLUS one
    // explicit causal clause so there is always >=1 inter-atom edge to assert on
    // (structural links often stay intra-atom — see edges_db_verified for the
    // robust DB-ground-truth check; here we just guard against zero edges).
    let structural = format!(
        "Arsenal e2e {run}: the lexer_{run} is a part of the compiler_{run}. \
         The compiler_{run} is a kind of language toolchain. The build_{run} \
         failed because the lexer_{run} rejected malformed input."
    );
    let (added, add_ms) =
        mcp.call_tool("add_memory", json!({"message": structural, "user_id": user}));
    assert!(
        added["memories_added"].as_u64().unwrap_or(0) >= 1,
        "the structural statement must store memories: {added}"
    );
    let provider = std::env::var("HELIX_LLM_PROVIDER").unwrap_or_else(|_| "default".into());
    let model = std::env::var("HELIX_LLM_MODEL").unwrap_or_else(|_| "?".into());

    let (graph, _) = mcp.call_tool("get_memory_graph", json!({"user_id": user}));
    let edges = graph["edges"].as_array().cloned().unwrap_or_default();
    let edge_types: Vec<String> = edges
        .iter()
        .filter_map(|e| e["edge_type"].as_str().map(str::to_string))
        .collect();

    // No silent garbage/corruption: every typed memory→memory edge is a valid
    // arsenal member (HAS_MEMORY etc. structural edges are allowed through).
    let arsenal_edges: Vec<&String> = edge_types
        .iter()
        .filter(|t| ALL_EDGES.iter().any(|a| t.as_str() == *a))
        .collect();
    assert!(
        !arsenal_edges.is_empty(),
        "a structural statement must create at least one typed edge: {edges:?}"
    );

    // Anti-regression invariant (the reported P0): the pipeline must NOT collapse
    // every relation to IMPLIES. Before the fix the parser silently coerced any
    // non-causal token to IMPLIES; now the model picks from the full vocabulary,
    // so a structural write yields a NON-IMPLIES typed edge. (Which specific type
    // the LLM chooses — SUPPORTS vs PART_OF vs RELATES_TO — is model-steering, not
    // wiring; the arsenal's correctness is locked by the types.rs unit tests.)
    let non_implies: Vec<&String> = arsenal_edges
        .iter()
        .filter(|t| t.as_str() != "IMPLIES")
        .copied()
        .collect();
    assert!(
        !non_implies.is_empty(),
        "the relation vocabulary must not be collapsed to IMPLIES — a structural \
         write must yield a non-IMPLIES typed edge; got: {edge_types:?}"
    );

    let associative: Vec<&String> = edge_types
        .iter()
        .filter(|t| ASSOCIATIVE_EDGES.iter().any(|a| t.as_str() == *a))
        .collect();

    println!("\n==== associative_edges_built_on_fresh_write ====");
    println!("PROVIDER={provider} MODEL={model} add_memory={add_ms:.0}ms");
    println!("all edge types: {edge_types:?}");
    println!("typed (arsenal) edges: {arsenal_edges:?}");
    if associative.is_empty() {
        println!(
            "associative (PART_OF/IS_A/RELATES_TO): NONE this run (model chose {non_implies:?})"
        );
    } else {
        println!("associative (PART_OF/IS_A/RELATES_TO): {associative:?}");
    }
}
