use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio;
use tracing::info;

use super::bm25::Bm25Search;
use super::models::{SearchResult, SearchMethod};
use super::vector::{VectorSearch, VectorSearchError};

#[derive(Error, Debug)]
pub enum HybridSearchError {
    #[error("Vector search error: {0}")]
    VectorSearch(#[from] VectorSearchError),
    #[error("Invalid weights: sum must be > 0")]
    InvalidWeights,
}

pub struct HybridSearch {
    vector_search: Arc<VectorSearch>,
    vector_weight: f64,
    bm25_weight: f64,
}

impl HybridSearch {
    pub fn new(
        vector_search: Arc<VectorSearch>,
        vector_weight: f64,
        bm25_weight: f64,
    ) -> Self {
        let total_weight = vector_weight + bm25_weight;
        let normalized_vector_weight = if total_weight > 0.0 { vector_weight / total_weight } else { 0.5 };
        let normalized_bm25_weight = if total_weight > 0.0 { bm25_weight / total_weight } else { 0.5 };

        Self {
            vector_search,
            vector_weight: normalized_vector_weight,
            bm25_weight: normalized_bm25_weight,
        }
    }

    pub async fn search(
        &self,
        query: &str,
        user_id: Option<&str>,
        documents: Option<&[(String, String)]>,
        limit: usize,
    ) -> Result<Vec<SearchResult>, HybridSearchError> {
        let vector_future = self.vector_search.search(query, user_id, limit * 2, 0.0, true);
        let bm25_future = async {
            if let Some(docs) = documents {
                Bm25Search::search(query, docs, limit * 2, 0.0)
            } else {
                Vec::new()
            }
        };

        let (vector_results, bm25_results) = tokio::join!(vector_future, bm25_future);
        let vector_results = vector_results?;
        let bm25_results = bm25_results;

        let mut combined_scores: HashMap<String, (String, String, f64, HashMap<String, f64>)> = HashMap::new();

        for result in vector_results {
            let score = result.score * self.vector_weight;
            let mut metadata = HashMap::new();
            metadata.insert("vector".to_string(), result.score);
            combined_scores.insert(
                result.memory_id.clone(),
                (result.memory_id.clone(), result.content.clone(), score, metadata),
            );
        }

        for result in bm25_results {
            let score = result.score * self.bm25_weight;
            if let Some((_, _, existing_score, metadata)) = combined_scores.get_mut(&result.memory_id) {
                *existing_score += score;
                metadata.insert("bm25".to_string(), result.score);
            } else {
                let mut metadata = HashMap::new();
                metadata.insert("bm25".to_string(), result.score);
                combined_scores.insert(
                    result.memory_id.clone(),
                    (result.memory_id.clone(), result.content.clone(), score, metadata),
                );
            }
        }

        let mut results: Vec<SearchResult> = combined_scores
            .into_values()
            .map(|(memory_id, content, score, method_meta)| SearchResult {
                memory_id,
                content,
                score,
                method: SearchMethod::Hybrid,
                metadata: method_meta.into_iter()
                    .map(|(k, v)| (k, serde_json::json!(v)))
                    .collect(),
                created_at: String::new(),
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        info!("Hybrid search returned {} results", results.len().min(limit));
        Ok(results.into_iter().take(limit).collect())
    }
}