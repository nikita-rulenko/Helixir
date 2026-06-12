//! Reciprocal Rank Fusion (Cormack et al., SIGIR 2009) for merging two ranked lists.
//!
//! Helixir uses the standard `k = 60` smoothing constant. Score-agnostic — no need
//! to normalise BM25 vs cosine magnitudes.

const DEFAULT_RRF_K: f64 = 60.0;

/// Fuses two ranked lists of memory IDs into a single ordering (highest RRF first).
/// IDs appearing in only one list still receive a non-zero contribution from that list.
/// 1-based ranks: top item has rank 1.
pub fn fused_memory_order(list_vector: &[String], list_bm25: &[String]) -> Vec<String> {
    fused_memory_order_with_k(list_vector, list_bm25, DEFAULT_RRF_K)
}

pub(crate) fn fused_memory_order_with_k(
    list_vector: &[String],
    list_bm25: &[String],
    k: f64,
) -> Vec<String> {
    use std::collections::HashMap;
    let mut scores: HashMap<&str, f64> = HashMap::new();

    for (i, id) in list_vector.iter().enumerate() {
        if id.is_empty() {
            continue;
        }
        let rank = (i + 1) as f64;
        *scores.entry(id.as_str()).or_insert(0.0) += 1.0 / (k + rank);
    }

    for (i, id) in list_bm25.iter().enumerate() {
        if id.is_empty() {
            continue;
        }
        let rank = (i + 1) as f64;
        *scores.entry(id.as_str()).or_insert(0.0) += 1.0 / (k + rank);
    }

    let mut pairs: Vec<(&str, f64)> = scores.into_iter().collect();
    pairs.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(b.0))
    });

    pairs.into_iter().map(|(id, _)| id.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrf_prefers_doc_strong_in_both_lists() {
        let v = vec!["a".into(), "b".into(), "c".into()];
        let b = vec!["b".into(), "c".into(), "d".into()];
        let fused = fused_memory_order(&v, &b);
        assert_eq!(
            fused.first().map(String::as_str),
            Some("b"),
            "b is #2 in vector list and #1 in BM25 — should outrank a (vector-only #1)"
        );
    }

    #[test]
    fn rrf_single_list_is_identity_order() {
        let v = vec!["x".into(), "y".into()];
        let b: Vec<String> = vec![];
        let fused = fused_memory_order(&v, &b);
        assert_eq!(fused, vec!["x", "y"]);
    }
}
