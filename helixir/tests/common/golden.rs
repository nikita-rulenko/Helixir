//! The golden fixture corpus (#76) — a deterministic, LLM-free seedable
//! corpus for the read-path oracles. Replaces the historic bench corpus
//! whose exact memory ids died with the 2026-06-30 data loss.
//!
//! Design decisions:
//! - Seeded through `add_prepared` (no extraction), so the DEAD-LLM-KEY
//!   property of the read suites is preserved end to end.
//! - Assertions match CONTENT MARKERS, not memory ids — ids are random
//!   UUIDs, but the texts are ours; marker matching survives re-seeds
//!   (content_key dedup makes re-seeding a deterministic no-op).
//! - Typed causal chains are wired directly via `add_relation` so
//!   `search_reasoning_chain` has deterministic structure to walk.

use helixir::core::HelixirClient;
use helixir::llm::extractor::ExtractedMemory;
use helixir::toolkit::mind_toolbox::reasoning::ReasoningType;

pub const GOLDEN_USER: &str = "golden_v1";

/// `(marker, text, memory_type)` — marker is a unique substring used by
/// golden_set assertions. 24 atoms across four themes with near-duplicates
/// for rank discrimination.
pub fn corpus() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        // Theme A: the migration saga (causal chain A1→A2→A3→A4)
        (
            "GA1",
            "golden GA1: the payments service migrated from sqlite to postgres in sprint twelve.",
            "action",
        ),
        (
            "GA2",
            "golden GA2: the sqlite file locked under concurrent writers during checkout spikes.",
            "fact",
        ),
        (
            "GA3",
            "golden GA3: postgres row locking removed the checkout latency spikes entirely.",
            "fact",
        ),
        (
            "GA4",
            "golden GA4: the team standardized on postgres for every new service after the migration.",
            "goal",
        ),
        // Theme B: retry policy (chain B1→B2→B3)
        (
            "GB1",
            "golden GB1: the aurora ingest worker saw transient kappa queue outages under a minute.",
            "fact",
        ),
        (
            "GB2",
            "golden GB2: aurora retries use exponential backoff capped at ninety seconds with jitter.",
            "fact",
        ),
        (
            "GB3",
            "golden GB3: dropped kappa messages fell to zero after the backoff cap landed.",
            "achievement",
        ),
        // Theme C: preferences & tooling
        (
            "GC1",
            "golden GC1: dana prefers trunk-based development over long-lived feature branches.",
            "preference",
        ),
        (
            "GC2",
            "golden GC2: dana prefers ripgrep over grep for code search.",
            "preference",
        ),
        (
            "GC3",
            "golden GC3: the linter budget is zero warnings on main, enforced in ci.",
            "fact",
        ),
        (
            "GC4",
            "golden GC4: dana wants the incident runbook rewritten as a checklist by q3.",
            "goal",
        ),
        // Theme D: observability
        (
            "GD1",
            "golden GD1: the metrics pipeline samples traces at one percent in production.",
            "fact",
        ),
        (
            "GD2",
            "golden GD2: raising the trace sample rate to ten percent doubled the tempo storage bill.",
            "experience",
        ),
        (
            "GD3",
            "golden GD3: alert fatigue dropped after the pager rules moved to symptom-based alerts.",
            "experience",
        ),
        (
            "GD4",
            "golden GD4: the oncall handbook lives in the platform wiki under runbooks slash pager.",
            "fact",
        ),
        // Near-duplicates / distractors (rank discrimination)
        (
            "GX1",
            "golden GX1: a sqlite database is a single file on disk.",
            "fact",
        ),
        (
            "GX2",
            "golden GX2: postgres is an open source relational database.",
            "fact",
        ),
        (
            "GX3",
            "golden GX3: queues deliver messages between services.",
            "fact",
        ),
        (
            "GX4",
            "golden GX4: retries are a common resilience pattern.",
            "fact",
        ),
        (
            "GX5",
            "golden GX5: dana attended the platform guild meetup in june.",
            "experience",
        ),
        (
            "GX6",
            "golden GX6: tracing shows request flow across services.",
            "fact",
        ),
        (
            "GX7",
            "golden GX7: checklists reduce omission errors in operations.",
            "fact",
        ),
        (
            "GX8",
            "golden GX8: ci pipelines run the test suite on every push.",
            "fact",
        ),
        (
            "GX9",
            "golden GX9: the platform wiki hosts team documentation.",
            "fact",
        ),
    ]
}

/// Causal chain wiring: (from_marker, to_marker, type).
pub fn chains() -> Vec<(&'static str, &'static str, ReasoningType)> {
    vec![
        ("GA2", "GA1", ReasoningType::Because), // locking caused the migration
        ("GA1", "GA3", ReasoningType::Implies), // migration → spikes gone
        ("GA3", "GA4", ReasoningType::Because), // result → standardization
        ("GB1", "GB2", ReasoningType::Because), // outages → backoff policy
        ("GB2", "GB3", ReasoningType::Implies), // policy → zero drops
        ("GD2", "GD1", ReasoningType::Because), // bill → 1% sampling
    ]
}

/// `(query, expected markers)` — a hit is any result whose content contains
/// one of the markers. Shared by read_path_e2e and mcp_read_e2e.
pub fn golden_set() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("why did payments move off sqlite", vec!["GA2", "GA1"]),
        ("what fixed the checkout latency spikes", vec!["GA3", "GA1"]),
        ("aurora retry backoff policy", vec!["GB2"]),
        ("did dropped queue messages stop", vec!["GB3", "GB2"]),
        ("what does dana prefer for code search", vec!["GC2"]),
        ("branching strategy preference", vec!["GC1"]),
        ("trace sampling rate in production", vec!["GD1", "GD2"]),
        ("where is the oncall handbook", vec!["GD4"]),
        ("linter warnings policy on main", vec!["GC3"]),
        ("incident runbook rewrite goal", vec!["GC4"]),
    ]
}

/// Aged fixtures for the temporal contract (#31): raw HQL inserts with
/// CONTROLLED created_at / valid_from (no vector — the BM25 arm of hybrid
/// search carries them). `GOLDOLD` is a year-old ingestion; `GOLDEVENT`
/// was ingested long ago but its EVENT time (valid_from) is recent —
/// the bi-temporal discriminator.
pub const AGED: &[(&str, &str, &str, &str)] = &[
    (
        "gold_aged_created",
        "golden GOLDOLD: the legacy billing cron still runs quarterly reconciliation.",
        "2025-05-01T00:00:00+00:00",
        "2025-05-01T00:00:00+00:00",
    ),
    (
        "gold_aged_event",
        "golden GOLDEVENT: the quarterly reconciliation window moved to the first business day.",
        "2025-05-01T00:00:00+00:00",
        "2026-06-20T00:00:00+00:00",
    ),
];

fn seed_aged() {
    use serde_json::json;
    super::db_query(
        "addUser",
        &json!({"user_id": GOLDEN_USER, "name": GOLDEN_USER}),
    );
    for (mid, text, created, valid_from) in AGED {
        super::db_query(
            "addMemoryWithValidFrom",
            &json!({
                "memory_id": mid, "user_id": GOLDEN_USER, "content": text,
                "memory_type": "fact", "certainty": 90, "importance": 60,
                "created_at": created, "updated_at": created, "valid_from": valid_from,
                "context_tags": "golden-fixture", "source": "fixture", "metadata": "{}",
            }),
        );
        super::db_query(
            "linkUserToMemory",
            &json!({"user_id": GOLDEN_USER, "memory_id": mid, "context": "golden"}),
        );
    }
}

/// Idempotently seed the corpus + chains + aged fixtures, then WAIT for
/// search visibility (HelixDB snapshot lag: durable != immediately visible).
/// One batched add_prepared call: one embed batch, one decision phase, one
/// concurrent inference phase — LLM-free on a fresh store, minutes not tens.
pub async fn ensure_seeded(client: &HelixirClient) -> usize {
    let atoms: Vec<ExtractedMemory> = corpus()
        .into_iter()
        .map(|(_, text, mtype)| ExtractedMemory {
            text: text.to_string(),
            memory_type: mtype.to_string(),
            certainty: 90,
            importance: 60,
            entities: vec![],
            context: None,
        })
        .collect();

    let r = client
        .add_prepared(atoms, GOLDEN_USER, None, Some("golden-fixture"))
        .await
        .expect("golden seed batch");
    let added_total = r.memories_added;

    // Fresh store: ADDs come back in input order -> marker->id mapping holds.
    if added_total == corpus().len() {
        let marker_ids: std::collections::HashMap<&'static str, String> = corpus()
            .iter()
            .map(|(m, _, _)| *m)
            .zip(r.memory_ids.iter().cloned())
            .collect();
        for (from_m, to_m, rt) in chains() {
            if let (Some(f), Some(t)) = (marker_ids.get(from_m), marker_ids.get(to_m)) {
                let _ = client.tooling().add_typed_relation(f, t, rt, 80).await;
            }
        }
    }

    // Aged fixtures are independent of the batch: seed when absent.
    let probe = super::db_query(
        "getMemory",
        &serde_json::json!({"memory_id": "gold_aged_created"}),
    );
    if probe
        .get("memory")
        .and_then(|m| m.get("memory_id"))
        .is_none()
    {
        seed_aged();
    }

    // Visibility wait: poll until the first marker is searchable (or 30s).
    for _ in 0..30 {
        let hits = client
            .search(
                "payments sqlite postgres migration",
                GOLDEN_USER,
                Some(5),
                Some("full"),
                None,
                None,
                Some("personal"),
            )
            .await
            .unwrap_or_default();
        if hits.iter().any(|h| h.content.contains("GA1")) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    added_total
}
