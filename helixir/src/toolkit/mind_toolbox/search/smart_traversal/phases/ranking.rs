//! Final result ranking and filtering phase.

use super::*;

pub fn rank_and_filter(results: Vec<SearchResult>, min_combined_score: f64) -> Vec<SearchResult> {
    info!(
        "Starting Phase 3: Ranking and filtering {} results",
        results.len()
    );

    let mut best_scores: std::collections::HashMap<String, SearchResult> =
        std::collections::HashMap::new();

    for result in results {
        match best_scores.get(&result.memory_id) {
            Some(existing) => {
                if result.combined_score > existing.combined_score {
                    best_scores.insert(result.memory_id.clone(), result);
                }
            }
            None => {
                best_scores.insert(result.memory_id.clone(), result);
            }
        }
    }

    let mut filtered_results: Vec<SearchResult> = best_scores
        .into_values()
        .filter(|r| r.combined_score >= min_combined_score)
        .collect();

    filtered_results.sort_by(|a, b| {
        crate::toolkit::mind_toolbox::ranking::desc(&a.combined_score, &b.combined_score)
    });

    info!(
        "Phase 3 completed: {} final results",
        filtered_results.len()
    );
    filtered_results
}
