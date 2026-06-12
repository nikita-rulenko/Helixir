//! `add_memory` orchestrator: extract → embed → search → decide → enrich
//! → resolve cross-memory relations → preserve raw source. Every step
//! lives in a sibling module; this file is the conductor.

use std::collections::HashMap;

use tracing::{debug, info, warn};

use crate::llm::decision::SimilarMemory;
use crate::llm::extractor::ExtractedMemory;

use super::super::ToolingManager;
use super::super::types::{AddMemoryResult, ToolingError};
use crate::safe_truncate;

impl ToolingManager {
    pub async fn add_memory(
        &self,
        message: &str,
        user_id: &str,
        agent_id: Option<&str>,
        _metadata: Option<HashMap<String, serde_json::Value>>,
        context_tags: Option<&str>,
    ) -> Result<AddMemoryResult, ToolingError> {
        let preview: String = message.chars().take(50).collect();
        let tags = context_tags.unwrap_or("");
        info!(
            "Adding memory for user={}: {}... [tags={}]",
            user_id, preview, tags
        );

        debug!("Step 1: LLM extraction");
        let extraction = self
            .extractor
            .extract(message, user_id, true, true)
            .await
            .map_err(|e| ToolingError::Extraction(e.to_string()))?;

        info!(
            "Extracted {} memories, {} entities, {} relations",
            extraction.memories.len(),
            extraction.entities.len(),
            extraction.relations.len()
        );

        let mut added_ids = Vec::new();
        let mut updated_ids = Vec::new();
        let mut stored_memory_ids: HashMap<usize, String> = HashMap::new();
        let mut skipped = 0usize;
        let mut entities_linked = 0usize;
        let mut clarifications: Vec<super::super::types::Clarification> = Vec::new();
        let mut relations_created = 0usize;
        let mut chunks_created = 0usize;

        let memories_to_store = Self::prepare_memories_for_storage(extraction.memories, message);

        debug!(
            "Batch-generating embeddings for {} memories",
            memories_to_store.len()
        );
        let memory_texts: Vec<&str> = memories_to_store.iter().map(|m| m.text.as_str()).collect();
        let all_embeddings = self
            .embedder
            .generate_batch(&memory_texts, true)
            .await
            .map_err(|e| ToolingError::Embedding(e.to_string()))?;

        for (i, memory) in memories_to_store.iter().enumerate() {
            debug!(
                "Processing memory {}/{}: {}...",
                i,
                memories_to_store.len(),
                safe_truncate(&memory.text, 30)
            );

            let vector = &all_embeddings[i];

            let similar_results = self
                .search_engine
                .search(
                    &memory.text,
                    vector,
                    user_id,
                    5,
                    "contextual",
                    None,
                    "personal",
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
                })
                .collect();

            info!(
                "Memory {}: similar_count={}, top_score={:.3}",
                i,
                similar_memories.len(),
                similar_memories.first().map(|m| m.score).unwrap_or(0.0)
            );

            let decision = self
                .decision_engine
                .decide(&memory.text, &similar_memories, user_id)
                .await;

            info!(
                "Memory {} decision: {:?} (confidence={}, target={:?})",
                i, decision.operation, decision.confidence, decision.target_memory_id
            );

            // Charter escalation (memory-charter.md, flag-don't-block): the
            // decision still executes below; the conflict is surfaced to the
            // agent so it can ask the human or apply a learned rule.
            if let Some(conflict_type) =
                crate::core::charter::escalation_reason(&decision, &memory.memory_type)
            {
                let existing_content = decision.target_memory_id.as_deref().and_then(|tid| {
                    similar_memories
                        .iter()
                        .find(|m| m.id == tid)
                        .map(|m| m.content.clone())
                });
                clarifications.push(super::super::types::Clarification {
                    conflict_type: conflict_type.to_string(),
                    new_content: memory.text.clone(),
                    existing_memory_id: decision.target_memory_id.clone(),
                    existing_content: existing_content.clone(),
                    suggested_question: crate::core::charter::suggested_question(
                        conflict_type,
                        &memory.text,
                        existing_content.as_deref().unwrap_or(""),
                    ),
                    decision_taken: format!("{:?}", decision.operation),
                    confidence: decision.confidence,
                });
            }

            let memory_id = match self
                .handle_memory_operation(
                    &decision,
                    memory,
                    user_id,
                    agent_id,
                    tags,
                    vector,
                    &similar_memories,
                    &mut added_ids,
                    &mut updated_ids,
                    &mut skipped,
                    &mut chunks_created,
                    &mut relations_created,
                )
                .await?
            {
                Some(id) => id,
                None => continue,
            };

            stored_memory_ids.insert(i, memory_id.clone());

            let (linked, rels) = self
                .enrich_memory_relations(
                    &memory_id,
                    memory,
                    &extraction.entities,
                    &similar_memories,
                    &decision,
                )
                .await?;

            entities_linked += linked;
            relations_created += rels;
        }

        relations_created += self
            .resolve_and_persist_extraction_relations(
                &extraction.relations,
                &memories_to_store,
                &stored_memory_ids,
            )
            .await?;

        if message.len() > 100 && added_ids.len() > 1 {
            let raw_mem = ExtractedMemory {
                text: message.to_string(),
                memory_type: "fact".to_string(),
                certainty: 70,
                importance: 40,
                entities: vec![],
                context: None,
            };
            match self.embedder.generate(message, true).await {
                Ok(raw_vec) => {
                    match self
                        .store_raw_source(&raw_mem, user_id, &raw_vec, tags)
                        .await
                    {
                        Ok(raw_id) => {
                            debug!("Raw source stored: {}", raw_id);
                            chunks_created += 1;
                        }
                        Err(e) => warn!("Failed to store raw source: {}", e),
                    }
                }
                Err(e) => warn!("Failed to embed raw source: {}", e),
            }
        }

        info!(
            "Memory pipeline complete: {} added, {} updated, {} skipped, {} entities, {} relations",
            added_ids.len(),
            updated_ids.len(),
            skipped,
            entities_linked,
            relations_created
        );

        let mut metadata = HashMap::new();
        metadata.insert(
            "provider".to_string(),
            serde_json::Value::String(self.llm_provider.provider_name().to_string()),
        );
        metadata.insert(
            "model".to_string(),
            serde_json::Value::String(self.llm_provider.model_name().to_string()),
        );
        metadata.insert(
            "user_id".to_string(),
            serde_json::Value::String(user_id.to_string()),
        );

        Ok(AddMemoryResult {
            added: added_ids,
            updated: updated_ids,
            deleted: vec![],
            skipped,
            entities_extracted: entities_linked,
            reasoning_relations_created: relations_created,
            chunks_created,
            metadata,
            needs_clarification: clarifications,
        })
    }
}
