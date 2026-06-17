//! Write-path end-to-end suite over the **real MCP transport** — the target
//! flow per project convention: agents talk to Helixir through `helixir-mcp`.
//!
//! Covers the v0.4.0 write features: batched decisions (W1), deterministic
//! gates (W2), charter escalations (`needs_clarification`) and Hive cognitive
//! layers (stances). LLM decisions are non-deterministic — assertions are
//! deliberately tolerant; if a run flakes, retry (same policy as
//! `hive_memory_e2e`).
//!
//! Requires live HelixDB + embeddings + a **working LLM** (writes extract):
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test mcp_write_e2e -- --ignored --nocapture
//! ```

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

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
#[ignore = "needs HELIX_E2E=1, live HelixDB + embeddings + working LLM; see module doc"]
fn mcp_write_e2e() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );
    assert_eq!(
        std::env::var("HELIXIR_RETRIEVAL_PROFILE").unwrap_or_default(),
        "algo_opt",
        "This suite validates the algo_opt write path"
    );

    // This suite exercises Phase 2 cross-user (Hive) linking, which is opt-in:
    // it only runs under HELIXIR_MODE=collective (default solo skips it).
    let (mut mcp, _boot) = McpClient::spawn_with_env(&[("HELIXIR_MODE", "collective")]);
    let run = token();

    // ---------- 1. multi-fact add goes through the batch path ----------
    let user_a = format!("e2e_write_{run}_a");
    let multi = format!(
        "Write e2e {run}, three separate facts: 1) The staging server runs on port 8443. \
         2) The team decided to freeze the API schema until March. \
         3) Backups are verified weekly by the oncall."
    );
    let (added, _) = mcp.call_tool("add_memory", json!({"message": multi, "user_id": user_a}));
    let first_added = added["memories_added"].as_u64().unwrap_or(0);
    // Extraction granularity is LLM-dependent (the prompt favours "fewer,
    // richer memories") — the suite asserts the FLOW, not the fact count.
    assert!(
        first_added >= 1,
        "multi-fact input must store at least one memory, got: {added}"
    );
    if first_added < 2 {
        eprintln!("note: extractor consolidated the input into {first_added} memory(ies) this run");
    }

    // ---------- 2. identical re-add is eaten by the gates ----------
    let (re_added, _) = mcp.call_tool("add_memory", json!({"message": multi, "user_id": user_a}));
    let second_added = re_added["memories_added"].as_u64().unwrap_or(99);
    assert!(
        second_added < first_added || second_added == 0,
        "re-adding the same input must be mostly NOOP'd by gates \
         (first={first_added}, second={second_added}): {re_added}"
    );

    // ---------- 3. preference reversal escalates per the charter ----------
    let user_b = format!("e2e_write_{run}_b");
    // No run-token prefix here on purpose: noisy prefixes skew extraction
    // typing away from "preference" (run isolation comes from the user_id).
    let (_, _) = mcp.call_tool(
        "add_memory",
        json!({
            "message": "I strongly prefer the dark color theme in every editor.",
            "user_id": user_b
        }),
    );
    let (reversal, _) = mcp.call_tool(
        "add_memory",
        json!({
            "message": "I now prefer the light color theme in every editor.",
            "user_id": user_b
        }),
    );
    let clarifications = reversal["needs_clarification"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !clarifications.is_empty(),
        "a preference reversal must escalate via needs_clarification \
         (charter C3); got: {reversal}"
    );
    let q = clarifications[0]["suggested_question"]
        .as_str()
        .unwrap_or("");
    assert!(
        !q.is_empty(),
        "clarification must carry a ready-to-ask question"
    );
    println!(
        "clarification: [{}] {}",
        clarifications[0]["conflict_type"], q
    );

    // ---------- 4. Hive stances: second knower confirms ----------
    let shared = format!(
        "Write e2e {run}: The canonical deployment region for the project is eu-central-1."
    );
    let user_c = format!("e2e_write_{run}_c");
    let user_d = format!("e2e_write_{run}_d");
    let (first, _) = mcp.call_tool("add_memory", json!({"message": shared, "user_id": user_c}));
    assert!(first["memories_added"].as_u64().unwrap_or(0) >= 1);
    let (_, _) = mcp.call_tool("add_memory", json!({"message": shared, "user_id": user_d}));

    // Phase 2 (cross-user link) runs in the background — poll collective
    // search until the stance distribution shows a second knower.
    let mut linked = false;
    for _ in 0..15 {
        std::thread::sleep(Duration::from_secs(2));
        let (results, _) = mcp.call_tool(
            "search_memory",
            json!({
                "query": shared, "user_id": user_c,
                "mode": "full", "scope": "collective", "limit": 5
            }),
        );
        let results = results.as_array().cloned().unwrap_or_default();
        linked = results.iter().any(|r| {
            let meta = &r["metadata"];
            let user_count_ok = meta["user_count"].as_u64().unwrap_or(0) >= 2;
            let stances = &meta["stances"];
            let confirms = stances["confirms"].as_u64().unwrap_or(0);
            let asserts = stances["asserts"].as_u64().unwrap_or(0);
            user_count_ok || confirms >= 1 && asserts >= 1
        });
        if linked {
            let sample: Vec<&Value> = results
                .iter()
                .filter(|r| r["metadata"]["stances"].is_object())
                .collect();
            if let Some(r) = sample.first() {
                println!(
                    "stances on shared fact: {} (user_count={})",
                    r["metadata"]["stances"], r["metadata"]["user_count"]
                );
            }
            break;
        }
    }
    assert!(
        linked,
        "second knower must appear via Phase 2 (HAS_MEMORY stance link) \
         within the polling window"
    );

    println!("\n==== mcp_write_e2e summary ====");
    println!("batch add: {first_added} memories; re-add gated to {second_added}");
    println!(
        "charter escalation: {} clarification(s)",
        clarifications.len()
    );
    println!("hive stances: second knower linked");
}
