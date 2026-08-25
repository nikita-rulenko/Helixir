//! #47 longest-chain context reconstruction — pull the single longest coherent
//! reasoning thread through a topic and narrate it hop by hop.
//!
//! Seeds the deterministic golden corpus into the disposable current-schema
//! database. Its payments thread contains three typed reasoning hops, so the
//! release gate proves reconstruction without depending on a developer's
//! private dogfood memories.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test longest_chain_e2e -- --ignored --nocapture
//! ```

use helixir::core::HelixirClient;

mod common;
use common::golden::{GOLDEN_USER, ensure_seeded};

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings; self-seeds golden graph"]
async fn longest_chain_reconstructs_a_thread() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");
    let actor = common::e2e_actor();
    ensure_seeded(&client).await;

    let narrative = client
        .longest_chain_as(
            &actor,
            "payments sqlite migration checkout latency postgres standardization",
            GOLDEN_USER,
            8,
        )
        .await
        .expect("longest_chain")
        .expect("the deterministic golden payments thread should exist");

    println!("\n==== longest_chain_e2e ====");
    println!(
        "hops={} confidence={:.4}",
        narrative.hops, narrative.confidence
    );
    for (i, step) in narrative.steps.iter().enumerate() {
        let edge = step
            .edge_type
            .as_deref()
            .map(|t| format!(" --[{t} w={:.2}]-->", step.edge_weight))
            .unwrap_or_default();
        println!(
            "  {i}.{edge} [{}] {}",
            step.memory_id,
            step.content.chars().take(90).collect::<String>()
        );
    }

    // The thread must be ordered: exactly one edge between consecutive steps,
    // and only the first step lacks an incoming edge.
    assert!(
        narrative.hops >= 3,
        "expected a multi-hop thread, got {}",
        narrative.hops
    );
    assert_eq!(narrative.steps.len(), narrative.hops + 1);
    assert!(
        narrative.steps[0].edge_type.is_none(),
        "first step has no incoming edge"
    );
    assert!(
        narrative.steps[1..].iter().all(|s| s.edge_type.is_some()),
        "every step after the first carries the edge it arrived by"
    );
    // No memory repeats — it's a simple path.
    let unique: std::collections::HashSet<_> =
        narrative.steps.iter().map(|s| &s.memory_id).collect();
    assert_eq!(
        unique.len(),
        narrative.steps.len(),
        "thread must be a simple path"
    );
    // Confidence is a real product of weights in (0, 1].
    assert!(
        narrative.confidence > 0.0 && narrative.confidence <= 1.0,
        "confidence {} should be a weight product in (0,1]",
        narrative.confidence
    );
}
