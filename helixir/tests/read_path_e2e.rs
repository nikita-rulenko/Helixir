//! Read-path end-to-end suite: every read tool the `local-reasoning` branch
//! touched, exercised against a live HelixDB with the bench corpus loaded.
//!
//! **Not run by default** (`#[ignore]`). Requires live infrastructure:
//! - `HELIX_HOST` / `HELIX_PORT` — HelixDB with the bench corpus (user `bench`)
//! - embedding env vars (ollama) — reads need embeddings, but **no working LLM**:
//!   run with a deliberately dead `HELIX_LLM_API_KEY` to prove the read path
//!   never calls an LLM (`HELIXIR_RETRIEVAL_PROFILE=algo_opt`).
//!
//! Run:
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt HELIX_LLM_API_KEY=dead-key-on-purpose \
//!   cargo test -p helixir read_path_e2e -- --ignored --nocapture
//! ```
//!
//! Reports per-tool latency (cold first call vs warm) and context-restoration
//! quality (hit@5 / MRR over a golden query set tied to the bench corpus).

use std::time::Instant;

use helixir::core::HelixirClient;

const USER: &str = "bench";

/// Golden set: query → memory_ids, any of which counts as the right context.
/// Curated against the bench corpus (seeded 2026-03-27..04-23).
fn golden_set() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "flaky test Cyrillic",
            vec!["mem_6d0c00cbb797", "mem_02e89bafeed2", "raw_02b063cbbd7a"],
        ),
        // Exact identifier — the BM25 half of the hybrid earns its keep here.
        (
            "TestIntegrationProductSearch",
            vec!["mem_02e89bafeed2"],
        ),
        (
            "repository interfaces",
            vec!["mem_14f614cee843", "mem_c100418279dc", "mem_74c82048e8a9"],
        ),
        (
            "ICU extension SQLite",
            vec!["raw_3c52decc7930", "raw_02b063cbbd7a"],
        ),
        (
            "Clean Architecture test isolation",
            vec!["raw_97ec3e9ac5f9", "mem_4d3b50638e96"],
        ),
        (
            "test coverage repository sqlite",
            vec!["mem_c100418279dc"],
        ),
        (
            "interfaces.go ProductRepository methods",
            vec!["mem_14f614cee843", "mem_491ed67a50f4", "mem_c100418279dc"],
        ),
        (
            "boilerplate trade-off",
            vec!["mem_c100418279dc", "raw_97ec3e9ac5f9"],
        ),
        (
            "setupTestDB isolated in-memory database",
            vec!["mem_02e89bafeed2"],
        ),
        (
            "SQLite LIKE case sensitivity Unicode",
            vec!["mem_7ed1df043686", "mem_02e89bafeed2", "raw_3c52decc7930"],
        ),
    ]
}

fn percentile(sorted_ms: &[f64], p: f64) -> f64 {
    if sorted_ms.is_empty() {
        return 0.0;
    }
    let k = ((sorted_ms.len() - 1) as f64 * p / 100.0).round() as usize;
    sorted_ms[k.min(sorted_ms.len() - 1)]
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 and live HelixDB (bench corpus) + embeddings; see module doc"]
async fn read_path_e2e() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );
    assert_eq!(
        std::env::var("HELIXIR_RETRIEVAL_PROFILE").unwrap_or_default(),
        "algo_opt",
        "This suite validates the algo_opt read path"
    );

    let client = HelixirClient::from_env().expect("HelixirClient::from_env");
    client.initialize().await.expect("initialize");

    // ---------- 1. search_memory: context restoration quality ----------
    let golden = golden_set();
    let mut hits_at_5 = 0usize;
    let mut reciprocal_ranks: Vec<f64> = Vec::new();
    let mut cold_ms: Vec<f64> = Vec::new();
    let mut first_query_ms = 0.0f64;

    for (i, (query, expected)) in golden.iter().enumerate() {
        let t0 = Instant::now();
        let results = client
            .search(query, USER, Some(5), Some("full"), None, None, Some("personal"))
            .await
            .unwrap_or_else(|e| panic!("search '{query}' failed: {e}"));
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if i == 0 {
            // The session-start moment: fresh process, first question asked.
            first_query_ms = ms;
        }
        cold_ms.push(ms);

        let rank = results
            .iter()
            .position(|r| expected.contains(&r.id.as_str()));
        match rank {
            Some(r) => {
                hits_at_5 += 1;
                reciprocal_ranks.push(1.0 / (r as f64 + 1.0));
            }
            None => {
                reciprocal_ranks.push(0.0);
                eprintln!(
                    "  MISS '{query}': expected one of {:?}, got {:?}",
                    expected,
                    results.iter().map(|r| r.id.as_str()).collect::<Vec<_>>()
                );
            }
        }
    }
    let hit_rate = hits_at_5 as f64 / golden.len() as f64;
    let mrr = reciprocal_ranks.iter().sum::<f64>() / reciprocal_ranks.len() as f64;

    // ---------- 2. search_memory: warm latency ----------
    let mut warm_ms: Vec<f64> = Vec::new();
    for (query, _) in &golden {
        let t0 = Instant::now();
        let _ = client
            .search(query, USER, Some(5), Some("full"), None, None, Some("personal"))
            .await
            .expect("warm search");
        warm_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    cold_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    warm_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());

    // ---------- 3. temporal window + cache isolation ----------
    // NOTE: mode "full" deliberately ignores temporal_days (dispatch.rs hardcodes
    // cutoff = None), so the window check must use a windowed mode. `recent`
    // has a 4h default window; the bench corpus is months old.
    let q = "repository interfaces";
    let full = client
        .search(q, USER, Some(5), Some("full"), None, None, Some("personal"))
        .await
        .expect("full search");
    let recent = client
        .search(q, USER, Some(5), Some("recent"), None, None, Some("personal"))
        .await
        .expect("recent search");
    assert!(!full.is_empty(), "corpus must be reachable in full mode");
    if !recent.is_empty() {
        for r in &recent {
            eprintln!(
                "  LEAK: {} created_at={} score={:.3}",
                r.id, r.created_at, r.score
            );
        }
    }
    assert!(
        recent.is_empty(),
        "recent mode (4h window) on a months-old corpus must return nothing — \
         non-empty means the cache collided across temporal windows (P0.3) \
         or BM25 leaked past the cutoff"
    );

    // ---------- 4. search_reasoning_chain: relations without an LLM ----------
    let t0 = Instant::now();
    let chains = client
        .search_reasoning_chain(
            "repository interfaces clean architecture",
            USER,
            Some("both"),
            Some(5),
            Some(5),
        )
        .await
        .expect("search_reasoning_chain");
    let chain_ms = t0.elapsed().as_secs_f64() * 1000.0;
    assert!(
        !chains.chains.is_empty(),
        "bench corpus has IMPLIES/BECAUSE edges around the repository-interfaces \
         cluster; empty chains mean seed search or traversal regressed"
    );
    let has_logical_edge = chains.chains.iter().any(|c| {
        c.nodes
            .iter()
            .any(|n| n.relation == "BECAUSE" || n.relation == "IMPLIES")
    });
    assert!(has_logical_edge, "chains must surface logical edge types");

    // ---------- 4b. "why" restoration: causal mode must surface a BECAUSE ----------
    // The point of Helixir vs plain RAG: the agent asks "why" and gets back a
    // cause via a BECAUSE edge, not just similar text.
    let causal = client
        .search_reasoning_chain(
            "repository interfaces clean architecture",
            USER,
            Some("causal"),
            Some(5),
            Some(5),
        )
        .await
        .expect("causal chain");
    let causal_because = causal
        .chains
        .iter()
        .flat_map(|c| c.nodes.iter())
        .any(|n| n.relation == "BECAUSE" && !n.content.is_empty());
    assert!(
        causal_because,
        "causal mode must restore at least one BECAUSE cause with content"
    );

    // ---------- 4c. collective scope (Hive shared graph) ----------
    let t0 = Instant::now();
    let collective = client
        .search(
            "flaky test",
            USER,
            Some(5),
            Some("full"),
            None,
            None,
            Some("collective"),
        )
        .await
        .expect("collective search");
    let collective_ms = t0.elapsed().as_secs_f64() * 1000.0;
    assert!(
        !collective.is_empty(),
        "collective scope must reach the shared graph"
    );
    let has_user_count = collective
        .iter()
        .any(|r| r.metadata.contains_key("user_count"));
    assert!(
        has_user_count,
        "collective results must carry user_count enrichment (Hive consensus)"
    );

    // ---------- 4d. provenance: graph-pulled results must say so ----------
    // Elder-brain requirement: the agent must be able to tell a direct hit
    // from a fact pulled through the graph, and see the link that pulled it.
    let provenance_results = client
        .search(
            "repository interfaces",
            USER,
            Some(15),
            Some("deep"),
            None,
            None,
            Some("personal"),
        )
        .await
        .expect("provenance search");
    let seed_count = provenance_results
        .iter()
        .filter(|r| r.metadata.get("origin").and_then(|v| v.as_str()) == Some("seed"))
        .count();
    let graph_pulled: Vec<_> = provenance_results
        .iter()
        .filter(|r| r.metadata.get("origin").and_then(|v| v.as_str()) == Some("graph"))
        .collect();
    assert!(seed_count > 0, "results must mark direct hits as origin=seed");
    assert!(
        !graph_pulled.is_empty(),
        "deep search around the repository-interfaces cluster must pull at \
         least one neighbour through the graph (origin=graph)"
    );
    for r in &graph_pulled {
        assert!(
            r.metadata.contains_key("edge") && r.metadata.contains_key("parent"),
            "graph-pulled result {} must carry edge + parent provenance",
            r.id
        );
    }

    // ---------- 5. get_memory_graph ----------
    let t0 = Instant::now();
    let graph = client
        .get_graph(USER, Some("mem_c100418279dc"), Some(2))
        .await
        .expect("get_graph");
    let graph_ms = t0.elapsed().as_secs_f64() * 1000.0;
    assert!(
        !graph.nodes.is_empty() && !graph.edges.is_empty(),
        "mem_c100418279dc has IMPLIES edges in/out; graph must not be empty"
    );

    // ---------- 6. search_by_concept ----------
    let t0 = Instant::now();
    let concepts = client
        .search_by_concept("flaky test decision", USER, Some("action"), None, None, Some(5))
        .await
        .expect("search_by_concept");
    let concept_ms = t0.elapsed().as_secs_f64() * 1000.0;
    assert!(
        !concepts.is_empty(),
        "bench corpus contains action-typed memories about the flaky test"
    );

    // ---------- summary ----------
    println!("\n==== read_path_e2e summary (user={USER}) ====");
    println!(
        "search_memory   quality: hit@5 {hits_at_5}/{} ({:.0}%), MRR {:.3}",
        golden.len(),
        hit_rate * 100.0,
        mrr
    );
    println!(
        "search_memory   latency: session-first {:.1}ms | cold p50 {:.1}ms p95 {:.1}ms | warm p50 {:.1}ms p95 {:.1}ms",
        first_query_ms,
        percentile(&cold_ms, 50.0),
        percentile(&cold_ms, 95.0),
        percentile(&warm_ms, 50.0),
        percentile(&warm_ms, 95.0)
    );
    println!(
        "collective     : {} results, {:.1}ms (user_count enrichment present)",
        collective.len(),
        collective_ms
    );
    println!("causal 'why'   : BECAUSE cause restored: {causal_because}");
    println!(
        "provenance     : {} seeds + {} graph-pulled (edge+parent attached)",
        seed_count,
        graph_pulled.len()
    );
    println!(
        "reasoning_chain: {} chains, deepest {}, {:.1}ms (LLM key is dead — zero LLM calls)",
        chains.chains.len(),
        chains.deepest_chain,
        chain_ms
    );
    println!(
        "get_graph      : {} nodes / {} edges, {:.1}ms",
        graph.nodes.len(),
        graph.edges.len(),
        graph_ms
    );
    println!("search_concept : {} results, {:.1}ms", concepts.len(), concept_ms);
    println!("temporal-window cache isolation: OK (recent-mode query returned empty)");

    // Quality bars: loose enough to survive corpus drift, tight enough to
    // catch real regressions.
    assert!(
        hit_rate >= 0.8,
        "context restoration degraded: hit@5 {hit_rate:.2} < 0.8"
    );
    // Baseline at suite adoption (2026-06-12, algo_opt R1-R3): MRR 0.582 —
    // golden facts land at avg rank ~2. The bar is a regression guard below
    // baseline; raising MRR itself is the PPR phase's job.
    assert!(
        mrr >= 0.5,
        "ranking degraded: MRR {mrr:.3} < 0.5 (baseline 0.582)"
    );
    let warm_p95 = percentile(&warm_ms, 95.0);
    assert!(
        warm_p95 < 300.0,
        "warm search p95 {warm_p95:.1}ms exceeds 300ms budget"
    );
}
