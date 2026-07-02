//! Lachesis coherence gate (#39 / Moira) end-to-end: route a reasoning chain
//! between two memories and gate it against apophenia.
//!
//! Hermetic: seeds two DISTINCT facts and a real BECAUSE edge between them
//! (directly, via `record_causation` — no LLM atomization, no ambient-cluster
//! drift), then routes BY ID so anchor resolution is deterministic (#59). The
//! gate's discrimination logic is also proven in the module's unit tests; this
//! confirms it runs against a real routed reasoning chain.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test lachesis_gate_e2e -- --ignored --nocapture
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use helixir::agents::lachesis::EpistemicLabel;
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
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + working LLM"]
async fn lachesis_gates_a_routed_chain() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");

    // Two clearly-distinct single facts — each must store exactly its own memory
    // (no dedup, no multi-split), so the BECAUSE edge we seed connects the two
    // anchors we route between. (A fragile seed where one add returns a
    // different/empty id was the real cause of this suite's old flakiness — the
    // routing itself is fine.)
    let run = token();
    let user = format!("lachesis_gate_{run}");
    let ra = client
        .add(
            &format!("Service nova{run} adopted gRPC for its transport layer."),
            &user,
            None,
            None,
        )
        .await
        .expect("add A");
    let rb = client
        .add(
            &format!("The nova{run} REST gateway was far too slow during peak traffic."),
            &user,
            None,
            None,
        )
        .await
        .expect("add B");
    let id_a = ra
        .memory_ids
        .first()
        .cloned()
        .expect("fact A must store exactly one memory");
    let id_b = rb
        .memory_ids
        .first()
        .cloned()
        .expect("fact B must store exactly one memory");
    // Strong edge (strength ≈ 0..100 → weight 0..1): a confident causal link,
    // like the ones the extractor mints. A weak strength would drag coherence
    // below the gate's bar and the chain would (correctly) read as apophenia.
    client
        .tooling()
        .record_causation(&id_a, &id_b, 90)
        .await
        .expect("seed BECAUSE edge");

    let hypo = client
        .lachesis()
        .route(&id_a, &id_b, &user, 5)
        .await
        .expect("route")
        .expect("the seeded causal pair must connect through the BECAUSE edge");

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
    assert!(
        (0.0..=1.0).contains(&v.reasoning_support),
        "support in [0,1]"
    );
    assert_eq!(
        v.requires_verification,
        v.label == EpistemicLabel::PlausibleHypothesis,
        "a hypothesis (and only a hypothesis) carries the verification flag — never a verdict"
    );

    // The seeded path is a single typed BECAUSE hop → reasoning-backed, and the
    // gate must pass it as a hypothesis (not dismiss it as apophenia).
    assert!(
        v.reasoning_support >= 0.5,
        "a BECAUSE-backed chain must be reasoning-supported, got {}",
        v.reasoning_support
    );
    assert_eq!(
        v.label,
        EpistemicLabel::PlausibleHypothesis,
        "a coherent reasoning-backed chain should pass the gate"
    );
}
