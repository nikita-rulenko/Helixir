//! Read-path end-to-end suite: every read tool the `local-reasoning` branch
//! touched, exercised against a live HelixDB with the GOLDEN corpus (#76) —
//! deterministic, self-seeding, LLM-free.
//!
//! **Not run by default** (`#[ignore]`). Requires live infrastructure:
//! - `HELIX_HOST` / `HELIX_PORT` — a live HelixDB (the suite seeds `golden_v1`)
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
//! quality (hit@5 / MRR over the golden query set, matched by content markers).

use std::time::Instant;

use helixir::core::HelixirClient;

mod common;
use common::golden::{GOLDEN_USER as USER, ensure_seeded, golden_set};

fn percentile(sorted_ms: &[f64], p: f64) -> f64 {
    if sorted_ms.is_empty() {
        return 0.0;
    }
    let k = ((sorted_ms.len() - 1) as f64 * p / 100.0).round() as usize;
    sorted_ms[k.min(sorted_ms.len() - 1)]
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 and live HelixDB + embeddings; see module doc"]
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

    // #76: the golden corpus is deterministic and LLM-free — seed it in
    // place if this store has never seen it (content_key dedup makes the
    // re-run a no-op, so the dead-LLM-key property of the suite holds).
    let seeded = ensure_seeded(&client).await;
    println!("golden corpus: {seeded} atoms added this run");

    // ---------- 1. search_memory: context restoration quality ----------
    let golden = golden_set();
    let mut hits_at_5 = 0usize;
    let mut reciprocal_ranks: Vec<f64> = Vec::new();
    let mut cold_ms: Vec<f64> = Vec::new();
    let mut first_query_ms = 0.0f64;

    for (i, (query, expected)) in golden.iter().enumerate() {
        let t0 = Instant::now();
        let results = client
            .search(
                query,
                USER,
                Some(5),
                Some("full"),
                None,
                None,
                Some("personal"),
            )
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
            .position(|r| expected.iter().any(|m| r.content.contains(m)));
        match rank {
            Some(r) => {
                hits_at_5 += 1;
                reciprocal_ranks.push(1.0 / (r as f64 + 1.0));
            }
            None => {
                reciprocal_ranks.push(0.0);
                eprintln!(
                    "  MISS '{query}': expected marker of {:?}, got {:?}",
                    expected,
                    results
                        .iter()
                        .map(|r| r.content.chars().take(50).collect::<String>())
                        .collect::<Vec<_>>()
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
            .search(
                query,
                USER,
                Some(5),
                Some("full"),
                None,
                None,
                Some("personal"),
            )
            .await
            .expect("warm search");
        warm_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    cold_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    warm_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());

    // ---------- 3. the temporal contract (#31) ----------
    // Time governs ATTENTION, never REACHABILITY: no mode hides old facts;
    // only an EXPLICIT temporal_days is a hard window, and it runs on EVENT
    // time (valid_from else created_at) — bi-temporality.
    // Marker-specific queries: BM25 lifts the vectorless fixtures to the top
    // for THEIR OWN terms; a shared-topic query only proves attention, not
    // reachability.
    let q_old = "legacy billing cron quarterly reconciliation";
    let q_event = "reconciliation window moved first business day";
    let hits = |rs: &Vec<helixir::core::helixir_client::SearchResult>, marker: &str| {
        rs.iter().any(|r| r.content.contains(marker))
    };

    // 3a. A year-old fact is reachable in EVERY mode.
    for mode in ["full", "recent", "contextual", "deep"] {
        let rs = client
            .search(
                q_old,
                USER,
                Some(10),
                Some(mode),
                None,
                None,
                Some("personal"),
            )
            .await
            .unwrap_or_else(|e| panic!("{mode} search failed: {e}"));
        assert!(
            hits(&rs, "GOLDOLD"),
            "reachability violated: year-old GOLDOLD missing in mode={mode}: {:?}",
            rs.iter().map(|r| &r.content).collect::<Vec<_>>()
        );
    }

    // 3b. An EXPLICIT 30-day window is a hard filter on EVENT time for
    // SEEDS: GOLDOLD (created AND valid 2025) may NOT rank as an ordinary
    // row; since #87 it MAY come back as a graph flashback, but then it MUST
    // wear the flag. GOLDEVENT (ingested 2025 but valid_from 2026-06-20)
    // survives as a seed — the bi-temporal discriminator.
    let is_flashback = |r: &helixir::core::helixir_client::SearchResult| {
        r.metadata
            .get("flashback")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };
    let windowed_old = client
        .search(
            q_old,
            USER,
            Some(10),
            Some("full"),
            Some(30.0),
            None,
            Some("personal"),
        )
        .await
        .expect("windowed search (old)");
    assert!(
        !windowed_old
            .iter()
            .any(|r| r.content.contains("GOLDOLD") && !is_flashback(r)),
        "explicit temporal_days=30 must not rank GOLDOLD (event time 2025) as an ordinary row — unflagged, it WAS top-ranked for this query without the window: {:?}",
        windowed_old.iter().map(|r| &r.content).collect::<Vec<_>>()
    );
    let windowed_event = client
        .search(
            q_event,
            USER,
            Some(10),
            Some("full"),
            Some(30.0),
            None,
            Some("personal"),
        )
        .await
        .expect("windowed search (event)");
    assert!(
        hits(&windowed_event, "GOLDEVENT"),
        "bi-temporality: GOLDEVENT (ingested 2025, valid_from 2026-06) must SURVIVE the 30-day window: {:?}",
        windowed_event
            .iter()
            .map(|r| &r.content)
            .collect::<Vec<_>>()
    );

    // 3c. #87 flashbacks: a two-sided EVENT-time window that admits only
    // GOLDEVENT, plus a causal edge GOLDEVENT→GOLDOLD, must bring GOLDOLD
    // back THROUGH THE GRAPH — flagged as a flashback with its event date,
    // never hidden and never disguised as an in-window row.
    let _ = client
        .tooling()
        .add_typed_relation(
            "gold_aged_event",
            "gold_aged_created",
            helixir::toolkit::mind_toolbox::reasoning::ReasoningType::Because,
            80,
        )
        .await; // idempotent: the duplicate guard makes re-runs a no-op
    let window = helixir::core::TimeWindow {
        from: Some(
            chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        ),
        to: None,
    };
    let flash_rs = client
        .search_windowed(
            q_event,
            USER,
            Some(10),
            Some("full"),
            None,
            None,
            Some("personal"),
            window,
        )
        .await
        .expect("windowed search (flashback)");
    assert!(
        hits(&flash_rs, "GOLDEVENT"),
        "3c: in-window GOLDEVENT must seed the windowed search: {:?}",
        flash_rs.iter().map(|r| &r.content).collect::<Vec<_>>()
    );
    let goldold_row = flash_rs.iter().find(|r| r.content.contains("GOLDOLD"));
    let goldold_row = goldold_row.unwrap_or_else(|| {
        panic!(
            "3c: GOLDOLD must return as a graph flashback (edge GOLDEVENT→GOLDOLD exists): {:?}",
            flash_rs.iter().map(|r| &r.content).collect::<Vec<_>>()
        )
    });
    assert!(
        is_flashback(goldold_row),
        "3c: the out-of-window GOLDOLD row must be FLAGGED flashback: {:?}",
        goldold_row.metadata
    );
    assert!(
        goldold_row
            .metadata
            .get("event_date")
            .and_then(|v| v.as_str())
            .map(|d| d.starts_with("2025"))
            .unwrap_or(false),
        "3c: the flashback must carry its true event date (2025-…): {:?}",
        goldold_row.metadata
    );

    // ---------- 4. search_reasoning_chain: relations without an LLM ----------
    let t0 = Instant::now();
    let chains = client
        .search_reasoning_chain(
            "why did payments migrate from sqlite to postgres",
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
        "the golden corpus wires BECAUSE/IMPLIES around the sqlite->postgres \
         migration (GA-chain); empty chains mean seed search or traversal regressed"
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
            "why did checkout latency spikes stop",
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

    // ---------- 4f. relation-inference baseline (guards #96 batch-infer) ----------
    // The golden corpus is built through the write pipeline's relation
    // inference. Pin, read-side and LLM-free, that inference does NOT collapse
    // the graph-of-why to a single edge type: a batched re-implementation (#96),
    // re-seeded onto a fresh store, must still yield MULTIPLE typed relations.
    const TYPED: [&str; 7] = [
        "BECAUSE",
        "IMPLIES",
        "SUPPORTS",
        "CONTRADICTS",
        "RELATES_TO",
        "PART_OF",
        "IS_A",
    ];
    let infer_chains = client
        .search_reasoning_chain(
            "postgres sqlite migration checkout latency metrics",
            USER,
            Some("both"),
            Some(8),
            Some(6),
        )
        .await
        .expect("relation-inference chains");
    let rel_types: std::collections::HashSet<String> = infer_chains
        .chains
        .iter()
        .flat_map(|c| c.nodes.iter())
        .map(|n| n.relation.clone())
        .filter(|r| TYPED.contains(&r.as_str()))
        .collect();
    assert!(
        rel_types.len() >= 2,
        "relation inference must not collapse the golden graph-of-why to one \
         edge type (guards #96 batch-infer); got {rel_types:?}"
    );
    println!("relation-inference: golden chains expose types {rel_types:?}");

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
            "postgres migration payments service",
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
    assert!(
        seed_count > 0,
        "results must mark direct hits as origin=seed"
    );
    assert!(
        !graph_pulled.is_empty(),
        "deep search around the migration cluster must pull at \
         least one neighbour through the graph (origin=graph)"
    );
    for r in &graph_pulled {
        assert!(
            r.metadata.contains_key("edge") && r.metadata.contains_key("parent"),
            "graph-pulled result {} must carry edge + parent provenance",
            r.id
        );
    }

    // ---------- 4e. connect_memories: path between two anchors ----------
    let t0 = Instant::now();
    let connection = client
        .connect_memories(
            "sqlite file locked under concurrent writers",
            "team standardized on postgres for new services",
            USER,
            Some(4),
        )
        .await
        .expect("connect_memories");
    let connect_ms = t0.elapsed().as_secs_f64() * 1000.0;
    assert!(
        connection.found,
        "GA2 and GA4 are linked through the golden migration chain \
         (BECAUSE/IMPLIES edges) — a path must exist"
    );
    assert_eq!(
        connection.nodes.len(),
        connection.edges.len() + 1,
        "path shape: N nodes need N-1 edges"
    );

    // ---------- 5. get_memory_graph ----------
    // Anchor on a chain node resolved at runtime (ids are random per seed).
    let anchor = client
        .search(
            "payments service migrated sqlite postgres",
            USER,
            Some(3),
            Some("full"),
            None,
            None,
            Some("personal"),
        )
        .await
        .expect("anchor search")
        .into_iter()
        .find(|r| r.content.contains("GA1"))
        .map(|r| r.id)
        .expect("GA1 must be findable to anchor the graph probe");
    let t0 = Instant::now();
    let graph = client
        .get_graph(USER, Some(anchor.as_str()), Some(2))
        .await
        .expect("get_graph");
    let graph_ms = t0.elapsed().as_secs_f64() * 1000.0;
    assert!(
        !graph.nodes.is_empty() && !graph.edges.is_empty(),
        "GA1 sits mid-chain (BECAUSE in, IMPLIES out); graph must not be empty"
    );

    // ---------- 6. search_by_concept ----------
    let t0 = Instant::now();
    let concepts = client
        .search_by_concept(
            "payments service migrated postgres",
            USER,
            Some("action"),
            None,
            None,
            Some(5),
        )
        .await
        .expect("search_by_concept");
    let concept_ms = t0.elapsed().as_secs_f64() * 1000.0;
    assert!(
        !concepts.is_empty(),
        "the golden corpus contains an action-typed memory (GA1 migration)"
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
        "connect A<->B  : found={} hops={} conf={:.3}, {:.1}ms",
        connection.found, connection.hops, connection.confidence, connect_ms
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
    println!(
        "search_concept : {} results, {:.1}ms",
        concepts.len(),
        concept_ms
    );
    println!(
        "temporal contract: reachable in every mode; explicit window bi-temporal; flashbacks flagged (#87)"
    );

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
