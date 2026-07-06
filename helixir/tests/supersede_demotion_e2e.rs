//! #92 guard: a superseded memory must rank BELOW its successor — and wear
//! the flag. In an append-only store a densely-linked stale hub otherwise
//! outranks its own corrections forever (observed live: stale fact at
//! 0.926/ppr=1.0 above two explicit corrections).
//!
//! LLM-free and deterministic: prepared atoms + a direct SUPERSEDES edge.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt HELIX_LLM_API_KEY=dead-key-on-purpose \
//!   cargo test -p helixir --test supersede_demotion_e2e -- --ignored --nocapture
//! ```

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use helixir::core::HelixirClient;
use helixir::llm::extractor::ExtractedMemory;

fn atom(text: &str) -> ExtractedMemory {
    ExtractedMemory {
        text: text.to_string(),
        memory_type: "fact".to_string(),
        certainty: 90,
        importance: 60,
        entities: vec![],
        context: None,
    }
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings (no LLM)"]
async fn superseded_hub_ranks_below_its_correction() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("client");
    client.initialize().await.expect("init");

    let run = format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let user = format!("demote_{}", &run[..10]);

    // The stale fact and its correction share the query vocabulary tightly,
    // so BOTH rank; without demotion the older row (stored first, equal
    // wording weight) competes head-on.
    let stale =
        format!("SUPTEST{run}: the payments gateway endpoint is https://old-gw.example/v1.");
    let fresh =
        format!("SUPTEST{run}: the payments gateway endpoint moved to https://new-gw.example/v2.");
    let r = client
        .add_prepared(
            vec![atom(&stale), atom(&fresh)],
            &user,
            None,
            Some("e2e-92"),
        )
        .await
        .expect("seed");
    assert_eq!(r.memories_added, 2, "both atoms stored: {:?}", r);
    let stale_id = r.memory_ids[0].clone();
    let fresh_id = r.memory_ids[1].clone();

    client
        .tooling()
        .record_supersession(&fresh_id, &stale_id, "e2e-92: endpoint moved")
        .await
        .expect("supersession edge");

    // HelixDB snapshot lag: durable != immediately searchable.
    tokio::time::sleep(Duration::from_secs(2)).await;

    let results = client
        .search(
            &format!("SUPTEST{run} payments gateway endpoint"),
            &user,
            Some(10),
            Some("full"),
            None,
            None,
            Some("personal"),
        )
        .await
        .expect("search");

    let pos = |id: &str| results.iter().position(|r| r.id == id);
    let (stale_pos, fresh_pos) = (pos(&stale_id), pos(&fresh_id));
    assert!(
        fresh_pos.is_some(),
        "correction missing from results: {:?}",
        results.iter().map(|r| &r.content).collect::<Vec<_>>()
    );
    let stale_pos = stale_pos.unwrap_or_else(|| {
        panic!(
            "reachability violated: superseded row must still RETURN (demoted, not hidden): {:?}",
            results.iter().map(|r| &r.content).collect::<Vec<_>>()
        )
    });
    assert!(
        fresh_pos.unwrap() < stale_pos,
        "#92: the correction must outrank the superseded fact: fresh at {:?}, stale at {stale_pos}: {:?}",
        fresh_pos,
        results
            .iter()
            .map(|r| format!(
                "{:.3} {}",
                r.score,
                r.content.chars().take(60).collect::<String>()
            ))
            .collect::<Vec<_>>()
    );

    let stale_row = &results[stale_pos];
    assert_eq!(
        stale_row
            .metadata
            .get("superseded")
            .and_then(|v| v.as_bool()),
        Some(true),
        "the demoted row must wear the flag: {:?}",
        stale_row.metadata
    );
    assert_eq!(
        stale_row
            .metadata
            .get("superseded_by")
            .and_then(|v| v.as_str()),
        Some(fresh_id.as_str()),
        "the flag must name the successor: {:?}",
        stale_row.metadata
    );

    println!(
        "==== supersede_demotion_e2e ====\nfresh at #{}, stale at #{stale_pos} (flagged, successor named)",
        fresh_pos.unwrap()
    );
}
