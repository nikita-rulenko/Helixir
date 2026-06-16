//! Lachesis PMI subset-overlap routing (#39 / Moira) — the cross-domain
//! apophenia guard, live.
//!
//! Builds a controlled scenario and proves the core property: a SPECIFIC
//! co-occurrence (two narrow categories that share members) scores high PMI,
//! while a THICK axis (a category covering the whole universe) is damped to ≈0
//! — it gates itself out, no LLM needed. This is what lets Lachesis route real
//! cross-domain insights without becoming a bullshit generator.
//!
//! The PMI math is pinned deterministically in the module unit tests; this runs
//! it end-to-end over live tags.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test lachesis_pmi_e2e -- --ignored --nocapture
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use helixir::core::HelixirClient;

fn token() -> String {
    format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + Category schema deployed"]
async fn pmi_damps_thick_axis_lifts_specific() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");

    let run = token();
    let user = format!("lpmi_{run}");

    // A small universe of distinct facts.
    let facts = [
        "The harbour ferry runs every twenty minutes on weekday mornings.",
        "Basalt columns formed as the lava cooled slowly over centuries.",
        "The orchestra tuned to the oboe's A before the overture began.",
        "Sourdough starter needs daily feeding to stay active.",
        "The comet will not return to the inner system for 4000 years.",
        "Tin solder melts at a lower temperature than the copper it joins.",
    ];
    let mut mids: Vec<String> = Vec::new();
    for f in facts {
        let r = client.add(f, &user, None, None).await.expect("add");
        if let Some(id) = r.memory_ids.first() {
            mids.push(id.clone());
        }
    }
    assert!(mids.len() >= 4, "need a few memories, got {}", mids.len());
    let universe = mids.len();

    // Run-unique categories so their member sets are exactly this run's tags.
    let cat = |name: &str| format!("{name}-{run}");
    let cat_a = client.tooling().ensure_category(&cat("alpha"), "test", "").await.expect("cat a");
    let cat_b = client.tooling().ensure_category(&cat("beta"), "test", "").await.expect("cat b");
    let cat_thick = client.tooling().ensure_category(&cat("thick"), "test", "").await.expect("cat thick");

    // SPECIFIC pair: alpha and beta both tag exactly the first two memories.
    for id in &mids[..2] {
        client.tooling().tag_memory(id, &cat_a, 90, "test").await.expect("tag a");
        client.tooling().tag_memory(id, &cat_b, 90, "test").await.expect("tag b");
    }
    // THICK axis: tags the whole universe (like raw-material covering everything).
    for id in &mids {
        client.tooling().tag_memory(id, &cat_thick, 90, "test").await.expect("tag thick");
    }

    let lachesis = client.lachesis();
    let specific = lachesis.subset_pmi(&cat_a, &cat_b, universe).await.expect("pmi specific");
    let thick = lachesis.subset_pmi(&cat_a, &cat_thick, universe).await.expect("pmi thick");

    println!("\n==== lachesis_pmi_e2e ====");
    println!("universe N={universe}");
    println!("PMI(alpha, beta)  specific co-occurrence = {specific:.4}");
    println!("PMI(alpha, thick) thick axis             = {thick:.4}");

    // The thick axis covers the whole universe → co-occurrence is exactly chance
    // → PMI ≈ 0: it gates itself out.
    assert!(
        thick.abs() < 1e-6,
        "thick axis should damp to ~0 (chance co-occurrence), got {thick}"
    );
    // The specific pair co-occurs far above chance → clearly positive, and well
    // above the thick axis. This is the signal Lachesis routes on.
    assert!(
        specific > 0.5 && specific > thick,
        "specific co-occurrence should lift above chance and beat the thick axis \
         (specific={specific}, thick={thick})"
    );
}
