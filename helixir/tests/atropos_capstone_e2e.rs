//! Capstone (#48 / Moira): signal on clean data. The whole session built the
//! generative pipeline; the live runs showed it faithfully curates whatever the
//! tags say — garbage on a noise-polluted corpus. This proves the converse:
//! given CLEAN tags, Atropos surfaces the ONE real cross-domain thread and
//! nothing else.
//!
//! The guar chain in miniature, tagged deterministically over run-unique
//! categories (so the global corpus's noise can't leak in): weather → crop →
//! thickener → fracking → price, each consecutive pair bridged by a memory that
//! genuinely spans both domains (guar gum is BOTH a food thickener AND a
//! fracking additive). Atropos must route and curate exactly that thread,
//! ranked, deduped, with real witnesses, as a hypothesis-requiring-verification.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test atropos_capstone_e2e -- --ignored --nocapture
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
async fn atropos_surfaces_the_one_real_thread() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");

    let run = token();
    let user = format!("cap_{run}");

    // Four crisp bridge facts — each spans two adjacent domains. The novel
    // entity `guarbridge{run}` is woven into the CLAIM (not just a prefix) so the
    // extracted facts stay novel and don't dedup against the bench corpus.
    let g = format!("guarbridge{run}");
    let facts = [
        format!("Heavy monsoon rain produced a bumper {g} bean harvest on the farms."),
        format!("Food makers mill {g} beans into a gum that thickens sauces and ice cream."),
        format!("Oilfield crews pump {g} gum to gel the fluid in shale fracking wells."),
        format!("Rising {g} additive costs pushed the drilling firm's share price higher."),
    ];
    let mut m: Vec<String> = Vec::new();
    for (i, f) in facts.iter().enumerate() {
        let r = client.add(f, &user, None, None).await.expect("add");
        let id = r
            .memory_ids
            .first()
            .unwrap_or_else(|| panic!("fact {i} produced no memory: {f}"));
        m.push(id.clone());
    }

    // Run-unique categories — their member sets are exactly this test's tags, so
    // the polluted global graph can't contaminate the routing.
    let cat = |n: &str| format!("{n}-{run}");
    let ens = |name: String| {
        let client = &client;
        async move { client.tooling().ensure_category(&name, "domain", "").await.expect("cat") }
    };
    let weather = ens(cat("weather")).await;
    let crop = ens(cat("crop")).await;
    let thickener = ens(cat("thickener")).await;
    let fracking = ens(cat("fracking")).await;
    let price = ens(cat("price")).await;

    // Tag so consecutive domains overlap on the spanning memory — the bridges.
    let tag = |mid: &str, cid: &str| {
        let client = &client;
        let (mid, cid) = (mid.to_string(), cid.to_string());
        async move { client.tooling().tag_memory(&mid, &cid, 90, "test").await.expect("tag") }
    };
    tag(&m[0], &weather).await;
    tag(&m[0], &crop).await; // monsoon ⋂ crop
    tag(&m[1], &crop).await;
    tag(&m[1], &thickener).await; // crop ⋂ thickener (guar gum from beans)
    tag(&m[2], &thickener).await;
    tag(&m[2], &fracking).await; // thickener ⋂ fracking (the cross-domain bridge)
    tag(&m[3], &fracking).await;
    tag(&m[3], &price).await; // fracking ⋂ price

    // A universe large enough that single overlaps read as above-chance.
    let universe = 20;
    let candidates = vec![
        (weather.clone(), "weather".to_string()),
        (crop.clone(), "crop".to_string()),
        (thickener.clone(), "thickener".to_string()),
        (fracking.clone(), "fracking".to_string()),
        (price.clone(), "price".to_string()),
    ];

    let insights = client
        .atropos()
        .curate(&candidates, &candidates, universe, 5)
        .await
        .expect("curate");

    println!("\n==== atropos_capstone_e2e ====");
    for ins in &insights {
        println!(
            "★ value {:.2} [{} hops, min PMI {:.2}, {}] {}",
            ins.value,
            ins.hops,
            ins.min_pmi,
            ins.status,
            ins.category_path.join(" → ")
        );
        for w in &ins.witnesses {
            println!("    · {} :: {}", w.link, w.snippet);
        }
    }

    // Exactly one insight: the sub-threads are subsumed into the full chain.
    assert_eq!(insights.len(), 1, "the one real thread, deduped; got {}", insights.len());
    let top = &insights[0];
    assert_eq!(top.hops, 4, "weather→crop→thickener→fracking→price is 4 hops");
    let got: std::collections::HashSet<&str> =
        top.category_path.iter().map(String::as_str).collect();
    let want: std::collections::HashSet<&str> =
        ["weather", "crop", "thickener", "fracking", "price"].into_iter().collect();
    assert_eq!(got, want, "the thread spans exactly the guar chain; got {:?}", top.category_path);
    assert!(top.requires_verification, "an insight is a hypothesis, never a verdict");
    assert_eq!(top.status, "proposed");
    // Provenance is real: every hop is witnessed by a spanning memory.
    assert_eq!(top.witnesses.len(), 4, "one witness bridging each hop");
}
