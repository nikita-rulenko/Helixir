//! Liveness oracle: exercise EVERY MCP Helixir tool end-to-end (#42 audit).
//!
//! "Compiles" does not prove a code path is live — only the running product
//! does. This drives all 17 MCP tools through the real stdio transport and
//! proves write→read-back persistence, so it can serve as the gate for the
//! dead-code deletion stages: after removing a suspected-dead module, this
//! must stay green (not just `cargo check`).
//!
//! Runs the synchronous write path (buffer OFF) for determinism. Tools covered:
//!   add_memory, search_memory, list_memories, update_memory, get_memory_graph,
//!   search_by_concept, search_reasoning_chain, connect_memories,
//!   search_incomplete_thoughts, get_add_status,
//!   think_start, think_add, think_status, think_recall, think_conclude,
//!   think_commit, think_discard.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test mcp_full_surface_e2e -- --ignored --nocapture
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
fn mcp_full_surface_liveness() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );
    assert_ne!(
        std::env::var("HELIXIR_INGEST_BUFFER").unwrap_or_default(),
        "1",
        "this oracle runs the synchronous path — do NOT set HELIXIR_INGEST_BUFFER"
    );

    let (mut mcp, _boot) = McpClient::spawn();
    let run = token();
    let user = format!("oracle_{run}");
    let mut exercised: Vec<&str> = Vec::new();

    // ---- write + persistence read-back -------------------------------------
    let fact =
        format!("Oracle {run}: the canonical CI runner for project zeta is buildkite on arm64.");
    let (added, _) = mcp.call_tool("add_memory", json!({"message": fact, "user_id": user}));
    exercised.push("add_memory");
    let added_ids: Vec<String> = added["memory_ids"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        added["memories_added"].as_u64().unwrap_or(0) >= 1 || !added_ids.is_empty(),
        "sync add_memory must persist at least one memory: {added}"
    );

    let (results, _) = mcp.call_tool(
        "search_memory",
        json!({"query": fact, "user_id": user, "mode": "full", "limit": 5}),
    );
    exercised.push("search_memory");
    assert!(
        results.as_array().map(|a| !a.is_empty()).unwrap_or(false),
        "the just-added fact must be searchable (write→read-back): {results}"
    );

    let (listed, _) = mcp.call_tool("list_memories", json!({"user_id": user}));
    exercised.push("list_memories");
    let list_arr = listed.as_array().cloned().unwrap_or_default();
    assert!(
        !list_arr.is_empty(),
        "list_memories must return our memory: {listed}"
    );

    // A memory id to update: prefer the add result, else the listing.
    let mem_id = added_ids
        .first()
        .cloned()
        .or_else(|| {
            list_arr
                .first()
                .and_then(|m| m["memory_id"].as_str().or_else(|| m["id"].as_str()))
                .map(str::to_string)
        })
        .expect("need a memory_id to exercise update_memory");

    let (updated, _) = mcp.call_tool(
        "update_memory",
        json!({
            "memory_id": mem_id,
            "new_content": format!("Oracle {run}: CI runner for project zeta is buildkite on arm64 (verified)."),
            "user_id": user
        }),
    );
    exercised.push("update_memory");
    assert_eq!(
        updated["updated"].as_bool(),
        Some(true),
        "update_memory must report updated=true: {updated}"
    );
    // Read-back: the new content must be retrievable (the update actually landed).
    let (after_update, _) = mcp.call_tool(
        "search_memory",
        json!({"query": format!("Oracle {run} verified buildkite"), "user_id": user, "mode": "full", "limit": 5}),
    );
    assert!(
        after_update
            .as_array()
            .map(|a| a.iter().any(|r| r["content"].as_str().unwrap_or("").contains("verified")))
            .unwrap_or(false),
        "the updated content must be searchable after update_memory: {after_update}"
    );

    // ---- the rest of the read surface --------------------------------------
    let (graph, _) = mcp.call_tool("get_memory_graph", json!({"user_id": user}));
    exercised.push("get_memory_graph");
    assert!(
        graph["nodes"].as_array().map(|a| !a.is_empty()).unwrap_or(false),
        "get_memory_graph must return the user's node(s): {graph}"
    );

    let (by_concept, _) = mcp.call_tool(
        "search_by_concept",
        json!({"query": fact, "user_id": user, "concept_type": "fact"}),
    );
    exercised.push("search_by_concept");
    assert!(
        by_concept.as_array().map(|a| !a.is_empty()).unwrap_or(false),
        "search_by_concept must find the just-added fact: {by_concept}"
    );

    let (chain, _) = mcp.call_tool(
        "search_reasoning_chain",
        json!({"query": fact, "user_id": user}),
    );
    exercised.push("search_reasoning_chain");
    assert!(
        chain["chains"].is_array() && chain["total_memories"].is_number(),
        "search_reasoning_chain must return a well-formed chain envelope: {chain}"
    );

    let (connected, _) = mcp.call_tool(
        "connect_memories",
        json!({"query_a": "CI runner zeta", "query_b": "buildkite arm64", "user_id": user}),
    );
    exercised.push("connect_memories");
    // This oracle user has a single unconnected memory, so there is no PATH to
    // find (found=false is correct). The surface guard here is well-formedness:
    // the tool must compute and return a complete envelope, not error or return
    // garbage. Deterministic path DISCOVERY (found=true, hops>=1) is asserted in
    // cross_domain_bridge_e2e over a seeded connected graph.
    assert!(
        connected["found"].is_boolean()
            && connected["hops"].is_number()
            && connected["nodes"].is_array()
            && connected["edges"].is_array(),
        "connect_memories must return a well-formed path envelope: {connected}"
    );

    let (incomplete, _) = mcp.call_tool("search_incomplete_thoughts", json!({"limit": 3}));
    exercised.push("search_incomplete_thoughts");
    assert!(
        incomplete["found"].is_number() || incomplete.is_array(),
        "search_incomplete_thoughts must return a well-formed result: {incomplete}"
    );

    // get_add_status on a non-existent id must report not_found (buffer off).
    let (status, _) = mcp.call_tool(
        "get_add_status",
        json!({"pending_id": "pi_oracle_does_not_exist"}),
    );
    exercised.push("get_add_status");
    assert_eq!(
        status["status"].as_str(),
        Some("not_found"),
        "get_add_status on a bogus id must be not_found: {status}"
    );

    // ---- FastThink happy path (start→add→status→recall→conclude→commit) ----
    // The conclusion must be a NOVEL fact, unrelated to `fact` above, so the
    // add pipeline genuinely stores it (a duplicate would dedup to 0 memories
    // and the empty memory_id would be correct behaviour, not a commit bug).
    let s = format!("oracle_think_{run}");
    let (started, _) = mcp.call_tool(
        "think_start",
        json!({"session_id": s, "initial_thought": format!("Decide the backup cadence for datastore nimbus_{run}")}),
    );
    exercised.push("think_start");
    let root_idx = started["root_thought_idx"].as_u64().unwrap_or(0);

    let (thought, _) = mcp.call_tool(
        "think_add",
        json!({
            "session_id": s,
            "content": format!("datastore nimbus_{run} holds only ephemeral session data"),
            "thought_type": "observation",
            "parent_idx": root_idx
        }),
    );
    exercised.push("think_add");
    let thought_idx = thought["thought_idx"].as_u64().unwrap_or(root_idx);

    let (tstatus, _) = mcp.call_tool("think_status", json!({"session_id": s}));
    exercised.push("think_status");
    assert!(
        tstatus["thought_count"].as_u64().unwrap_or(0) >= 2,
        "think_status must reflect root + added thought: {tstatus}"
    );

    let (recalled, _) = mcp.call_tool(
        "think_recall",
        json!({"session_id": s, "query": format!("nimbus_{run} backup"), "parent_idx": root_idx, "user_id": user}),
    );
    exercised.push("think_recall");
    assert!(
        recalled["recalled_count"].is_number() && recalled["thought_indices"].is_array(),
        "think_recall must report what it pulled into the session: {recalled}"
    );

    let (concluded, _) = mcp.call_tool(
        "think_conclude",
        json!({
            "session_id": s,
            "conclusion": format!("datastore nimbus_{run} is backed up hourly with seven day retention"),
            "supporting_idx": [thought_idx]
        }),
    );
    exercised.push("think_conclude");
    assert_eq!(
        concluded["status"].as_str(),
        Some("decided"),
        "think_conclude must move the session to 'decided': {concluded}"
    );

    let (committed, _) = mcp.call_tool("think_commit", json!({"session_id": s, "user_id": user}));
    exercised.push("think_commit");
    assert!(
        committed["memory_id"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "think_commit must persist a memory and return its id: {committed}"
    );

    // ---- FastThink discard path --------------------------------------------
    let s2 = format!("oracle_think_discard_{run}");
    mcp.call_tool(
        "think_start",
        json!({"session_id": s2, "initial_thought": "a throwaway line of reasoning"}),
    );
    let (discarded, _) = mcp.call_tool("think_discard", json!({"session_id": s2}));
    exercised.push("think_discard");
    assert!(
        discarded["discarded_thoughts"].as_u64().unwrap_or(0) >= 1,
        "think_discard must drop the started session's thought(s): {discarded}"
    );
    // The discarded session must be gone: status on it should no longer be active.
    let gone = mcp.call_tool_expect_error("think_status", json!({"session_id": s2}));
    assert!(
        !gone.is_empty(),
        "think_status on a discarded session should report it no longer exists"
    );

    // ---- report -------------------------------------------------------------
    const ALL: [&str; 17] = [
        "add_memory",
        "search_memory",
        "list_memories",
        "update_memory",
        "get_memory_graph",
        "search_by_concept",
        "search_reasoning_chain",
        "connect_memories",
        "search_incomplete_thoughts",
        "get_add_status",
        "think_start",
        "think_add",
        "think_status",
        "think_recall",
        "think_conclude",
        "think_commit",
        "think_discard",
    ];
    let missed: Vec<&&str> = ALL.iter().filter(|t| !exercised.contains(*t)).collect();

    println!("\n==== mcp_full_surface_e2e (liveness oracle) ====");
    println!("exercised {}/{} MCP tools", exercised.len(), ALL.len());
    if !missed.is_empty() {
        println!("MISSED: {missed:?}");
    }
    assert!(
        missed.is_empty(),
        "every MCP tool must be exercised: missed {missed:?}"
    );
}
