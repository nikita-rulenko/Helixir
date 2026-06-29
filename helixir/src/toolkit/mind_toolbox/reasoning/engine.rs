//! [`ReasoningEngine`] — owns the LRU relation cache and the `HelixClient`
//! handle. CRUD methods over reasoning edges live in [`super::edges`],
//! chain traversal in [`super::chain`], LLM inference in [`super::infer`].

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use lru::LruCache;
use serde::Deserialize;
use tracing::{debug, info};

use crate::db::HelixClient;
use crate::llm::providers::base::LlmProvider;

use super::types::{CacheStats, ReasoningError, ReasoningRelation, ReasoningType};

pub struct ReasoningEngine {
    pub(super) client: Arc<HelixClient>,
    pub(super) llm_provider: Option<Arc<dyn LlmProvider>>,
    pub(super) relation_cache: parking_lot::Mutex<LruCache<String, ReasoningRelation>>,
    pub(super) cache_size: usize,
    pub(super) is_warmed_up: AtomicBool,
}

impl ReasoningEngine {
    #[must_use]
    pub fn new(
        client: Arc<HelixClient>,
        llm_provider: Option<Arc<dyn LlmProvider>>,
        cache_size: usize,
    ) -> Self {
        let cache = LruCache::new(
            NonZeroUsize::new(cache_size).unwrap_or(NonZeroUsize::new(1000).unwrap()),
        );

        info!(
            "ReasoningEngine initialized (cache_size={}, llm={})",
            cache_size,
            if llm_provider.is_some() {
                "enabled"
            } else {
                "disabled"
            }
        );

        Self {
            client,
            llm_provider,
            relation_cache: parking_lot::Mutex::new(cache),
            cache_size,
            is_warmed_up: AtomicBool::new(false),
        }
    }

    pub async fn warm_up_cache(
        &self,
        memory_id: Option<&str>,
        limit: usize,
    ) -> Result<usize, ReasoningError> {
        use std::sync::atomic::Ordering;

        if self.is_warmed_up.load(Ordering::Relaxed) {
            info!("Reasoning cache already warmed up, skipping");
            return Ok(self.relation_cache.lock().len());
        }

        info!(
            "Warming up reasoning cache (memory={:?}, limit={})",
            memory_id, limit
        );

        #[derive(Deserialize)]
        struct QueryResult {
            relations: Option<Vec<serde_json::Value>>,
        }

        match self
            .client
            .execute_query::<QueryResult, _>(
                "getRecentRelations",
                &serde_json::json!({
                    "limit": limit,
                    "memory_id": memory_id,
                }),
            )
            .await
        {
            Ok(result) => {
                let relations = result.relations.map(|r| r.len()).unwrap_or(0);
                self.is_warmed_up.store(true, Ordering::Relaxed);
                info!("Cache warmup complete: {} relations loaded", relations);
                Ok(relations)
            }
            Err(e) => {
                debug!("Cache warmup skipped (query not available): {}", e);
                Ok(0)
            }
        }
    }

    #[must_use]
    pub fn get_cache_stats(&self) -> CacheStats {
        use std::sync::atomic::Ordering;
        CacheStats {
            size: self.relation_cache.lock().len(),
            capacity: self.cache_size,
            is_warmed_up: self.is_warmed_up.load(Ordering::Relaxed),
        }
    }

    /// Renders a compact arrow-trail like `[mem_aaaa] → [mem_bbbb] ← [mem_cccc]`
    /// from an ordered slice of relations. Used by [`super::chain`] to attach
    /// a human-readable string to each [`super::types::ReasoningChain`].
    pub(super) fn build_reasoning_trail(&self, relations: &[ReasoningRelation]) -> String {
        if relations.is_empty() {
            return "No reasoning chain found.".to_string();
        }

        let mut trail = String::new();
        for (i, rel) in relations.iter().enumerate() {
            let arrow = match rel.relation_type {
                ReasoningType::Implies => "→",
                ReasoningType::Because => "←",
                ReasoningType::Contradicts => "⊗",
                ReasoningType::Supports => "↔",
                ReasoningType::RelatesTo => "~",
                ReasoningType::PartOf => "⊂",
                ReasoningType::IsA => "≼",
            };

            if i > 0 {
                trail.push(' ');
            }
            trail.push_str(&format!(
                "[{}] {} [{}]",
                crate::safe_truncate(&rel.from_memory_id, 8),
                arrow,
                crate::safe_truncate(&rel.to_memory_id, 8)
            ));
        }

        trail
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolkit::mind_toolbox::reasoning::types::ReasoningRelation;

    #[test]
    fn test_build_reasoning_trail() {
        let relations = vec![
            ReasoningRelation {
                peer_memory_id: String::new(),
                peer_memory_content: String::new(),
                relation_id: "r1".to_string(),
                from_memory_id: "mem_aaaa".to_string(),
                to_memory_id: "mem_bbbb".to_string(),
                to_memory_content: "test content".to_string(),
                from_memory_content: String::new(),
                relation_type: ReasoningType::Implies,
                strength: 90,
                reasoning_id: None,
            },
            ReasoningRelation {
                peer_memory_id: String::new(),
                peer_memory_content: String::new(),
                relation_id: "r2".to_string(),
                from_memory_id: "mem_bbbb".to_string(),
                to_memory_id: "mem_cccc".to_string(),
                to_memory_content: "test content".to_string(),
                from_memory_content: String::new(),
                relation_type: ReasoningType::Because,
                strength: 85,
                reasoning_id: None,
            },
        ];

        let client = Arc::new(crate::db::HelixClient::new("localhost", 6969).unwrap());
        let engine = ReasoningEngine::new(client, None, 100);
        let trail = engine.build_reasoning_trail(&relations);

        assert!(trail.contains("→"));
        assert!(trail.contains("←"));
    }
}
