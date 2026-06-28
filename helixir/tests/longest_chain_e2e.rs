//! #47 longest-chain context reconstruction — pull the single longest coherent
//! reasoning thread through a topic and narrate it hop by hop.
//!
//! Runs against the live `claude` dogfood cluster (the Moira/Helixir
//! development memories), which is densely woven with IMPLIES/BECAUSE/SUPPORTS/
//! CONTRADICTS edges (search_reasoning_chain reports deepest_chain ≈ 8). We
//! assert a multi-hop ordered thread comes back and print it — the elder-brain
//! replaying how an understanding came to be.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test longest_chain_e2e -- --ignored --nocapture
//! ```

use helixir::core::HelixirClient;

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB with a woven reasoning cluster"]
async fn longest_chain_reconstructs_a_thread() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");

    let narrative = client
        .longest_chain(
            "Moira critical path: relation density, Clotho, Lachesis, daemon, honesty kit",
            "claude",
            8,
        )
        .await
        .expect("longest_chain")
        .expect("a reasoning thread should exist in the claude cluster");

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
            &step.memory_id,
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
