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

        // Phase A: recall similar memories for every fact.
        let mut recall: Vec<Vec<SimilarMemory>> = Vec::with_capacity(memories_to_store.len());
        for (i, memory) in memories_to_store.iter().enumerate() {
            let vector = &all_embeddings[i];
            let similar_results = self
                .search_engine
                .search(
                    &memory.text,
                    vector,
                    user_id,
                    self.config.write.recall_top_k,
                    "contextual",
                    None,
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
                    // The duplicate gate needs the pure semantic signal:
                    // under algo_opt results carry raw cosine in metadata
                    // (the blended rank score never reaches the 0.98 bar,
                    // even for verbatim duplicates). Legacy results lack
                    // the key and keep the historic blended score.
                    score: r
                        .metadata
                        .get("cosine")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(r.score as f64),
                    memory_type: r
                        .metadata
                        .get("memory_type")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
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
            recall.push(similar_memories);
        }

        // Phase B: decisions. Under algo_opt all gray-zone facts are judged
        // in ONE LLM call (W1, #32); deterministic gates never reach the
        // model either way. Legacy keeps the per-fact loop.
        let batch_enabled = matches!(
            crate::core::RetrievalProfile::cached(),
            crate::core::RetrievalProfile::AlgoOpt
        ) && !std::env::var("HELIXIR_DISABLE_BATCH_DECISION")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let decisions: Vec<crate::llm::decision::MemoryDecision> =
            if batch_enabled && memories_to_store.len() > 1 {
                let items: Vec<(String, String, Vec<SimilarMemory>)> = memories_to_store
                    .iter()
                    .zip(recall.iter())
                    .map(|(m, sims)| (m.text.clone(), m.memory_type.clone(), sims.clone()))
                    .collect();
                self.decision_engine.decide_batch(&items, user_id).await
            } else {
                let mut out = Vec::with_capacity(memories_to_store.len());
                for (memory, sims) in memories_to_store.iter().zip(recall.iter()) {
                    out.push(
                        self.decision_engine
                            .decide(&memory.text, &memory.memory_type, sims, user_id)
                            .await,
                    );
                }
                out
            };

        // Phase C: execute.
        for (i, memory) in memories_to_store.iter().enumerate() {
            debug!(
                "Processing memory {}/{}: {}...",
                i,
                memories_to_store.len(),
                safe_truncate(&memory.text, 30)
            );
            let vector = &all_embeddings[i];
            let similar_memories = &recall[i];
            let decision = decisions[i].clone();

            info!(
                "Memory {} decision: {:?} (confidence={}, target={:?})",
                i, decision.operation, decision.confidence, decision.target_memory_id
            );

            // Charter escalation (memory-charter.md, flag-don't-block): the
            // decision still executes below; the conflict is surfaced to the
            // agent so it can ask the human or apply a learned rule.
            let target_id = decision
                .target_memory_id
                .as_deref()
                .or(decision.supersedes_memory_id.as_deref());
            let target_type = target_id.and_then(|tid| {
                similar_memories
                    .iter()
                    .find(|m| m.id == tid)
                    .and_then(|m| m.memory_type.as_deref())
            });
            if let Some(conflict_type) =
                crate::core::charter::escalation_reason(&decision, &memory.memory_type, target_type)
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

        if message.len() > self.config.write.raw_source_min_chars && added_ids.len() > 1 {
            let raw_mem = ExtractedMemory {
                text: message.to_string(),
                memory_type: "fact".to_string(),
                certainty: self.config.write.raw_source_certainty as i32,
                importance: self.config.write.raw_source_importance as i32,
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
