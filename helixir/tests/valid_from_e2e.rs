//! valid_from persistence (#45): the write path must store a real timestamp,
//! not the literal schema default "{{timestamp}}".
//!
//! Drives the real MCP transport: add_memory writes the Memory node via the new
//! `addMemoryWithValidFrom` query, and list_memories reads the raw stored node
//! back — so this asserts the actual DB entry, end to end.
//!
//! Requires the addMemoryWithValidFrom query deployed to the live instance.
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test valid_from_e2e -- --ignored --nocapture
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
#[ignore = "needs HELIX_E2E=1 + live HelixDB + LLM + addMemoryWithValidFrom deployed"]
fn valid_from_is_a_real_timestamp() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");
    assert_ne!(
        std::env::var("HELIXIR_INGEST_BUFFER").unwrap_or_default(),
        "1",
        "run the synchronous path for a deterministic read-back"
    );

    let (mut mcp, _boot) = McpClient::spawn();
    let run = token();
    let user = format!("vf_mcp_{run}");
    // Unique subject so the write is a genuine NEW node (not a dedup link to an
    // old row that predates the fix and still carries "{{timestamp}}").
    let svc = format!("vfsvc{run}");
    let msg = format!("Service {svc} rotates its TLS certificates every ninety days.");

    mcp.call_tool("add_memory", json!({ "message": msg, "user_id": user }));

    let (listed, _) = mcp.call_tool("list_memories", json!({ "user_id": user }));
    let arr = listed.as_array().cloned().unwrap_or_default();
    let hit = arr
        .iter()
        .find(|m| {
            m["content"]
                .as_str()
                .map(|c| c.contains(&svc))
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("must find the written memory mentioning {svc}: {listed}"));

    let valid_from = hit["valid_from"].as_str().unwrap_or("");
    assert_ne!(
        valid_from, "{{timestamp}}",
        "valid_from is the unsubstituted literal default — the fix did not take"
    );
    assert!(!valid_from.is_empty(), "valid_from is empty");
    assert!(
        chrono::DateTime::parse_from_rfc3339(valid_from).is_ok(),
        "valid_from must be a real RFC3339 timestamp, got {valid_from:?}"
    );

    println!("\n==== valid_from_e2e ====");
    println!("stored valid_from = {valid_from} ✓ (not the literal template)");
}
