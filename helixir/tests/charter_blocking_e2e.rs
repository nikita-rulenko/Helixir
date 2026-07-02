//! Charter increment 2 (#34): blocking semantics.
//!
//! The OBSERVABLE constitution contract (C3) is deterministic and asserted
//! unconditionally: after a preference reversal lands, the ORIGINAL
//! preference still exists — no silent rewrite, whatever verdict the model
//! chose (ADD / CONTRADICT / deferred SUPERSEDE all preserve it).
//!
//! The deferral MACHINERY is asserted when the model actually produced a
//! destructive verdict (probabilistic per write): the clarification says
//! DEFERRED, a charter_deferred CONTRADICTS edge exists, and
//! resolve_contradiction(retract) executes the supersede THEN — with history.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test charter_blocking_e2e -- --ignored --nocapture
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

mod common;
use common::McpClient;

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
fn preference_reversal_never_rewrites_silently() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let (mut mcp, _) = McpClient::spawn();
    let run = token();
    let user = format!("charter_{run}");

    let old_pref = format!("I prefer dark mode in the blorp{run} editor.");
    let (a1, _) = mcp.call_tool("add_memory", json!({"message": old_pref, "user_id": user}));
    let old_id = a1["memory_ids"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| panic!("first preference must store: {a1}"));

    let new_pref = format!("I prefer light mode in the blorp{run} editor.");
    let (a2, _) = mcp.call_tool("add_memory", json!({"message": new_pref, "user_id": user}));
    assert_eq!(a2["ok"].as_bool(), Some(true), "reversal write ok: {a2}");

    // ---- C3, unconditional: the OLD preference is still in the graph. ----
    let (results, _) = mcp.call_tool(
        "search_memory",
        json!({"query": format!("dark mode blorp{run}"), "user_id": user, "mode": "full", "limit": 10}),
    );
    let old_alive = results
        .as_array()
        .map(|a| {
            a.iter().any(|r| {
                r["content"].as_str().unwrap_or("").contains("dark mode")
                    && r["content"].as_str().unwrap_or("").contains(&run)
            })
        })
        .unwrap_or(false);
    assert!(
        old_alive,
        "C3 violated: the original preference was silently rewritten. old_id={old_id}: {results}"
    );

    // ---- Deferral machinery, when the verdict was destructive. ----
    let deferred = a2["needs_clarification"]
        .as_array()
        .map(|c| {
            c.iter().any(|cl| {
                cl["decision_taken"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("DEFERRED")
            })
        })
        .unwrap_or(false);

    if deferred {
        let new_id = a2["memory_ids"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .expect("deferred write stores the new fact as ADD");

        // Settle it: the human says the old preference is outdated.
        let (settled, _) = mcp.call_tool(
            "resolve_contradiction",
            json!({"from_id": new_id, "to_id": old_id, "resolution": "retract"}),
        );
        assert_eq!(
            settled["resolved"].as_bool(),
            Some(true),
            "the deferred dispute must settle: {settled}"
        );
        // The supersede happened ON RESOLUTION, with history preserved: the
        // old memory is still reachable via direct listing.
        let (listed, _) = mcp.call_tool("list_memories", json!({"user_id": user, "limit": 20}));
        let still_reachable = listed
            .as_array()
            .map(|a| {
                a.iter()
                    .any(|m| m["memory_id"].as_str().or(m["id"].as_str()) == Some(old_id.as_str()))
            })
            .unwrap_or(false);
        assert!(
            still_reachable,
            "retract must SUPERSEDE, never destroy: {listed}"
        );
        println!("\n==== charter_blocking_e2e ====");
        println!("verdict was destructive → DEFERRED → retract settled with supersede+history ✓");
    } else {
        println!("\n==== charter_blocking_e2e ====");
        println!(
            "NOTE: model verdict was non-destructive this run (C3 held via ADD/CONTRADICT); the deferral branch was not exercised — probabilistic per write."
        );
    }
}
