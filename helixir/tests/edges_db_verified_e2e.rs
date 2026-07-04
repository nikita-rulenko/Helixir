//! DB-ground-truth guard for the typed-edge arsenal (P0).
//!
//! Other reasoning tests read edges back through the MCP layer. This one writes
//! via MCP, then queries HelixDB DIRECTLY (`getMemoryOutgoingRelations`) to
//! assert the persisted edges carry VALID `relation_type`s from the full
//! arsenal — catching silent coercion / garbage that an MCP-shaped read could
//! hide. Runs under whatever provider HELIX_LLM_PROVIDER selects, so it doubles
//! as a Cerebras-vs-DeepSeek edge-quality probe.
//!
//! ```text
//! HELIX_E2E=1 cargo test -p helixir --test edges_db_verified_e2e -- --ignored --nocapture
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

mod common;
use common::{McpClient, db_edge_types_out};

fn token() -> String {
    format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

/// The full persistable arsenal. IMPLIES/BECAUSE are dedicated edges; the rest
/// ride the generic MEMORY_RELATION edge keyed by `relation_type`.
const ARSENAL: [&str; 7] = [
    "IMPLIES",
    "BECAUSE",
    "SUPPORTS",
    "CONTRADICTS",
    "RELATES_TO",
    "PART_OF",
    "IS_A",
];
const ASSOCIATIVE: [&str; 3] = ["RELATES_TO", "PART_OF", "IS_A"];

#[test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + working LLM"]
fn edges_persisted_with_correct_types_db() {
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
    let user = format!("edb_{run}");
    let provider = std::env::var("HELIX_LLM_PROVIDER").unwrap_or_else(|_| "default".into());
    let model = std::env::var("HELIX_LLM_MODEL").unwrap_or_else(|_| "?".into());

    // (1) Intra-input causal + structural — a "because" reliably yields a typed
    // reasoning edge between the two extracted facts.
    let (a, a_ms) = mcp.call_tool(
        "add_memory",
        json!({
            "message": format!(
                "Edges DB {run}: the api_{run} server crashed because the disk_{run} was full. \
                 The disk_{run} is a component of the storage_{run} subsystem."
            ),
            "user_id": user,
        }),
    );
    assert!(
        a["memories_added"].as_u64().unwrap_or(0) >= 1,
        "must store memories: {a}"
    );

    // (2) Cross-memory: two SEPARATE writes about the same entity. The second
    // write's decision may wire a typed edge to the first (decision.relates_to).
    mcp.call_tool(
        "add_memory",
        json!({"message": format!("Edges DB {run}: the auth_{run} gateway validates every request token."), "user_id": user}),
    );
    mcp.call_tool(
        "add_memory",
        json!({"message": format!("Edges DB {run}: the auth_{run} gateway is implemented in Rust."), "user_id": user}),
    );

    // ---- DB GROUND TRUTH: sweep every memory's outgoing edges from HelixDB ----
    let (listed, _) = mcp.call_tool("list_memories", json!({"user_id": user, "limit": 50}));
    let mids: Vec<String> = listed
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["memory_id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        !mids.is_empty(),
        "list_memories must return our memories: {listed}"
    );

    let mut all_edges: Vec<String> = Vec::new();
    for mid in &mids {
        all_edges.extend(db_edge_types_out(mid));
    }

    // Assertion 1: edges were actually persisted (read straight from the DB).
    assert!(
        !all_edges.is_empty(),
        "no reasoning edges persisted in HelixDB for any of {} memories — the \
         graph-of-why is not being built",
        mids.len()
    );
    // Assertion 2: every persisted edge type is a VALID arsenal member — no
    // silent garbage, no corruption (the #P0 anti-regression).
    let invalid: Vec<&String> = all_edges
        .iter()
        .filter(|t| !ARSENAL.iter().any(|a| t.as_str() == *a))
        .collect();
    assert!(
        invalid.is_empty(),
        "every persisted edge must be a valid arsenal type; found invalid: {invalid:?} \
         (all: {all_edges:?})"
    );

    let associative: Vec<&String> = all_edges
        .iter()
        .filter(|t| ASSOCIATIVE.iter().any(|a| t.as_str() == *a))
        .collect();

    println!("\n==== edges_persisted_with_correct_types_db ====");
    println!("PROVIDER={provider} MODEL={model} first_add={a_ms:.0}ms");
    println!(
        "memories: {} | edges persisted in DB: {}",
        mids.len(),
        all_edges.len()
    );
    println!("edge types (DB ground truth): {all_edges:?}");
    if associative.is_empty() {
        println!("associative (PART_OF/IS_A/RELATES_TO): none this run");
    } else {
        println!("associative (PART_OF/IS_A/RELATES_TO): {associative:?}");
    }
}

/// #83: the causal 2-cycle guard — A BECAUSE B and B BECAUSE A cannot both
/// be true. The first writer wins; the reverse write is a soft no-op. Verified
/// against DB ground truth, not the API return value.
#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings"]
async fn causal_two_cycle_rejected_db() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );

    let client = helixir::core::helixir_client::HelixirClient::from_env().expect("client from env");
    client.initialize().await.expect("initialize");

    let run = token();
    let user = format!("cycle_{run}");
    let atoms: Vec<helixir::llm::extractor::ExtractedMemory> = [
        format!("cycle test {run}: the pump_{run} overheated"),
        format!("cycle test {run}: the coolant_{run} valve was stuck"),
    ]
    .into_iter()
    .map(|text| helixir::llm::extractor::ExtractedMemory {
        text,
        memory_type: "fact".to_string(),
        certainty: 90,
        importance: 50,
        entities: vec![],
        context: None,
    })
    .collect();
    let r = client
        .add_prepared(atoms, &user, None, Some("cycle-guard-e2e"))
        .await
        .expect("prepared add");
    assert_eq!(r.memories_added, 2, "two atoms expected: {r:?}");
    let (a, b) = (&r.memory_ids[0], &r.memory_ids[1]);

    // Forward causal edge lands...
    client
        .tooling()
        .add_typed_relation(
            a,
            b,
            helixir::toolkit::mind_toolbox::reasoning::ReasoningType::Because,
            80,
        )
        .await
        .expect("forward BECAUSE");
    // ...the reverse one must be soft-skipped (Ok, but NOT persisted).
    client
        .tooling()
        .add_typed_relation(
            b,
            a,
            helixir::toolkit::mind_toolbox::reasoning::ReasoningType::Because,
            80,
        )
        .await
        .expect("reverse call itself must not error");

    let b_out = db_edge_types_out(b);
    assert!(
        !b_out.contains(&"BECAUSE".to_string()),
        "reverse BECAUSE must NOT persist (b outgoing: {b_out:?})"
    );
    let a_out = db_edge_types_out(a);
    assert!(
        a_out.contains(&"BECAUSE".to_string()),
        "forward BECAUSE must persist (a outgoing: {a_out:?})"
    );
    println!("==== causal_two_cycle_rejected_db ==== forward kept, reverse rejected");
}
