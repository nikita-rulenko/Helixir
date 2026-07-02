//! #33 third-dimension routing — a cross-domain bridge via a shared CATEGORY
//! (the Clotho mechanism), proved deterministically.
//!
//! Two memories from different domains, embedding-DISSIMILAR and sharing NO
//! stated reasoning edge, must become connectable once both are `TAGGED_AS` the
//! same Category. `connect_memories` then routes through the shared node —
//! `Memory -TAGGED_AS-> Category -In TAGGED_AS-> Memory` — a jump no single
//! memory ever stated. We tag EXPLICITLY (no LLM entity extraction in the path),
//! so the outcome is predictable: the bridge exists iff routing works.
//!
//! Contrast with similarity-only candidate selection, which can never surface
//! this pair. The category is unique to the run, so the bridge is unambiguous
//! and the test cannot be polluted by other users' data.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test cross_domain_bridge_e2e -- --ignored --nocapture
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
async fn cross_domain_bridge_via_category() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");

    let run = token();
    let user = format!("xcat_{run}");
    // Deliberately different domains and vocabularies — cosine similarity alone
    // never links a monsoon to a fracking invoice.
    let fact_a =
        "Rajasthan saw an unusually strong monsoon this year, lifting the regional grain harvest.";
    let fact_b = "Shale well-completion costs rose this quarter as fracking-fluid additives \
                  became more expensive.";

    let a = client.add(fact_a, &user, None, None).await.expect("add A");
    let b = client.add(fact_b, &user, None, None).await.expect("add B");
    assert!(!a.memory_ids.is_empty(), "fact A produced no memory");
    assert!(!b.memory_ids.is_empty(), "fact B produced no memory");

    // Clotho substrate: ONE shared category, unique to this run so the bridge is
    // unambiguous. Tag every fact each side produced — the perpendicular axis the
    // two memories now share.
    let cat = client
        .tooling()
        .ensure_category(
            &format!("raw-material-{run}"),
            "domain",
            "primary commodities and feedstocks (guar, grain, hydrocarbons)",
        )
        .await
        .expect("ensure_category");
    for id in a.memory_ids.iter().chain(b.memory_ids.iter()) {
        client
            .tooling()
            .tag_memory(id, &cat, 80, "test")
            .await
            .expect("tag_memory");
    }

    // Connect by memory id (#59) so the test asserts the ROUTING, not the
    // search-anchor lottery: both endpoints are known, the only question is
    // whether the shared Category bridges them.
    let res = client
        .connect_memories(&a.memory_ids[0], &b.memory_ids[0], &user, Some(4))
        .await
        .expect("connect_memories");

    println!("\n==== cross_domain_bridge_via_category ====");
    println!(
        "a_ids={:?} b_ids={:?} cat={cat}",
        a.memory_ids, b.memory_ids
    );
    println!(
        "connect: found={} shared_seed={} hops={}",
        res.found, res.shared_seed, res.hops
    );

    assert!(
        res.found && !res.shared_seed && res.hops >= 1,
        "expected a category-routed cross-domain path (found && !shared_seed && hops>=1); \
         got found={} shared_seed={} hops={}",
        res.found,
        res.shared_seed,
        res.hops
    );
}
