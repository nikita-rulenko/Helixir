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
    let message = format!(
        "E2E Hive shared fact token {token}: the canonical test statement is that Helixir Hive links duplicate cross-user facts to one memory node."
    );

    let user_a = format!("e2e_hive_{token}_a");
    let user_b = format!("e2e_hive_{token}_b");

    let client = HelixirClient::from_env().expect("HelixirClient::from_env");
    client.initialize().await.expect("initialize (health + ontology)");

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

    let results = client
        .search(
            &token,
            &user_b,
            Some(20),
            Some("full"),
            None,
            None,
            Some("collective"),
        )
        .await
        .expect("collective search");

    let matched: Vec<_> = results
        .iter()
        .filter(|r| r.id == mem_a)
        .map(|r| {
            let uc = r
                .metadata
                .get("user_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            (r.id.as_str(), uc, r.content.as_str())
        })
        .collect();

    let ok = matched.iter().any(|(_, uc, _)| *uc >= 2);

    if !ok {
        eprintln!("E2E Hive assertion failed.");
        eprintln!("  token: {token}");
        eprintln!("  mem_a (user_a first id): {mem_a}");
        eprintln!("  user_a memory_ids: {:?}", r_a.memory_ids);
        eprintln!("  user_b memory_ids: {:?}", r_b.memory_ids);
        eprintln!("  collective search hits for mem_a: {matched:?}");
        eprintln!("  first 5 collective results (id, user_count, content preview):");
        for r in results.iter().take(5) {
            let uc = r
                .metadata
                .get("user_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let prev: String = r.content.chars().take(80).collect();
            eprintln!("    {} user_count={} {}", r.id, uc, prev);
        }
    }

    assert!(
        ok,
        "expected collective search to show mem_a with user_count >= 2 after cross-user link; \
         see stderr for diagnostics (LLM may have chosen ADD instead of LINK_EXISTING)"
    );
}
