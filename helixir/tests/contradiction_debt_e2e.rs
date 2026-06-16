//! Contradiction-debt reconciliation (#45): the Cutter drains dead cross-user
//! disputes so `resolved=0` edges don't grow unboundedly as the collective
//! scales. Seeds two open disputes on distinct from-memories — one preference
//! (drainable as coexistence), one factual (a live disagreement to keep) — then
//! runs Atropos::reconcile and asserts the preference was retired and the
//! factual remains open. Idempotent: a second pass drains nothing new.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test contradiction_debt_e2e -- --ignored --nocapture
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use helixir::core::HelixirClient;

fn token() -> String {
    format!(
        "{:x}",
        SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos()
    )
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + LLM + the #45 contradiction queries deployed"]
async fn reconcile_drains_preference_keeps_factual() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");
    let tooling = client.tooling();

    let run = token();
    let user = format!("debt_{run}");

    // Four memories on distinct topics → four real Memory nodes.
    async fn first_id(client: &HelixirClient, text: &str, user: &str) -> String {
        client
            .add(text, user, None, None)
            .await
            .expect("add")
            .memory_ids
            .into_iter()
            .next()
            .expect("a memory id")
    }
    // Four mutually DISSIMILAR facts (distinct domains) so none dedups against
    // another (the W2 cosine gate NOOPs near-duplicates). The dispute content is
    // irrelevant — reconcile keys off the edge's strategy, which we seed below.
    let m1 = first_id(&client, &format!("Service debt{run}a uses LRU cache eviction."), &user).await;
    let m2 = first_id(&client, &format!("The debt{run}b harvest festival is held in October."), &user).await;
    let m3 = first_id(&client, &format!("Planet debt{run}c orbits a red dwarf star."), &user).await;
    let m4 = first_id(&client, &format!("Chef debt{run}d perfected a rye sourdough recipe."), &user).await;

    // Seed two open disputes on SEPARATE from-memories so grouping is clean:
    // m1→m2 a preference (drainable), m3→m4 a factual claim (kept).
    tooling
        .record_contradiction(&m1, &m2, "cross_user_preference")
        .await
        .expect("seed preference");
    tooling
        .record_contradiction(&m3, &m4, "cross_user_factual")
        .await
        .expect("seed factual");

    // Both surface as open debt.
    let open = tooling
        .gather_open_contradictions(&user, 500)
        .await
        .expect("gather");
    assert!(
        open.iter().any(|o| o.from_id == m1),
        "preference dispute m1→m2 must be open: {open:?}"
    );
    assert!(
        open.iter().any(|o| o.from_id == m3),
        "factual dispute m3→m4 must be open: {open:?}"
    );

    // Reconcile: preference retired, factual kept.
    let s = client.atropos().reconcile(&user, 500).await.expect("reconcile");
    assert!(s.scanned >= 2, "should scan both seeded disputes: {s:?}");
    assert!(s.drained_preference >= 1, "preference must drain: {s:?}");
    assert!(s.kept_live >= 1, "factual must be kept live: {s:?}");

    // The preference is gone from the worklist; the factual remains open.
    let after = tooling
        .gather_open_contradictions(&user, 500)
        .await
        .expect("gather after");
    assert!(
        !after.iter().any(|o| o.from_id == m1),
        "preference m1→m2 must be drained after reconcile: {after:?}"
    );
    assert!(
        after.iter().any(|o| o.from_id == m3),
        "factual m3→m4 must still be open: {after:?}"
    );

    // Idempotent: a second pass drains nothing new (no live preference left).
    let s2 = client.atropos().reconcile(&user, 500).await.expect("reconcile 2");
    assert_eq!(s2.drained_preference, 0, "nothing new to drain: {s2:?}");

    println!("\n==== contradiction_debt_e2e ====");
    println!(
        "scanned {}, drained {} preference, kept {} live; after: {} open dispute(s)",
        s.scanned, s.drained_preference, s.kept_live, after.len()
    );
}
