//! Pure scoring and candidate-selection helpers for smart traversal reranking.

use super::models::{SearchConfig, SearchResult};
use super::scoring::calculate_vector_combined_score_weighted;

/// Blend the real semantic signal with the phase-1 hybrid rank without
/// changing what `metadata.cosine` means to the write-side duplicate gate.
pub(super) fn rerank_seed(hit: &mut SearchResult, real_cosine: f64, config: &SearchConfig) -> bool {
    let previous_semantic = hit.vector_score;
    let hybrid = hit
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("phase1_hybrid"))
        .is_some();
    let semantic_score = if hybrid {
        let fusion_score = hit
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("rrf_rank_score"))
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(previous_semantic);
        // A document that is strong in only one arm is deliberately placed
        // behind dual-arm documents by RRF. The fused position therefore
        // cannot be the sole lexical relevance signal: a BM25 rank-1 exact
        // hit may sit late in the union and would be erased by cosine
        // reranking. Preserve the stronger of the fused order and the
        // document's native BM25 rank. This remains rank-based and avoids
        // mixing incomparable raw BM25/cosine magnitudes.
        let lexical_score = hit
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("bm25_rank_score"))
            .and_then(serde_json::Value::as_f64)
            .map_or(fusion_score, |bm25_score| bm25_score.max(fusion_score));
        blend_hybrid_relevance(
            real_cosine,
            lexical_score,
            config.hybrid_vector_weight,
            config.hybrid_bm25_weight,
        )
    } else {
        real_cosine
    };

    hit.vector_score = semantic_score;
    hit.combined_score = calculate_vector_combined_score_weighted(
        semantic_score,
        hit.temporal_score,
        config.vector_weight,
        config.temporal_weight,
    );
    let metadata = hit.metadata.get_or_insert_with(Default::default);
    metadata.insert("cosine".to_string(), serde_json::Value::from(real_cosine));
    metadata.insert(
        "semantic_score".to_string(),
        serde_json::Value::from(semantic_score),
    );

    (semantic_score - previous_semantic).abs() > 0.01
}

fn blend_hybrid_relevance(
    cosine: f64,
    fusion_score: f64,
    vector_weight: f64,
    bm25_weight: f64,
) -> f64 {
    let vector_weight = vector_weight.max(0.0);
    let bm25_weight = bm25_weight.max(0.0);
    let total = vector_weight + bm25_weight;
    if !total.is_finite() || total <= f64::EPSILON {
        return cosine.clamp(0.0, 1.0);
    }
    ((cosine * vector_weight + fusion_score * bm25_weight) / total).clamp(0.0, 1.0)
}

pub(super) fn preserve_direct_seed_score(
    source: &str,
    direct_retrieval_score: f64,
    hybrid_semantic_floor: f64,
    graph_blend: f64,
) -> f64 {
    if source == "vector" {
        graph_blend
            .max(direct_retrieval_score)
            .max(hybrid_semantic_floor)
    } else {
        graph_blend
    }
}

/// #88: indices of the top `cap` rows by score, descending. Pure so the
/// selection contract is unit-tested without an embedder.
pub(super) fn top_rerank_indices(scores: &[f64], cap: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..scores.len()).collect();
    idx.sort_by(|&a, &b| {
        scores[b]
            .partial_cmp(&scores[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    idx.truncate(cap);
    idx
}

#[cfg(test)]
mod tests {
    use super::{
        blend_hybrid_relevance, preserve_direct_seed_score, rerank_seed, top_rerank_indices,
    };
    use crate::toolkit::mind_toolbox::search::smart_traversal::{SearchConfig, SearchResult};
    use std::collections::HashMap;

    #[test]
    fn cap_larger_than_input_keeps_everything() {
        assert_eq!(top_rerank_indices(&[0.1, 0.9, 0.5], 10), vec![1, 2, 0]);
    }

    #[test]
    fn cap_selects_best_scores_not_first_rows() {
        // Discovery order is depth-order — the best candidates may sit late.
        let scores = [0.2, 0.1, 0.95, 0.3, 0.9];
        assert_eq!(top_rerank_indices(&scores, 2), vec![2, 4]);
    }

    #[test]
    fn nan_scores_do_not_panic_or_win() {
        let scores = [f64::NAN, 0.8, 0.4];
        let top = top_rerank_indices(&scores, 2);
        assert_eq!(top.len(), 2);
        assert!(top.contains(&1));
    }

    #[test]
    fn hybrid_seed_keeps_rrf_signal_while_exposing_pure_cosine() {
        let mut hit = SearchResult::from_vector("old", "exact lexical hit", 0.95, 0.0);
        hit.metadata = Some(HashMap::from([
            (
                "phase1_hybrid".to_string(),
                serde_json::Value::String("vector_rrf_bm25".to_string()),
            ),
            ("rrf_rank_score".to_string(), serde_json::Value::from(0.95)),
            ("bm25_rank".to_string(), serde_json::Value::from(1)),
            ("bm25_rank_score".to_string(), serde_json::Value::from(0.95)),
        ]));
        let config = SearchConfig::default();

        rerank_seed(&mut hit, 0.20, &config);

        let expected_semantic = blend_hybrid_relevance(0.20, 0.95, 0.6, 0.4);
        assert!((hit.vector_score - expected_semantic).abs() < 1e-9);
        assert!(hit.combined_score >= config.min_combined_score);
        let metadata = hit.metadata.expect("rerank metadata");
        assert_eq!(
            metadata.get("cosine").and_then(serde_json::Value::as_f64),
            Some(0.20)
        );
        assert_eq!(
            metadata
                .get("bm25_rank")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            metadata
                .get("rrf_rank_score")
                .and_then(serde_json::Value::as_f64),
            Some(0.95)
        );
    }

    #[test]
    fn bm25_rank_one_survives_a_late_rrf_union_position() {
        let mut hit = SearchResult::from_vector("old", "exact lexical hit", 0.20, 0.0);
        hit.metadata = Some(HashMap::from([
            (
                "phase1_hybrid".to_string(),
                serde_json::Value::String("vector_rrf_bm25".to_string()),
            ),
            ("rrf_rank_score".to_string(), serde_json::Value::from(0.20)),
            ("bm25_rank".to_string(), serde_json::Value::from(1)),
            ("bm25_rank_score".to_string(), serde_json::Value::from(0.95)),
        ]));
        let config = SearchConfig::default();

        rerank_seed(&mut hit, 0.50, &config);

        let expected_semantic = blend_hybrid_relevance(0.50, 0.95, 0.6, 0.4);
        assert!((hit.vector_score - expected_semantic).abs() < 1e-9);
        assert!(hit.combined_score >= config.min_combined_score);
    }

    #[test]
    fn non_hybrid_seed_still_uses_pure_cosine() {
        let mut hit = SearchResult::from_vector("plain", "vector hit", 0.95, 0.0);
        let config = SearchConfig::default();

        rerank_seed(&mut hit, 0.42, &config);

        assert!((hit.vector_score - 0.42).abs() < 1e-9);
        assert_eq!(
            hit.metadata
                .as_ref()
                .and_then(|m| m.get("cosine"))
                .and_then(serde_json::Value::as_f64),
            Some(0.42)
        );
    }

    #[test]
    fn direct_seed_score_is_a_floor_for_graph_reranking() {
        assert_eq!(preserve_direct_seed_score("vector", 0.66, 0.0, 0.49), 0.66);
        assert_eq!(preserve_direct_seed_score("vector", 0.40, 0.0, 0.70), 0.70);
        assert_eq!(preserve_direct_seed_score("graph", 0.66, 0.95, 0.49), 0.49);
    }

    #[test]
    fn bm25_hybrid_semantic_is_a_direct_seed_floor() {
        assert_eq!(preserve_direct_seed_score("vector", 0.66, 0.78, 0.49), 0.78);
    }
}
