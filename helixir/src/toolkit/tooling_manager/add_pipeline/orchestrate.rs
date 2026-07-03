//! `add_memory` orchestrator: extract → embed → search → decide → enrich
//! → resolve cross-memory relations → preserve raw source. Every step
//! lives in a sibling module; this file is the conductor.

use std::collections::HashMap;

use tracing::{debug, info, warn};

use crate::llm::decision::{MemoryOperation, SimilarMemory};
use crate::llm::extractor::{ExtractedEntity, ExtractedMemory, ExtractedRelation};

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

        let memories_to_store = self.prepare_memories_for_storage(extraction.memories, message);
        self.run_add_pipeline(
            memories_to_store,
            &extraction.entities,
            &extraction.relations,
            Some(message),
            user_id,
            agent_id,
            tags,
        )
        .await
    }

    /// LLM-free entry for callers that ALREADY hold structured atoms (FastThink
    /// commit, future importers): the same pipeline as `add_memory` minus the
    /// extraction call — embeddings, recall, the batched decision phase (dedup
    /// and charter safety stay), storage, chunking and edges run unchanged.
    /// No raw-source preservation: the caller's atoms ARE the source.
    pub async fn add_prepared_memories(
        &self,
        memories: Vec<ExtractedMemory>,
        user_id: &str,
        agent_id: Option<&str>,
        context_tags: Option<&str>,
    ) -> Result<AddMemoryResult, ToolingError> {
        info!(
            "Adding {} prepared memories for user={} (no extraction)",
            memories.len(),
            user_id
        );
        self.run_add_pipeline(
            memories,
            &[],
            &[],
            None,
            user_id,
            agent_id,
            context_tags.unwrap_or(""),
        )
        .await
    }

    /// The shared post-extraction pipeline: embed → recall → decide → execute
    /// → cross-memory relations → optional raw-source preservation.
    #[allow(clippy::too_many_arguments)]
    async fn run_add_pipeline(
        &self,
        memories_to_store: Vec<ExtractedMemory>,
        extracted_entities: &[ExtractedEntity],
        extracted_relations: &[ExtractedRelation],
        raw_message: Option<&str>,
        user_id: &str,
        agent_id: Option<&str>,
        tags: &str,
    ) -> Result<AddMemoryResult, ToolingError> {
        let mut added_ids = Vec::new();
        let mut updated_ids = Vec::new();
        let mut deduped_ids = Vec::new();
        let mut stored_memory_ids: HashMap<usize, String> = HashMap::new();
        let mut skipped = 0usize;
        let mut entities_linked = 0usize;
        let mut clarifications: Vec<super::super::types::Clarification> = Vec::new();
        let mut relations_created = 0usize;
        let mut chunks_created = 0usize;
        // (memory_id, text, context pairs) per stored atom — the LLM relation
        // inference runs over these concurrently in Phase D.
        let mut infer_jobs: Vec<(String, String, Vec<(String, String)>)> = Vec::new();

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
            let mut decision = decisions[i].clone();

            info!(
                "Memory {} decision: {:?} (confidence={}, target={:?})",
                i, decision.operation, decision.confidence, decision.target_memory_id
            );

            // Charter escalation (memory-charter.md): surfaced to the agent;
            // destructive verdicts additionally DEFER under charter_blocking.
            let target_id: Option<String> = decision
                .target_memory_id
                .clone()
                .or_else(|| decision.supersedes_memory_id.clone());
            let target_type = target_id.as_deref().and_then(|tid| {
                similar_memories
                    .iter()
                    .find(|m| m.id == tid)
                    .and_then(|m| m.memory_type.as_deref())
            });
            // Charter increment 2 (#34): under blocking, a destructive verdict
            // that the charter escalates is DEFERRED — the new fact is stored
            // as an ADD next to the old one, the dispute lives on a
            // charter_deferred CONTRADICTS edge, and resolve_contradiction
            // settles it (retract = the supersede happens then). Nothing is
            // rewritten until a human-level answer exists.
            let mut deferred_target: Option<String> = None;
            if let Some(conflict_type) = crate::core::charter::escalation_reason(
                &decision,
                &memory.memory_type,
                target_type,
                self.config.write.charter_low_confidence,
            ) {
                let existing_content = decision.target_memory_id.as_deref().and_then(|tid| {
                    similar_memories
                        .iter()
                        .find(|m| m.id == tid)
                        .map(|m| m.content.clone())
                });
                let blocking = self.config.write.charter_blocking
                    && crate::core::charter::defers_under_blocking(&decision)
                    && target_id.is_some();
                let decision_taken = if blocking {
                    let was = decision.operation;
                    deferred_target = target_id.clone();
                    info!(
                        "Charter blocking: {:?} of {} DEFERRED ({conflict_type})",
                        decision.operation,
                        deferred_target.as_deref().unwrap_or("?")
                    );
                    decision.operation = crate::llm::decision::MemoryOperation::Add;
                    format!(
                        "DEFERRED (was {was:?}): both facts stored; settle with resolve_contradiction(from_id=<new>, to_id=<existing>, resolution=confirm|retract|preference)"
                    )
                } else {
                    format!("{:?}", decision.operation)
                };
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
                    decision_taken,
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
                    &mut deduped_ids,
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

            if let Some(old_id) = &deferred_target {
                if let Err(e) = self
                    .record_contradiction(&memory_id, old_id, "charter_deferred")
                    .await
                {
                    warn!("Charter blocking: deferred edge {memory_id} -> {old_id} failed: {e}");
                }
            }

            entities_linked += self
                .link_memory_semantics(&memory_id, memory, extracted_entities)
                .await?;

            let should_infer = !similar_memories.is_empty()
                && !matches!(
                    decision.operation,
                    MemoryOperation::Noop | MemoryOperation::Delete
                );
            if should_infer {
                let context_pairs: Vec<(String, String)> = similar_memories
                    .iter()
                    .take(self.config.write.relation_inference_context_k)
                    .map(|s| (s.id.clone(), s.content.clone()))
                    .collect();
                infer_jobs.push((memory_id, memory.text.clone(), context_pairs));
            }
        }

        // Phase D: relation inference — one independent LLM call per stored
        // atom. These used to run sequentially inside the store loop, stacking
        // K× model latency onto every multi-atom write; concurrent, the
        // wall-clock cost is the slowest single call.
        if !infer_jobs.is_empty() {
            let inferred = futures::future::join_all(
                infer_jobs
                    .iter()
                    .map(|(id, text, pairs)| self.infer_and_persist_relations(id, text, pairs)),
            )
            .await;
            relations_created += inferred.into_iter().sum::<usize>();
        }

        relations_created += self
            .resolve_and_persist_extraction_relations(
                extracted_relations,
                &memories_to_store,
                &stored_memory_ids,
            )
            .await?;

        // Deterministic causal floor (#66): an explicit connective in the RAW
        // message with >=2 stored atoms and ZERO relations from the whole
        // pipeline gets a BECAUSE edge wired by clause alignment — "reasons
        // in chains" must not depend on the model's mood (or its fallback
        // tier). The LLM path stays first; this fires only when it gave nothing.
        if relations_created == 0 && stored_memory_ids.len() >= 2 {
            if let Some(message) = raw_message {
                if let Some((cause_text, effect_text)) =
                    super::connective_backstop::split_causal(message)
                {
                    let mut idx: Vec<usize> = stored_memory_ids.keys().copied().collect();
                    idx.sort_unstable();
                    let atom_texts: Vec<&str> = idx
                        .iter()
                        .map(|i| memories_to_store[*i].text.as_str())
                        .collect();
                    if let Some((c, e)) = super::connective_backstop::pick_cause_effect(
                        &atom_texts,
                        &cause_text,
                        &effect_text,
                    ) {
                        let from = &stored_memory_ids[&idx[c]];
                        let to = &stored_memory_ids[&idx[e]];
                        match self
                            .reasoning_engine
                            .add_relation(
                                from,
                                to,
                                crate::toolkit::mind_toolbox::reasoning::ReasoningType::Because,
                                55,
                                None,
                            )
                            .await
                        {
                            Ok(_) => {
                                relations_created += 1;
                                info!(
                                    "connective backstop: BECAUSE {} -> {} (extractor emitted no relations for an explicitly causal message)",
                                    safe_truncate(from, 12),
                                    safe_truncate(to, 12)
                                );
                            }
                            Err(err) => warn!("connective backstop failed: {err}"),
                        }
                    }
                }
            }
        }

        if let Some(message) = raw_message
            .filter(|m| m.len() > self.config.write.raw_source_min_chars && added_ids.len() > 1)
        {
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
                            // #82: family link — every atom points at the raw
                            // it was extracted from, so search can collapse a
                            // raw and its atoms into one result instead of
                            // billing the same content twice.
                            for atom_id in &added_ids {
                                if let Err(e) = self
                                    .add_typed_relation(
                                        atom_id,
                                        &raw_id,
                                        crate::toolkit::mind_toolbox::reasoning::ReasoningType::PartOf,
                                        self.config.write.raw_part_of_strength,
                                    )
                                    .await
                                {
                                    warn!(
                                        "PART_OF link {} -> {} failed: {}",
                                        atom_id, raw_id, e
                                    );
                                }
                            }
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
            deduped: deduped_ids,
            skipped,
            entities_extracted: entities_linked,
            reasoning_relations_created: relations_created,
            chunks_created,
            metadata,
            needs_clarification: clarifications,
        })
    }
}
