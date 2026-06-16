//! Clotho auto-tagger (#33 / Moira) — the Spinner's core move proven live.
//!
//! Seed the controlled dictionary, then let embedding-match tag real memories:
//! an agricultural fact must earn `agriculture` (+ `raw material` by ancestor
//! propagation); a petrochemical fact must earn `petrochemicals` (+ `raw
//! material`); an off-domain fact must clear NOTHING at a high bar and instead
//! escalate per the charter (no silent category invention).
//!
//! The test also OBSERVES (prints, does not hard-assert) whether the two
//! domain facts, now sharing `raw material` purely via auto-tags, bridge through
//! `connect_memories`. That bridge is real but its determinism depends on
//! hub-degree caps over a broad shared category (#15) — so it's a signal here,
//! not a gate.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test clotho_autotag_e2e -- --ignored --nocapture
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
#[ignore = "needs HELIX_E2E=1 + live HelixDB + Category schema deployed + ollama embed"]
async fn clotho_autotag_dictionary() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");

    let run = token();
    let user = format!("clotho_{run}");

    // 1) Seed the controlled vocabulary (idempotent — shared global dictionary).
    let seeded = client
        .clotho()
        .seed_dictionary()
        .await
        .expect("seed_dictionary");
    println!("\n==== clotho_autotag_e2e ====");
    println!("seeded {seeded} categories");

    // 2) Two on-domain facts + one off-domain control.
    let agri =
        "After the monsoon rains this season, farmers across the region harvested a record grain crop.";
    let petro = "Hydraulic fracturing of shale wells relies on petrochemical fluid additives \
                 whose price drives drilling costs.";
    let arts = "The violin concerto moved the entire audience to tears at last night's premiere.";

    let a = client.add(agri, &user, None, None).await.expect("add agri");
    let b = client.add(petro, &user, None, None).await.expect("add petro");
    let c = client.add(arts, &user, None, None).await.expect("add arts");

    // 3) Auto-tag by embedding match. Tag every produced memory with its own
    //    stored fact text. Calibrated bar: on-target cosine ≈ 0.74, off-target
    //    ≤ 0.57 (nomic-embed-text), so 0.65 cleanly isolates the right domain.
    let bar = 0.65;
    let mut agri_tags: Vec<String> = Vec::new();
    let mut petro_tags: Vec<String> = Vec::new();
    for (label, fact, ids, sink) in [
        ("agri", agri, &a.memory_ids, &mut agri_tags),
        ("petro", petro, &b.memory_ids, &mut petro_tags),
    ] {
        for id in ids {
            let outcome = client
                .clotho()
                .auto_tag(id, fact, 10, bar)
                .await
                .expect("auto_tag");
            println!(
                "[{label}] {id}: tagged {:?}",
                outcome
                    .tagged
                    .iter()
                    .map(|h| format!("{}={:.3}", h.name, h.score))
                    .collect::<Vec<_>>()
            );
            sink.extend(outcome.tagged.into_iter().map(|h| h.name));
        }
    }

    // Off-domain control at the same bar — no dictionary domain fits, so expect
    // an escalation (charter), not a silent tag.
    let mut arts_escalated = false;
    for id in &c.memory_ids {
        let outcome = client
            .clotho()
            .auto_tag(id, arts, 10, bar)
            .await
            .expect("auto_tag");
        println!(
            "[arts] {id}: tagged {:?} escalation={}",
            outcome.tagged.iter().map(|h| h.name.clone()).collect::<Vec<_>>(),
            outcome.escalation.is_some()
        );
        if outcome.escalation.is_some() {
            arts_escalated = true;
        }
    }

    // 4) OBSERVE the cross-domain bridge on auto-tags (not a gate — see header).
    let bridge = client
        .connect_memories(
            "monsoon grain harvest crop yield",
            "shale fracking petrochemical fluid additive cost",
            &user,
            Some(4),
        )
        .await
        .expect("connect_memories");
    println!(
        "bridge-on-autotags: found={} shared_seed={} hops={}",
        bridge.found, bridge.shared_seed, bridge.hops
    );

    // ---- Assertions (the deterministic core) ----
    // Right domain earned, by measurement.
    assert!(
        agri_tags.iter().any(|n| n == "agriculture"),
        "agricultural fact should earn 'agriculture' at bar {bar}; got {agri_tags:?}"
    );
    assert!(
        petro_tags.iter().any(|n| n == "petrochemicals"),
        "petrochemical fact should earn 'petrochemicals' at bar {bar}; got {petro_tags:?}"
    );
    // Ancestor propagation up the hierarchy — the shared jump-plane both inherit.
    assert!(
        agri_tags.iter().any(|n| n == "raw material")
            && petro_tags.iter().any(|n| n == "raw material"),
        "both should inherit 'raw material' via SUBCATEGORY_OF propagation; \
         agri={agri_tags:?} petro={petro_tags:?}"
    );
    // Precision: the bar excludes the OTHER domain — not a tag-everything pass.
    assert!(
        !agri_tags.iter().any(|n| n == "petrochemicals" || n == "finance"),
        "agri fact should NOT cross-tag petrochemicals/finance at bar {bar}; got {agri_tags:?}"
    );
    assert!(
        !petro_tags.iter().any(|n| n == "agriculture"),
        "petro fact should NOT cross-tag agriculture at bar {bar}; got {petro_tags:?}"
    );
    // Unknown domain escalates rather than mis-tagging.
    assert!(
        arts_escalated,
        "off-domain fact should escalate (no category over bar {bar}), not tag silently"
    );
}
