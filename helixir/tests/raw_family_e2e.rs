//! #82: raw-family collapse — compaction of redundancy, never of content.
//!
//! A multi-atom add_memory stores the atoms AND the raw source; before #82
//! a search matching the story billed the reader twice (raw + atoms). Now
//! the write path wires atom→raw PART_OF edges and search collapses the
//! family: the raw folds under the best-ranked atom (its id kept in
//! metadata.collapsed), while SIBLING ATOMS — distinct facts — always stay.

mod common;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::McpClient;
use serde_json::json;

fn token() -> String {
    format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

#[test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + working LLM"]
fn raw_family_collapses_but_atoms_stay() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );

    let (mut mcp, _boot) = McpClient::spawn();
    let run = token();
    let user = format!("rawfam_{run}");

    // Long enough to clear raw_source_min_chars, causal enough to atomize.
    let (add, _ms) = mcp.call_tool(
        "add_memory",
        json!({
            "message": format!(
                "The nightly report for cluster_{run} shows three findings: the ingest queue \
                 backed up because the parser_{run} stalled on malformed rows, the on-call \
                 engineer restarted the parser at three in the morning, and after the restart \
                 the queue drained completely within twenty minutes."
            ),
            "user_id": user,
        }),
    );
    let atom_ids: Vec<String> = add["memory_ids"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        atom_ids.len() >= 2,
        "multi-fact message must atomize into >=2 atoms: {add}"
    );

    // The raw must exist and carry incoming PART_OF edges from the atoms.
    std::thread::sleep(Duration::from_secs(3));
    let mems = common::db_query("getUserMemories", &json!({"user_id": &user, "limit": 30}));
    let raw_id = mems["memories"]
        .as_array()
        .and_then(|rows| {
            rows.iter().find_map(|m| {
                m["memory_id"]
                    .as_str()
                    .filter(|id| id.starts_with("raw_"))
                    .map(String::from)
            })
        })
        .expect("raw source must be stored for a long multi-atom message");
    let incoming = common::db_query("getMemoryIncomingRelations", &json!({"memory_id": &raw_id}));
    let part_of_count = incoming["relations_in"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|e| e["relation_type"].as_str() == Some("PART_OF"))
                .count()
        })
        .unwrap_or(0);
    assert!(
        part_of_count >= 2,
        "write path must wire atom->raw PART_OF edges (found {part_of_count})"
    );

    // Search matching the story: the raw must NOT coexist with its atoms —
    // it folds under the best atom, whose metadata carries the folded ids.
    // Sibling atoms (distinct facts) must all survive.
    let (res, _ms) = mcp.call_tool(
        "search_memory",
        json!({
            "query": format!("why did the ingest queue back up on cluster_{run}"),
            "user_id": user,
            "mode": "full",
            "limit": 10,
        }),
    );
    let rows = res.as_array().expect("search returns rows");
    let raw_present = rows
        .iter()
        .any(|r| r["id"].as_str() == Some(raw_id.as_str()));
    let family_atoms_present: Vec<&str> = rows
        .iter()
        .filter_map(|r| r["id"].as_str())
        .filter(|id| atom_ids.iter().any(|a| a == id))
        .collect();
    let keeper = rows
        .iter()
        .find(|r| r["metadata"]["collapsed"].is_array())
        .expect("a keeper must carry metadata.collapsed (the family shared this window)");
    let folded: Vec<&str> = keeper["metadata"]["collapsed"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    // The contract is NO COEXISTENCE — either direction of folding is legal:
    // best member is an atom → the raw folds (sibling atoms stay, distinct
    // facts); best member is the raw → the atoms fold (their content is
    // verbatim inside the kept raw). Both are content-lossless.
    if raw_present {
        assert!(
            family_atoms_present.is_empty(),
            "raw kept => its atoms must be folded, not coexist: {family_atoms_present:?}"
        );
        assert!(
            folded.iter().any(|id| atom_ids.iter().any(|a| a == id)),
            "folded ids must cover the atoms: {folded:?}"
        );
    } else {
        assert!(
            family_atoms_present.len() >= 2,
            "atom kept => sibling atoms (distinct facts) must survive: {rows:?}"
        );
        assert!(
            folded.contains(&raw_id.as_str()),
            "folded ids must include the raw ({raw_id}): {folded:?}"
        );
    }

    println!(
        "==== raw_family_e2e ==== atoms={} part_of={} rows={} (raw folded into keeper, siblings intact)",
        atom_ids.len(),
        part_of_count,
        rows.len()
    );
}
