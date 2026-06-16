//! Clotho dominance gate (#39 / Moira): tagging quality — a memory belongs to
//! its top domain(s), not to everything that grazes the threshold.
//!
//! The provenance drill showed the corpus's "cross-domain" bridges were woven by
//! noise-floor cross-tags (a dev memory tagged `food industry`). The dominance
//! gate fixes that: tag only categories within a margin of the best match. Here
//! a clearly-software memory and a clearly-agricultural one go through a grow
//! pass and must end up sharing NO category — no spurious cross-tag, so no
//! chimeric bridge can form between them.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test clotho_dominance_e2e -- --ignored --nocapture
//! ```

use std::collections::HashSet;
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
#[ignore = "needs HELIX_E2E=1 + live HelixDB + Category schema deployed + LLM"]
async fn dominance_gate_keeps_domains_disjoint() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");
    // Ensure the dictionary has the broad domains both facts could grab at the
    // noise floor (agriculture/raw-material/technology), so the gate has
    // something to suppress.
    client.clotho().seed_dictionary().await.expect("seed");

    let run = token();
    let user = format!("dom_{run}");

    let sw = "The repository layer dispatches HTTP handlers through the Chi router in the Go service.";
    let ag = "Farmers harvested a record grain crop this season after the strong monsoon rains.";
    let s = client.add(sw, &user, None, None).await.expect("add sw");
    let a = client.add(ag, &user, None, None).await.expect("add ag");
    let s_id = s.memory_ids.first().expect("sw id").clone();
    let a_id = a.memory_ids.first().expect("ag id").clone();

    // Grow pass with the dominance gate.
    let stats = client
        .clotho()
        .grow_pass(
            &[(s_id.clone(), sw.to_string()), (a_id.clone(), ag.to_string())],
            0.62,
        )
        .await
        .expect("grow_pass");
    println!(
        "\n==== clotho_dominance_e2e ====\ngrow: matched={} minted={} reused={}",
        stats.tagged_by_match, stats.minted, stats.reused_mint
    );

    let s_cats: HashSet<String> = client
        .tooling()
        .memory_category_names(&s_id)
        .await
        .expect("sw cats")
        .into_iter()
        .collect();
    let a_cats: HashSet<String> = client
        .tooling()
        .memory_category_names(&a_id)
        .await
        .expect("ag cats")
        .into_iter()
        .collect();
    println!("software memory categories : {s_cats:?}");
    println!("agriculture memory categories: {a_cats:?}");

    let shared: Vec<&String> = s_cats.intersection(&a_cats).collect();
    println!("shared categories: {shared:?}");

    // Each memory got at least one category (no dead end).
    assert!(!s_cats.is_empty(), "software memory should be tagged");
    assert!(!a_cats.is_empty(), "agriculture memory should be tagged");
    // The point: two clearly-different domains must NOT share a category — that
    // shared tag is the noise that would weave a chimeric cross-domain bridge.
    assert!(
        shared.is_empty(),
        "a software fact and an agriculture fact must share no category after the \
         dominance gate; shared={shared:?}"
    );
}
