//! Use-case guard: the paraphrase backstop (#55) must NEVER unify two
//! CONTRADICTORY memories. This exercises the full wiring — add →
//! `merge_paraphrases` → NLI judge — not the judge in isolation, so a
//! regression that drops the `is_same_fact` guard from the merge path (and
//! would silently merge "prefer dark" with "prefer light", gaslighting the
//! owner) is caught.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --features nli --test nli_antimerge_e2e -- --ignored --nocapture
//! ```
#![cfg(feature = "nli")]

use std::time::{SystemTime, UNIX_EPOCH};

use helixir::core::HelixirClient;
use helixir::llm::nli;

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
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + the NLI model"]
async fn paraphrase_merge_never_unifies_contradiction() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );
    if !nli::status().installed {
        eprintln!("SKIP paraphrase_merge_never_unifies_contradiction: NLI model not downloaded");
        return;
    }

    let client = HelixirClient::from_env().expect("from_env");
    let run = token();
    let user_x = format!("antimerge_{run}_x");
    let user_y = format!("antimerge_{run}_y");

    // Two facts that are cosine-NEAR (one word apart → they WILL pass the
    // embedding pre-filter and reach the NLI) but semantically OPPOSITE. Held
    // by different users so the collective merge sees both as candidates.
    let dark = format!("I prefer the dark color theme in editor build {run}.");
    let light = format!("I prefer the light color theme in editor build {run}.");

    let dark_id = client
        .add(&dark, &user_x, None, None)
        .await
        .expect("add dark")
        .memory_ids
        .into_iter()
        .next()
        .expect("a dark memory id");
    let light_id = client
        .add(&light, &user_y, None, None)
        .await
        .expect("add light")
        .memory_ids
        .into_iter()
        .next()
        .expect("a light memory id");

    let ck_dark_before = client.tooling().content_key_of(&dark_id).await;
    let ck_light_before = client.tooling().content_key_of(&light_id).await;
    assert_ne!(
        ck_dark_before, ck_light_before,
        "precondition: the two opposite prefs start in different fingerprint groups"
    );

    // Run the backstop with a permissive cosine pre-filter so the pair is
    // genuinely evaluated by the NLI (not skipped as too-dissimilar).
    // Scan a bounded recent window — the just-added pair is the most recent, so
    // it is always in scope, and the test stays fast.
    let summary = client
        .atropos()
        .merge_paraphrases(50, 0.6)
        .await
        .expect("merge_paraphrases");

    let ck_dark_after = client.tooling().content_key_of(&dark_id).await;
    let ck_light_after = client.tooling().content_key_of(&light_id).await;

    // THE guarantee: opposite preferences must remain in separate groups.
    assert_ne!(
        ck_dark_after, ck_light_after,
        "the backstop unified two CONTRADICTORY memories — this is the gaslighting \
         failure the NLI exists to prevent (summary={summary:?})"
    );
    // And the NLI must have actively evaluated + rejected this pair, proving the
    // guard is wired into the merge path (not merely correct in isolation).
    assert!(
        summary.contradictions_blocked >= 1,
        "the NLI must have blocked the dark/light merge — contradictions_blocked=0 \
         means the pair was never judged (broken wiring): {summary:?}"
    );

    println!("\n==== nli_antimerge_e2e ====");
    println!(
        "scanned={} candidates={} merged_groups={} contradictions_blocked={}",
        summary.scanned, summary.candidates, summary.merged_groups, summary.contradictions_blocked
    );
}
