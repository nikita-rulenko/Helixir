//! #83 increment 2: retroactive causal stitching (Lachesis).
//!
//! Seeds two causally-related atoms through the PREPARED path (which builds
//! no cross-atom causal edges — the exact "old memories, invisible relation"
//! situation), waits for background entity enrichment to give the pair its
//! shared-entity signal, then runs one stitch pass and checks DB ground
//! truth: a BECAUSE edge from effect to cause, tagged lachesis-stitch. A
//! second pass must persist nothing (linked pairs are skipped) — the
//! convergence property that keeps the duty flood-safe.

mod common;

use std::time::{SystemTime, UNIX_EPOCH};

use common::db_edge_types_out;
use helixir::agents::lachesis::stitch::Stitcher;
use helixir::core::helixir_client::HelixirClient;
use helixir::llm::extractor::ExtractedMemory;

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
async fn stitch_builds_because_across_old_memories() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );

    let client = HelixirClient::from_env().expect("client from env");
    client.initialize().await.expect("initialize");
    let tooling = client.tooling();

    let run = token();
    let user = format!("stitch_{run}");
    let atoms: Vec<ExtractedMemory> = [
        format!("The bearing on turbine_{run} overheated and seized during the night shift."),
        format!("Turbine_{run} tripped offline at dawn when the seized bearing jammed its shaft."),
    ]
    .into_iter()
    .map(|text| ExtractedMemory {
        text,
        memory_type: "fact".to_string(),
        certainty: 90,
        importance: 50,
        entities: vec![],
        context: None,
    })
    .collect();

    let r = client
        .add_prepared(atoms, &user, None, Some("stitch-e2e"))
        .await
        .expect("prepared add");
    assert_eq!(r.memories_added, 2, "two atoms expected: {r:?}");
    let (a, b) = (r.memory_ids[0].clone(), r.memory_ids[1].clone());

    // The prepared path must NOT have linked them causally — that's the
    // precondition that makes this retroactive.
    assert!(
        !db_edge_types_out(&a).contains(&"BECAUSE".to_string())
            && !db_edge_types_out(&b).contains(&"BECAUSE".to_string()),
        "precondition: no causal edge before the stitch pass"
    );

    // Wire the shared-entity signal directly (the prepared path carries no
    // entities and spawns no enrichment — entity LINKING is another path's
    // test; this suite isolates candidate discovery + judge + persist).
    let entity_id = format!("ent_turbine_{run}");
    common::db_query(
        "createEntity",
        &serde_json::json!({
            "entity_id": &entity_id,
            "name": format!("turbine_{run}"),
            "entity_type": "equipment",
            "properties": "{}",
            "aliases": "",
        }),
    );
    for mid in [&a, &b] {
        common::db_query(
            "linkExtractedEntity",
            &serde_json::json!({
                "memory_id": mid,
                "entity_id": &entity_id,
                "confidence": 90i64,
                "method": "stitch-e2e",
            }),
        );
    }

    // Pass 1: the stitch must find the pair, judge it causal, persist
    // BECAUSE. The judge is one LLM call — probabilistic — so one retry is
    // allowed before the assertion (same posture as the 3a dedup retry).
    let mut stats = Stitcher::new(tooling)
        .stitch_pass(&user)
        .await
        .expect("stitch pass 1");
    if stats.persisted == 0 {
        stats = Stitcher::new(tooling)
            .stitch_pass(&user)
            .await
            .expect("stitch pass 1 retry");
    }
    assert!(
        stats.persisted >= 1,
        "stitch must persist a causal edge for an explicit overheat->trip pair: {stats:?}"
    );

    let a_out = db_edge_types_out(&a);
    let b_out = db_edge_types_out(&b);
    assert!(
        a_out.contains(&"BECAUSE".to_string()) || b_out.contains(&"BECAUSE".to_string()),
        "a BECAUSE edge must exist between the pair (a: {a_out:?}, b: {b_out:?})"
    );

    // Pass 2: convergence — the pair is linked now, nothing new may persist.
    let stats2 = Stitcher::new(tooling)
        .stitch_pass(&user)
        .await
        .expect("stitch pass 2");
    assert_eq!(
        stats2.persisted, 0,
        "second pass must skip the already-linked pair: {stats2:?}"
    );
    assert!(
        stats2.skipped_linked >= 1,
        "the pair must be counted as skipped_linked on pass 2: {stats2:?}"
    );

    println!(
        "==== stitch_e2e ==== pass1: {stats:?} | pass2: {stats2:?} — retroactive BECAUSE built, convergent"
    );
}
