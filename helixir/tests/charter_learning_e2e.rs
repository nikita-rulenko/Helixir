//! #34 increment 2b guard: the charter LEARNS from verdicts. Three identical
//! contradiction-review verdicts must produce a rule_proposal; adopting the
//! rule verbatim must render it in memory://rules; and a further identical
//! dispute must resolve with the proposal QUIET.
//!
//! Probabilistic ingredient: the write path must ESCALATE each preference
//! reversal (needs_clarification with ids). Near-restatement wording makes
//! that reliable; rounds that don't escalate are skipped and extra rounds
//! compensate, so the test asserts the LOOP, not per-write LLM behavior.
//!
//! ```text
//! HELIX_E2E=1 cargo test -p helixir --test charter_learning_e2e -- --ignored --nocapture
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
fn three_identical_verdicts_grow_a_rule() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let (mut mcp, _) = McpClient::spawn();
    let run = token();

    // Distinct nonsense subjects per round: fresh disputes, no cross-round
    // dedup, and the shape (preference-preference-owner_confirmed) repeats.
    let subjects = ["flarn", "gruble", "zwimp", "quorv", "blenk", "trosk"];
    let mut proposal: Option<serde_json::Value> = None;
    let mut resolved_rounds = 0usize;

    for (i, subj) in subjects.iter().enumerate() {
        if proposal.is_some() {
            break;
        }
        let user = format!("chlearn_{run}_{i}");
        let (a1, _) = mcp.call_tool(
            "add_memory",
            json!({"message": format!("I prefer the {subj}{run} layout on my dashboard."), "user_id": user}),
        );
        assert_eq!(a1["ok"].as_bool(), Some(true), "seed pref: {a1}");
        let (a2, _) = mcp.call_tool(
            "add_memory",
            json!({"message": format!("Actually I hate the {subj}{run} layout on my dashboard now, the plain layout only."), "user_id": user}),
        );

        let Some(cl) = a2["needs_clarification"]
            .as_array()
            .and_then(|c| c.first())
            .cloned()
        else {
            println!("round {i}: no escalation (LLM variance) — skipping");
            continue;
        };
        let (Some(from_id), Some(to_id)) = (
            cl["new_memory_id"].as_str(),
            cl["existing_memory_id"].as_str(),
        ) else {
            println!("round {i}: clarification without ids — skipping: {cl}");
            continue;
        };

        let (r, _) = mcp.call_tool(
            "resolve_contradiction",
            json!({"from_id": from_id, "to_id": to_id, "resolution": "confirm"}),
        );
        assert_eq!(r["resolved"].as_bool(), Some(true), "resolve: {r}");
        resolved_rounds += 1;
        println!(
            "round {i}: resolved (total {resolved_rounds}), proposal={}",
            r.get("rule_proposal").is_some()
        );
        if r.get("rule_proposal").is_some() {
            proposal = r.get("rule_proposal").cloned();
        }
    }

    // The shape namespace is GLOBAL and adoption is permanent (append-only
    // store, shared bench) — so this test proves whichever phase of the
    // learning loop the store is in:
    //   fresh store  -> proposal fires -> adopt -> renders -> quiet
    //   mature store -> a previously adopted rule already SILENCES proposals
    //                   (and must be rendered in memory://rules to prove it).
    const SHAPE: &str = "preference-preference-owner_confirmed";
    assert!(
        resolved_rounds >= 3,
        "need >=3 escalated+resolved rounds to exercise the loop (got {resolved_rounds})"
    );

    match proposal {
        Some(proposal) => {
            let shape = proposal["shape"].as_str().expect("proposal shape");
            assert_eq!(shape, SHAPE, "unexpected shape: {proposal}");
            assert!(
                proposal["precedents"].as_u64().unwrap_or(0) >= 3,
                "proposal must cite >=3 precedents: {proposal}"
            );
            assert!(
                proposal["proposal"]
                    .as_str()
                    .unwrap_or("")
                    .contains("Charter rule ["),
                "proposal must dictate the adoption call: {proposal}"
            );

            // Adopt verbatim (deterministic single-atom path, no extraction).
            let rule_text = format!(
                "Charter rule [{shape}]: treat this pair as complementary — keep both \
                 records without raising a clarification, because repeated reviews \
                 resolved this shape the same way. (e2e {run})"
            );
            let (adopt, _) = mcp.call_tool(
                "add_memory",
                json!({"message": rule_text, "user_id": "helixir"}),
            );
            assert_eq!(adopt["ok"].as_bool(), Some(true), "adoption: {adopt}");
            println!("fresh-store branch: proposal fired and rule adopted");
        }
        None => {
            // Mature store: proposals stayed quiet across >=3 identical
            // verdicts — legitimate ONLY because an adopted rule covers the
            // shape. Prove it.
            println!("mature-store branch: proposals quiet — verifying an adopted rule exists");
        }
    }

    // ---- In BOTH branches: the rule renders in memory://rules, and a
    // ---- further identical dispute resolves with the proposal QUIET.
    let rules = mcp.read_resource("memory://rules");
    assert!(
        rules.contains(&format!("Charter rule [{SHAPE}]")),
        "adopted rule for {SHAPE} missing from memory://rules — if proposals were quiet \
         WITHOUT an adopted rule, the learning loop is broken"
    );

    let user = format!("chlearn_{run}_after");
    let (_, _) = mcp.call_tool(
        "add_memory",
        json!({"message": format!("I prefer the wumbo{run} layout on my dashboard."), "user_id": user}),
    );
    let (a2, _) = mcp.call_tool(
        "add_memory",
        json!({"message": format!("Actually I hate the wumbo{run} layout on my dashboard now, the plain layout only."), "user_id": user}),
    );
    if let Some(cl) = a2["needs_clarification"].as_array().and_then(|c| c.first()) {
        if let (Some(from_id), Some(to_id)) = (
            cl["new_memory_id"].as_str(),
            cl["existing_memory_id"].as_str(),
        ) {
            let (r, _) = mcp.call_tool(
                "resolve_contradiction",
                json!({"from_id": from_id, "to_id": to_id, "resolution": "confirm"}),
            );
            assert!(
                r.get("rule_proposal").is_none(),
                "an adopted rule must SILENCE proposals of its shape: {r}"
            );
            println!("post-adoption dispute: resolved quietly ✓");
        }
    } else {
        println!("post-adoption round: no escalation (variance) — silencing untested this run");
    }

    println!(
        "==== charter_learning_e2e ====\nshape={SHAPE}, rounds={resolved_rounds}, rendered + quiet"
    );
}
