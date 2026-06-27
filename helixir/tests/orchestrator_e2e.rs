//! Orchestrator (#41 / Moira) integration smoke: the full choreography wires up
//! and completes. Clotho's tagging is LLM-driven (nondeterministic), so we assert
//! the structural invariants of a pass — it runs, the grow accounting is
//! consistent, and it returns insights — rather than exact content (that's the
//! capstone's job, on deterministic tags).
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test orchestrator_e2e -- --ignored --nocapture
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use helixir::agents::orchestrator::PassConfig;
use helixir::core::HelixirClient;

fn token() -> String {
    format!(
        "{:x}",
        SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos()
    )
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + LLM + Category schema deployed"]
async fn full_pass_runs_the_whole_choreography() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");

    let run = token();
    let user = format!("orch_{run}");
    for i in 0..4 {
        let fact = format!(
            "Run {run} note {i}: the deployment pipeline step {i} compiled and shipped cleanly."
        );
        client.add(&fact, &user, None, None).await.expect("add");
    }

    let cfg = PassConfig {
        max_seeds: 6,
        max_hops: 4,
        ..PassConfig::default()
    };
    let result = client
        .orchestrator()
        .full_pass(&user, &cfg)
        .await
        .expect("full_pass");

    let g = &result.grow;
    println!(
        "\n==== orchestrator_e2e ====\nscanned={} matched={} minted={} reused={} failed={} insights={}",
        g.scanned, g.tagged_by_match, g.minted, g.reused_mint, g.failed, result.insights.len()
    );

    // The pass scanned the user's corpus and accounted for every memory.
    assert!(g.scanned >= 1, "the pass scanned the corpus");
    assert!(
        g.tagged_by_match + g.minted + g.reused_mint + g.failed <= g.scanned,
        "grow buckets cannot exceed the scan"
    );
    // The grow stage must have actually CATEGORISED at least one memory. The
    // old test only checked `scanned>=1` and ignored `insights` entirely, so a
    // pass that scanned but tagged nothing (broken Clotho / empty category
    // dictionary / dead embedding tagger) passed silently. The four deployment
    // notes must earn at least one category between them.
    let categorised = g.tagged_by_match + g.minted + g.reused_mint;
    assert!(
        categorised >= 1,
        "the choreography must categorise ≥1 memory (scanned={}, failed={}, insights={}): \
         a scan that tags nothing means Clotho silently did no work",
        g.scanned,
        g.failed,
        result.insights.len()
    );
}
