//! #66 follow-up: ALIAS_OF — Clotho's vocabulary convergence.
//!
//! Weak models fragment the category dictionary with synonyms; fragmented
//! subsets blind Lachesis. Two near-duplicate categories must get an
//! ALIAS_OF edge (alias → canonical, canonical = lexicographically smaller
//! id), and a second pass must wire NOTHING — convergence, the flood lesson.

mod common;

use std::time::{SystemTime, UNIX_EPOCH};

use helixir::agents::clotho::Clotho;
use helixir::core::helixir_client::HelixirClient;

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
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings"]
async fn alias_pass_wires_synonyms_and_converges() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );

    let client = HelixirClient::from_env().expect("client from env");
    client.initialize().await.expect("initialize");
    let tooling = client.tooling();

    let run = token();
    // Same meaning, different surface — exactly what weak models mint.
    let name_a = format!("distributed tracing {run}");
    let name_b = format!("distributed tracing systems {run}");
    let desc = "collecting and correlating spans across services";
    let id_a = tooling
        .ensure_category(&name_a, "concept", desc)
        .await
        .expect("cat a");
    let id_b = tooling
        .ensure_category(&name_b, "concept", desc)
        .await
        .expect("cat b");
    assert_ne!(id_a, id_b, "two distinct categories must exist first");

    // Build the dict slice the pass operates on (id, name, embedding).
    let mut dict = Vec::new();
    for (id, name) in [(&id_a, &name_a), (&id_b, &name_b)] {
        let emb = tooling
            .embed_text(&format!("{name}: {desc}"))
            .await
            .expect("embed");
        dict.push((id.clone(), name.clone(), emb));
    }

    let clotho = Clotho::new(tooling);
    let wired = clotho.alias_pass(&dict).await;
    assert_eq!(wired, 1, "the synonym pair must get exactly one ALIAS_OF");

    // Ground truth: the alias (larger id) points at the canonical (smaller).
    let (canonical, alias) = if id_a < id_b {
        (&id_a, &id_b)
    } else {
        (&id_b, &id_a)
    };
    let edges = common::db_query(
        "getCategoryAliases",
        &serde_json::json!({"category_id": alias}),
    );
    let out = edges["aliases_out"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);
    assert_eq!(out, 1, "alias must carry one outgoing ALIAS_OF: {edges}");
    let target_ok = edges["aliases_out"][0]["category_id"]
        .as_str()
        .map(|c| c == canonical.as_str())
        .unwrap_or(false);
    assert!(target_ok, "ALIAS_OF must point at the canonical: {edges}");

    // Convergence: run again — nothing new.
    let wired2 = clotho.alias_pass(&dict).await;
    assert_eq!(wired2, 0, "second pass must wire nothing (idempotent)");

    println!("==== clotho_alias_e2e ==== wired=1 then 0; alias {alias} -> canonical {canonical}");
}
