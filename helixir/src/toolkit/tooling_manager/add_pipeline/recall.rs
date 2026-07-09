//! Reserved personal-only deduplication probe. Currently unused — the live
//! pipeline goes through the global `search_engine.search(..., "personal")`
//! call inside [`super::orchestrate`]. Kept until that fast-path stabilises.

use crate::llm::decision::SimilarMemory;

use super::super::{ToolingError, ToolingManager};

impl ToolingManager {
    // Reserved for the upcoming personal-only deduplication path (no Hive
    // fan-out). Currently `embed_and_search_global` is used end-to-end.
    #[allow(dead_code)]
    pub(super) async fn embed_and_search_personal(
        &self,
        text: &str,
        user_id: &str,
    ) -> Result<(Vec<f32>, Vec<SimilarMemory>), ToolingError> {
        let vector = self
            .embedder
            .generate(text, true)
            .await
            .map_err(|e| ToolingError::Embedding(e.to_string()))?;

        let similar_results = self
            .search_engine
            .search(
                text,
                &vector,
                user_id,
                crate::toolkit::mind_toolbox::search::SearchOptions::new(5, "contextual"),
            )
            .await
            .unwrap_or_default();

        let similar_memories: Vec<SimilarMemory> = similar_results
            .iter()
            .map(|r| SimilarMemory {
                id: r.memory_id.clone(),
                content: r.content.clone(),
                score: r.score as f64,
                created_at: None,
                user_id: None,
                is_cross_user: false,
                memory_type: r
                    .metadata
                    .get("memory_type")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            })
            .collect();

        Ok((vector, similar_memories))
    }
}
