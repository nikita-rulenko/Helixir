use chrono::{DateTime, Utc};

use crate::toolkit::mind_toolbox::ranking::sanitize_unit;

/// Cosine **score** in `[0, 1]` — the cosine similarity of two embedding
/// vectors, affinely remapped from its mathematical range `[-1, 1]`.
///
/// Returned value is `(cos + 1) / 2`, i.e. orthogonal vectors yield `0.5`,
/// identical yield `1.0`, antiparallel yield `0.0`. This is the form
/// required by the rerank step in `traversal.rs`, where it is mixed with
/// the temporal score under a `clamp(0, 1)` (negative values would silently
/// invert the combined score otherwise).
///
/// Distinct from the mathematical `cosine_similarity` (range `[-1, 1]`) by
/// design — see issue #25 / `helixir/doc/duplication-audit.md` D1 for the
/// historical duplication this name resolves.
pub fn cosine_score(vec1: &[f32], vec2: &[f32]) -> f64 {
    if vec1.is_empty() || vec2.is_empty() || vec1.len() != vec2.len() {
        return 0.0;
    }

    let dot_product: f32 = vec1.iter().zip(vec2.iter()).map(|(a, b)| a * b).sum();
    let mag1: f32 = vec1.iter().map(|a| a * a).sum::<f32>().sqrt();
    let mag2: f32 = vec2.iter().map(|b| b * b).sum::<f32>().sqrt();

    if mag1 == 0.0 || mag2 == 0.0 {
        return 0.0;
    }

    let similarity = f64::from(dot_product / (mag1 * mag2));

    ((similarity + 1.0) / 2.0).clamp(0.0, 1.0)
}

pub fn calculate_temporal_freshness(created_at: &str, decay_days: f64) -> f64 {
    let created = match DateTime::parse_from_rfc3339(created_at) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => {
            if let Ok(dt) = created_at.replace('Z', "+00:00").parse::<DateTime<Utc>>() {
                dt
            } else {
                return 0.5;
            }
        }
    };

    let now = Utc::now();
    let duration = now.signed_duration_since(created);
    let days_old = duration.num_seconds() as f64 / 86400.0;

    let freshness = (-days_old / decay_days).exp();
    freshness.clamp(0.0, 1.0)
}

pub fn calculate_vector_combined_score(vector_score: f64, temporal_score: f64) -> f64 {
    calculate_vector_combined_score_weighted(vector_score, temporal_score, 0.7, 0.3)
}

pub fn calculate_vector_combined_score_weighted(
    vector_score: f64,
    temporal_score: f64,
    vector_weight: f64,
    temporal_weight: f64,
) -> f64 {
    // sanitize_unit (not bare clamp): a NaN vector score from a degenerate
    // HelixDB vector would otherwise pass straight through clamp and poison
    // the ranking sort downstream (#41).
    sanitize_unit(vector_score * vector_weight + temporal_score * temporal_weight)
}

pub fn calculate_graph_combined_score(
    semantic_sim: f64,
    graph_score: f64,
    temporal_score: f64,
) -> f64 {
    calculate_graph_combined_score_weighted(
        semantic_sim,
        graph_score,
        temporal_score,
        0.3,
        0.5,
        0.2,
    )
}

pub fn calculate_graph_combined_score_weighted(
    semantic_sim: f64,
    graph_score: f64,
    temporal_score: f64,
    semantic_weight: f64,
    graph_weight: f64,
    temporal_weight: f64,
) -> f64 {
    sanitize_unit(
        semantic_sim * semantic_weight
            + graph_score * graph_weight
            + temporal_score * temporal_weight,
    )
}

pub fn calculate_graph_score(edge_weight: f64, parent_score: f64) -> f64 {
    sanitize_unit(edge_weight * parent_score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_score_identical_is_one() {
        let vec1 = vec![1.0, 0.0, 0.0];
        let vec2 = vec![1.0, 0.0, 0.0];
        let sim = cosine_score(&vec1, &vec2);
        assert!((sim - 1.0).abs() < 0.01);
    }

    #[test]
    fn cosine_score_orthogonal_is_half() {
        let vec1 = vec![1.0, 0.0, 0.0];
        let vec2 = vec![0.0, 1.0, 0.0];
        let sim = cosine_score(&vec1, &vec2);
        assert!((sim - 0.5).abs() < 0.01);
    }

    #[test]
    fn cosine_score_antiparallel_is_zero() {
        let vec1 = vec![1.0, 0.0, 0.0];
        let vec2 = vec![-1.0, 0.0, 0.0];
        let sim = cosine_score(&vec1, &vec2);
        assert!((sim - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_temporal_freshness_now() {
        let now = Utc::now().to_rfc3339();
        let freshness = calculate_temporal_freshness(&now, 30.0);
        assert!(freshness > 0.99);
    }

    #[test]
    fn test_temporal_freshness_old() {
        let old = (Utc::now() - chrono::Duration::days(90)).to_rfc3339();
        let freshness = calculate_temporal_freshness(&old, 30.0);

        assert!(freshness < 0.1);
    }

    #[test]
    fn test_combined_scores() {
        let vector_combined = calculate_vector_combined_score(0.8, 0.9);
        assert!((vector_combined - 0.83).abs() < 0.01);

        let graph_combined = calculate_graph_combined_score(0.5, 0.8, 0.9);

        assert!((graph_combined - 0.73).abs() < 0.01);
    }

    // --- #41 regression: a NaN score must never reach an unwrap'd sort ---
    //
    // The combine functions now route through `sanitize_unit`, so a NaN
    // vector score (e.g. a degenerate HelixDB phase-1 vector) is mapped to
    // 0.0 at the scoring boundary instead of passing through `.clamp()` and
    // panicking the downstream ranking sort. The NaN-safety of the sort
    // comparator itself is covered in `ranking::tests`.
    //
    // Pre-fix this asserted `combined.is_nan()` (the bug); it now asserts the
    // sanitized behaviour.

    #[test]
    fn combine_sanitizes_a_nan_vector_score_to_zero() {
        let combined = calculate_vector_combined_score_weighted(f64::NAN, 0.5, 0.7, 0.3);
        assert!(
            combined.is_finite() && combined == 0.0,
            "a NaN input must be sanitized to 0.0, got {combined}"
        );
    }

    #[test]
    fn combine_sanitizes_a_nan_semantic_score_to_zero() {
        let combined = calculate_graph_combined_score_weighted(f64::NAN, 0.8, 0.9, 0.3, 0.5, 0.2);
        assert!(
            combined.is_finite(),
            "a NaN input must not propagate, got {combined}"
        );
    }

    #[test]
    fn test_rank_based_scoring_discrimination() {
        const RANK_BASE: f64 = 0.95;
        const RANK_DECAY: f64 = 0.92;

        let scores: Vec<f64> = (0..10)
            .map(|rank| RANK_BASE * RANK_DECAY.powi(rank))
            .collect();

        assert!(scores[0] > 0.94, "Top result should be ~0.95");
        assert!(scores[9] < 0.50, "Rank 9 should be below 0.50");

        let spread = scores[0] - scores[9];
        assert!(
            spread > 0.4,
            "Score spread should be >0.4 for 10 results, got {spread}"
        );

        for w in scores.windows(2) {
            assert!(w[0] > w[1], "Scores must be strictly decreasing");
        }
    }
}
