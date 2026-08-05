//! Deterministic fallback scoring for concept search.

/// Score function used in the `search_by_concept` DB fallback path.
///
/// We don't have a real vector similarity at this point (we already fell
/// back to `getUserMemories` precisely because vector search returned
/// nothing), so the score is a deterministic mix of two cheap signals:
///
/// * **Token-overlap** between the query and the memory content
///   (`|q ∩ c| / |q|`). Cheap, language-agnostic, and good enough to
///   discriminate "this memory is on-topic" from "this memory happens to
///   share a `memory_type`".
/// * **Author's own importance + certainty** averaged into a [0, 1]
///   confidence proxy. Stops near-zero-overlap matches from being ranked
///   above well-attested but slightly off-topic ones.
///
/// The final score is `0.7 * overlap + 0.3 * confidence`, clamped to
/// `[0, 1]`. Replaces the constant `0.75` from issue #22.
pub(super) fn concept_fallback_score(
    query_lower: &str,
    memory_content: &str,
    importance: i64,
    certainty: i64,
) -> f64 {
    let query_tokens: std::collections::HashSet<&str> = query_lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 2)
        .collect();

    let overlap = if query_tokens.is_empty() {
        0.0
    } else {
        let content_lower = memory_content.to_lowercase();
        let hit = query_tokens
            .iter()
            .filter(|t| content_lower.contains(*t))
            .count();
        hit as f64 / query_tokens.len() as f64
    };

    let importance = importance.clamp(0, 100) as f64 / 100.0;
    let certainty = certainty.clamp(0, 100) as f64 / 100.0;
    let confidence = (importance + certainty) / 2.0;

    (0.7 * overlap + 0.3 * confidence).clamp(0.0, 1.0)
}
