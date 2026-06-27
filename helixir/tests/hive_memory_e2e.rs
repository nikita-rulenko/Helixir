//! Hive (cross-user) end-to-end test: full stack HelixDB + embeddings + LLM extraction + Phase 2 collective dedup.
//!
//! **Not run by default** (`#[ignore]`). Requires live infrastructure:
//! - `HELIX_HOST`, `HELIX_PORT` (HelixDB)
//! - LLM + embedding env vars (same as MCP / `HelixirConfig::from_env`)
//!
//! Run:
//! ```text
//! HELIX_E2E=1 cargo test -p helixir hive_cross_user_collective_link_e2e -- --ignored --nocapture
//! ```
//!
//! LLM decisions (`LINK_EXISTING` / `NOOP` with link) are non-deterministic; if this flakes, retry or
//! adjust the prompt/model. The message uses a unique UUID so vector search can find the first user's memory.

use helixir::core::HelixirClient;
use std::time::{SystemTime, UNIX_EPOCH};

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 and live HelixDB + LLM + embeddings; see module doc"]
async fn hive_cross_user_collective_link_e2e() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );

    let token = format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    // A SHORT, atomic, hard-to-rephrase fact with the token embedded in a
    // proper noun: both users' extractions land on the same text → the same
    // content_key → a real consensus group. A long/abstract sentence (the old
    // message) atomized differently per call, yielding mismatched content_keys
    // and the spurious flakiness — the extractor variance, not the Hive logic.
    let message = format!("Project kappa{token} runs PostgreSQL 16 in production.");

    let user_a = format!("e2e_hive_{token}_a");
    let user_b = format!("e2e_hive_{token}_b");

    // Hive (cross-user collective linking) is opt-in — enable it for this suite.
    unsafe { std::env::set_var("HELIXIR_MODE", "collective"); }
    let client = HelixirClient::from_env().expect("HelixirClient::from_env");
    client
        .initialize()
        .await
        .expect("initialize (health + ontology)");

    let r_a = client
        .add(&message, &user_a, None, None)
        .await
        .expect("user_a add_memory");
    let mem_a = r_a
        .memory_ids
        .first()
        .cloned()
        .expect("user_a should produce at least one memory id");

    let r_b = client
        .add(&message, &user_b, None, None)
        .await
        .expect("user_b add_memory");

    let _ = (&mem_a, &r_b); // ids kept for clarity; consensus is checked by token below

    // Cross-user consensus settles ASYNCHRONOUSLY (Phase 2 link + content_key
    // grouping), so a single immediate read races it. Poll the collective view
    // until SOME memory from this run (token in content) reaches user_count >= 2
    // — the "one fact, many knowers" invariant. (The previous version read once
    // and so flaked on timing; #43 makes the grouping itself deterministic.)
    let mut ok = false;
    let mut last5: Vec<(String, u64, String)> = vec![];
    for _ in 0..15 {
        let results = client
            .search(&token, &user_b, Some(20), Some("full"), None, None, Some("collective"))
            .await
            .expect("collective search");
        ok = results.iter().any(|r| {
            r.content.contains(&token)
                && r.metadata
                    .get("user_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
                    >= 2
        });
        if ok {
            break;
        }
        last5 = results
            .iter()
            .take(5)
            .map(|r| {
                let uc = r
                    .metadata
                    .get("user_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                (r.id.clone(), uc, r.content.chars().take(80).collect())
            })
            .collect();
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    assert!(
        ok,
        "expected a memory from this run to reach user_count >= 2 within the \
         polling window (cross-user 'one fact, many knowers'); last 5: {last5:?}"
    );
}
