

use std::collections::HashMap;
use super::super::config::OntoSearchConfig;
use super::super::models::OntoSearchResult;


pub fn calculate_combined_score(result: &OntoSearchResult, config: &OntoSearchConfig) -> f64 {
    result.vector_score * config.vector_weight
        + result.concept_score * config.concept_weight
        + result.tag_score * config.tag_weight
        + result.graph_score * config.graph_weight
        + result.temporal_score * config.temporal_weight
}


pub fn rank_results(results: Vec<OntoSearchResult>, config: &OntoSearchConfig) -> Vec<OntoSearchResult> {
    
    let mut unique: HashMap<String, OntoSearchResult> = HashMap::new();

    for mut result in results {
        result.final_score = calculate_combined_score(&result, config);

        match unique.get(&result.memory_id) {
            Some(existing) if result.final_score > existing.final_score => {
                unique.insert(result.memory_id.clone(), result);
            }
            None => {
                unique.insert(result.memory_id.clone(), result);
            }
            _ => {}
        }
    }

    
    let mut ranked: Vec<OntoSearchResult> = unique
        .into_values()
        .filter(|r| r.final_score >= config.min_final_score)
        .collect();

    ranked.sort_by(|a, b| b.final_score.partial_cmp(&a.final_score).unwrap_or(std::cmp::Ordering::Equal));
    ranked
}
