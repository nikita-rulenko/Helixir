//! Lachesis end-to-end subset routing (#39 / Moira): route a cross-domain thread
//! over the subset-overlap graph, following high-PMI links and never the thick
//! axis. The generative move — "domain X reaches distant domain Z through this
//! chain of above-chance overlaps".
//!
//! Controlled scenario: a 3-link chain catX–catY–catZ (consecutive subsets share
//! a member, far above chance) plus a thick axis tagging everything (PMI ≈ 0).
//! Routing from catX must walk catX → catY → catZ and exclude the thick axis.
//! The PMI math is pinned in unit tests; this proves the routing assembles a
//! real multi-hop chain over live tags.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test lachesis_route_subsets_e2e -- --ignored --nocapture
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
async fn route_subsets_walks_high_pmi_chain_not_thick_axis() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");

    let run = token();
    let user = format!("lrs_{run}");

    // A modest universe of distinct facts (filler dilutes chance, so real
    // overlaps stand out as high PMI).
    let facts = [
        "The lighthouse keeper logged every passing freighter by hand.",
        "Quartz veins glittered where the old mine shaft had collapsed.",
        "The choir rehearsed the descant until the high notes rang clean.",
        "Yeast doubles the dough's volume in a warm proofing drawer.",
        "Migrating cranes rest on the floodplain each October.",
        "The kiln reached cone six before the glaze began to flow.",
        "Salt marshes buffer the coast against winter storm surges.",
        "The telescope's mirror was ground to a millionth of an inch.",
        "Cobblers once cut leather soles from a single hide.",
        "The aquifer recharges slowly through the limestone karst.",
        "Brass valves corrode faster in humid engine rooms.",
        "The vineyard's south slope ripens grapes a week early.",
    ];
    let mut m: Vec<String> = Vec::new();
    for f in facts {
        let r = client.add(f, &user, None, None).await.expect("add");
        if let Some(id) = r.memory_ids.first() {
            m.push(id.clone());
        }
    }
    assert!(m.len() >= 6, "need a universe, got {}", m.len());
    let universe = m.len();

    let cat = |n: &str| format!("{n}-{run}");
    let ens = |name: String| {
        let client = &client;
        async move { client.tooling().ensure_category(&name, "test", "").await.expect("ensure") }
    };
    let x = ens(cat("catX")).await;
    let y = ens(cat("catY")).await;
    let z = ens(cat("catZ")).await;
    let thick = ens(cat("thick")).await;

    // Chain: X∩Y = {m1}, Y∩Z = {m2}, X∩Z = {} (X and Z connect only through Y).
    let tag = |id: &str, cat: &str| {
        let client = &client;
        let id = id.to_string();
        let cat = cat.to_string();
        async move { client.tooling().tag_memory(&id, &cat, 90, "test").await.expect("tag") }
    };
    tag(&m[0], &x).await;
    tag(&m[1], &x).await;
    tag(&m[1], &y).await;
    tag(&m[2], &y).await;
    tag(&m[2], &z).await;
    tag(&m[3], &z).await;
    // Thick axis tags the whole universe → PMI ≈ 0 with everything.
    for id in &m {
        tag(id, &thick).await;
    }

    let candidates = vec![
        (x.clone(), "catX".to_string()),
        (y.clone(), "catY".to_string()),
        (z.clone(), "catZ".to_string()),
        (thick.clone(), "thick".to_string()),
    ];
    let hypo = client
        .lachesis()
        .route_subsets(&x, &candidates, universe, 4)
        .await
        .expect("route_subsets")
        .expect("a cross-domain subset thread should route from catX");

    println!("\n==== lachesis_route_subsets_e2e ====");
    println!("universe N={universe}  hops={}  min_pmi={:.4}", hypo.hops, hypo.min_pmi);
    for (i, s) in hypo.steps.iter().enumerate() {
        println!("  {i}. {} (pmi_from_prev={:.3})", s.category_name, s.pmi_from_prev);
    }

    let names: Vec<&str> = hypo.steps.iter().map(|s| s.category_name.as_str()).collect();
    // The thread is the high-PMI chain; the thick axis never carries the route.
    assert!(hypo.hops >= 2, "expected a multi-hop subset thread, got {}", hypo.hops);
    assert_eq!(names.first(), Some(&"catX"), "thread starts at the seed");
    assert!(names.contains(&"catY") && names.contains(&"catZ"), "chain runs X→Y→Z; got {names:?}");
    assert!(!names.contains(&"thick"), "the thick axis must be gated out; got {names:?}");
    assert!(hypo.min_pmi > 0.5, "every hop beats chance, weakest={}", hypo.min_pmi);
    assert!(hypo.requires_verification, "a generated connection is a hypothesis, never a verdict");
}
