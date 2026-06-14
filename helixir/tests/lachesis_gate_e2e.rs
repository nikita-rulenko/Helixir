//! Lachesis coherence gate (#39 / Moira) end-to-end: route a chain between two
//! topics and gate it against apophenia.
//!
//! Runs over the live `claude`/Moira cluster, which is reasoning-connected
//! (IMPLIES/BECAUSE/MEMORY_RELATION). A pair drawn from it should route to a
//! reasoning-backed path and the gate should label it a PlausibleHypothesis —
//! flagged "requires verification", never asserted as truth.
//!
//! The gate's discrimination logic (hypothesis vs apophenia) is proven
//! deterministically in the module's unit tests; this confirms it runs against
//! real routed chains.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test lachesis_gate_e2e -- --ignored --nocapture
//! ```

use helixir::agents::lachesis::EpistemicLabel;
use helixir::core::HelixirClient;

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB with a reasoning-connected cluster"]
async fn lachesis_gates_a_routed_chain() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");

    let hypo = client
        .lachesis()
        .route(
            "liveness oracle and the cross-user consensus issue #43",
            "Moira critical path step one: relation density #33",
            "claude",
            5,
        )
        .await
        .expect("route")
        .expect("the two topics should connect in the woven claude cluster");

    let v = &hypo.verdict;
    println!("\n==== lachesis_gate_e2e ====");
    println!(
        "found chain: {} hops, conf {:.4}",
        hypo.path.hops, hypo.path.confidence
    );
    for (i, edge) in hypo.path.edges.iter().enumerate() {
        println!("  hop {i}: {} (w={:.2})", edge.edge_type, edge.weight);
    }
    println!(
        "verdict: {:?} | coherence={:.3} reasoning_support={:.2} requires_verification={}",
        v.label, v.coherence, v.reasoning_support, v.requires_verification
    );
    println!("reason: {}", v.reason);

    // Structural invariants of the verdict.
    assert!((0.0..=1.0).contains(&v.coherence), "coherence in [0,1]");
    assert!((0.0..=1.0).contains(&v.reasoning_support), "support in [0,1]");
    assert_eq!(
        v.requires_verification,
        v.label == EpistemicLabel::PlausibleHypothesis,
        "a hypothesis (and only a hypothesis) carries the verification flag — never a verdict"
    );

    // A reasoning-connected pair must route through typed reasoning, not bare
    // association, and clear the gate.
    assert!(
        v.reasoning_support >= 0.5,
        "the Moira cluster path should be reasoning-backed, got {}",
        v.reasoning_support
    );
    assert_eq!(
        v.label,
        EpistemicLabel::PlausibleHypothesis,
        "a coherent reasoning-backed chain should pass the gate"
    );
}
