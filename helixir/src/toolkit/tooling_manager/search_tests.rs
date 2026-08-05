use super::SearchMemoryResult;
use super::{collapse_collective_duplicates, concept_fallback_score};
use std::collections::HashMap;

fn res(memory_id: &str, content: &str, mtype: &str, score: f64) -> SearchMemoryResult {
    let mut metadata = HashMap::new();
    if !mtype.is_empty() {
        metadata.insert("memory_type".to_string(), serde_json::json!(mtype));
    }
    SearchMemoryResult {
        memory_id: memory_id.to_string(),
        internal_id: None,
        content: content.to_string(),
        score,
        method: "test".to_string(),
        metadata,
        created_at: String::new(),
    }
}

#[test]
fn collapse_folds_same_fact_across_users_keeping_best() {
    // Two users hold the SAME fact (same content+type -> same content_key),
    // plus one distinct fact. Whitespace/case differences must still fold.
    let input = vec![
        res("mem_a", "Rust is a systems language", "fact", 0.80), // user A
        res("mem_b", "rust  is a   systems language", "fact", 0.91), // user B (higher)
        res("mem_c", "Postgres is a database", "fact", 0.70),     // distinct
    ];
    let out = collapse_collective_duplicates(input);
    assert_eq!(out.len(), 2, "the duplicated fact must collapse to one row");
    // The surviving representative is the higher-scored copy.
    let folded = out
        .iter()
        .find(|r| r.content.contains("systems language"))
        .expect("folded fact present");
    assert_eq!(folded.memory_id, "mem_b", "keep the highest-scored holder");
    assert_eq!(
        folded
            .metadata
            .get("collapsed_holders")
            .and_then(|v| v.as_u64()),
        Some(2),
        "collapsed_holders must reflect both holders"
    );
    // The distinct fact is untouched and not annotated.
    let distinct = out
        .iter()
        .find(|r| r.content.contains("Postgres"))
        .expect("distinct fact present");
    assert!(!distinct.metadata.contains_key("collapsed_holders"));
}

#[test]
fn collapse_distinguishes_by_memory_type() {
    // Same text but different ontology type -> different content_key -> NOT folded.
    let input = vec![
        res("mem_a", "I prefer dark mode", "preference", 0.9),
        res("mem_b", "I prefer dark mode", "fact", 0.8),
    ];
    let out = collapse_collective_duplicates(input);
    assert_eq!(out.len(), 2, "different memory_type must not collapse");
}

#[test]
fn collapse_respects_persisted_rbac_scoped_content_keys() {
    let mut isolated_a = res("mem_a", "Shared wording", "fact", 0.9);
    isolated_a.metadata.insert(
        "content_key".to_string(),
        serde_json::json!("rbac:group:a:key"),
    );
    let mut isolated_b = res("mem_b", "Shared wording", "fact", 0.8);
    isolated_b.metadata.insert(
        "content_key".to_string(),
        serde_json::json!("rbac:group:b:key"),
    );

    let out = collapse_collective_duplicates(vec![isolated_a, isolated_b]);
    assert_eq!(
        out.len(),
        2,
        "identical text in isolated RBAC domains must not collapse"
    );
}

#[test]
fn concept_fallback_score_rewards_token_overlap() {
    let high = concept_fallback_score("rust gen keyword", "Rust 2024 reserves gen.", 80, 90);
    let low = concept_fallback_score("rust gen keyword", "I like coffee in the morning.", 80, 90);
    assert!(
        high > low,
        "overlap-heavy match must score above unrelated content: {high} <= {low}"
    );
}

#[test]
fn concept_fallback_score_uses_importance_when_overlap_is_zero() {
    let strong = concept_fallback_score("alpha beta", "Completely unrelated content.", 100, 100);
    let weak = concept_fallback_score("alpha beta", "Completely unrelated content.", 0, 0);
    assert!(strong > weak);
    assert!(strong <= 1.0);
    assert!(weak >= 0.0);
}

#[test]
fn concept_fallback_score_is_bounded() {
    // Saturating inputs must not let the score escape [0, 1].
    let high = concept_fallback_score("zzz", "zzz", 200, 200);
    assert!((0.0..=1.0).contains(&high));
}

#[test]
fn concept_fallback_score_handles_empty_query() {
    // An empty query should produce a non-NaN, bounded fallback driven
    // entirely by importance/certainty.
    let s = concept_fallback_score("", "anything", 50, 50);
    assert!(s.is_finite());
    assert!((0.0..=1.0).contains(&s));
}
