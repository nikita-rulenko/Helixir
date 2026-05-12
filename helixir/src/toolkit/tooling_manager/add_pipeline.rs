use std::collections::HashMap;

use serde::Serialize;
use tracing::{debug, info, warn};

use crate::llm::decision::{MemoryDecision, MemoryOperation, SimilarMemory};
use crate::llm::extractor::{ExtractedEntity, ExtractedMemory, ExtractedRelation};
use crate::toolkit::mind_toolbox::entity::EntityEdgeType;
use crate::toolkit::mind_toolbox::reasoning::ReasoningType;

use super::ToolingManager;
use super::helpers::safe_truncate;
use super::types::{AddMemoryResult, ToolingError};

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
        })
    }

    fn prepare_memories_for_storage(
        memories: Vec<ExtractedMemory>,
        message: &str,
    ) -> Vec<ExtractedMemory> {
        if memories.is_empty() {
            debug!("No memories extracted, storing original message");
            return vec![ExtractedMemory {
                text: message.to_string(),
                memory_type: "fact".to_string(),
                certainty: 50,
                importance: 50,
                entities: vec![],
                context: None,
            }];
        }

        let mut result = Vec::with_capacity(memories.len());
        for mem in memories {
            if Self::is_coherent_memory(&mem.text) {
                result.push(mem);
            } else {
                warn!(
                    "Splitting incoherent memory: {}...",
                    &mem.text.chars().take(60).collect::<String>()
                );
                let parts = Self::split_incoherent_memory(&mem);
                result.extend(parts);
            }
        }
        result
    }

    fn is_coherent_memory(text: &str) -> bool {
        let contradiction_markers = [
            " but ",
            " however ",
            " although ",
            " whereas ",
            " on the other hand ",
            " in contrast ",
            " conversely ",
            " nevertheless ",
        ];
        let lower = text.to_lowercase();

        let sentence_count = text
            .split(|c: char| c == '.' || c == '!' || c == '?')
            .filter(|s| s.trim().len() > 10)
            .count();

        if sentence_count <= 1 {
            return true;
        }

        let has_contradiction = contradiction_markers.iter().any(|m| lower.contains(m));
        if !has_contradiction {
            return true;
        }

        let distinct_subjects = Self::count_distinct_subjects(&lower);
        if distinct_subjects <= 1 {
            return true;
        }

        false
    }

    fn count_distinct_subjects(text: &str) -> usize {
        let subject_indicators: Vec<&str> = text
            .split(|c: char| c == '.' || c == ';' || c == ',')
            .filter(|s| s.trim().len() > 5)
            .filter_map(|s| {
                let trimmed = s.trim();
                trimmed.split_whitespace().next()
            })
            .collect();

        let mut unique = std::collections::HashSet::new();
        for s in &subject_indicators {
            unique.insert(s.to_lowercase());
        }
        unique.len()
    }

    fn split_incoherent_memory(mem: &ExtractedMemory) -> Vec<ExtractedMemory> {
        let split_patterns = [
            " but ",
            " however ",
            " although ",
            " whereas ",
            " on the other hand ",
        ];
        let lower = mem.text.to_lowercase();

        for pattern in &split_patterns {
            if let Some(pos) = lower.find(pattern) {
                let part1 = mem.text[..pos].trim().to_string();
                let part2 = mem.text[pos + pattern.len()..].trim().to_string();

                if part1.len() > 10 && part2.len() > 10 {
                    return vec![
                        ExtractedMemory {
                            text: part1,
                            memory_type: mem.memory_type.clone(),
                            certainty: mem.certainty,
                            importance: mem.importance,
                            entities: mem.entities.clone(),
                            context: mem.context.clone(),
                        },
                        ExtractedMemory {
                            text: part2,
                            memory_type: mem.memory_type.clone(),
                            certainty: mem.certainty,
                            importance: mem.importance,
                            entities: mem.entities.clone(),
                            context: mem.context.clone(),
                        },
                    ];
                }
            }
        }

        vec![mem.clone()]
    }

    // Reserved for the upcoming personal-only deduplication path (no Hive
    // fan-out). Currently `embed_and_search_global` is used end-to-end.
    #[allow(dead_code)]
    async fn embed_and_search_personal(
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
            .search(text, &vector, user_id, 5, "contextual", None, "personal")
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

        Ok((vector, similar_memories))
    }

    async fn handle_memory_operation(
        &self,
        decision: &MemoryDecision,
        memory: &ExtractedMemory,
        user_id: &str,
        agent_id: Option<&str>,
        tags: &str,
        vector: &[f32],
        phase1_similar: &[SimilarMemory],
        added_ids: &mut Vec<String>,
        updated_ids: &mut Vec<String>,
        skipped: &mut usize,
        chunks_created: &mut usize,
        relations_created: &mut usize,
    ) -> Result<Option<String>, ToolingError> {
        let memory_id = match decision.operation {
            MemoryOperation::Noop => {
                debug!("NOOP: skipping duplicate memory");
                *skipped += 1;
                if let Some(target_id) = &decision.target_memory_id {
                    self.emit_memory_deduplicated(target_id, user_id).await;
                }
                return Ok(None);
            }
            MemoryOperation::Update => {
                if let (Some(target_id), Some(merged)) =
                    (&decision.target_memory_id, &decision.merged_content)
                {
                    if !Self::is_coherent_memory(merged) {
                        warn!(
                            "UPDATE merged_content is incoherent, falling back to ADD: {}...",
                            &merged.chars().take(60).collect::<String>()
                        );
                        let (new_id, new_chunks) =
                            self.store_new_memory(memory, user_id, vector, tags).await?;
                        *chunks_created += new_chunks;
                        let _ = self
                            .add_memory_history_event(&new_id, "ADD", "", &memory.text, user_id)
                            .await;
                        added_ids.push(new_id.clone());
                        new_id
                    } else {
                        debug!("UPDATE: updating {} with merged content", target_id);
                        let old_content = phase1_similar
                            .iter()
                            .find(|m| m.id == *target_id)
                            .map(|m| m.content.as_str())
                            .unwrap_or("");
                        self.update_memory_internal(target_id, merged, vector)
                            .await?;
                        let _ = self
                            .add_memory_history_event(
                                target_id,
                                "UPDATE",
                                old_content,
                                merged,
                                user_id,
                            )
                            .await;
                        updated_ids.push(target_id.to_string());
                        self.emit_memory_updated(target_id, user_id).await;
                        target_id.to_string()
                    }
                } else {
                    let (new_id, new_chunks) =
                        self.store_new_memory(memory, user_id, vector, tags).await?;
                    *chunks_created += new_chunks;
                    let _ = self
                        .add_memory_history_event(&new_id, "ADD", "", &memory.text, user_id)
                        .await;
                    new_id
                }
            }
            MemoryOperation::Supersede => {
                let (new_id, new_chunks) =
                    self.store_new_memory(memory, user_id, vector, tags).await?;
                *chunks_created += new_chunks;
                let _ = self
                    .add_memory_history_event(&new_id, "ADD", "", &memory.text, user_id)
                    .await;
                if let Some(old_id) = &decision.supersedes_memory_id {
                    debug!("SUPERSEDE: {} supersedes {}", new_id, old_id);
                    #[derive(Serialize)]
                    struct SupersedeParams {
                        new_id: String,
                        old_id: String,
                        reason: String,
                        superseded_at: String,
                        is_contradiction: i64,
                    }
                    let _ = self
                        .db
                        .execute_query::<serde_json::Value, _>(
                            "addMemorySupersession",
                            &SupersedeParams {
                                new_id: new_id.clone(),
                                old_id: old_id.clone(),
                                reason: decision.reasoning.clone(),
                                superseded_at: chrono::Utc::now().to_rfc3339(),
                                is_contradiction: 0,
                            },
                        )
                        .await;
                    *relations_created += 1;
                    let _ = self
                        .add_memory_history_event(old_id, "SUPERSEDE", "", &new_id, user_id)
                        .await;
                    self.emit_memory_superseded(&new_id, old_id, user_id).await;
                }
                added_ids.push(new_id.clone());
                new_id
            }
            MemoryOperation::Contradict => {
                let (new_id, new_chunks) =
                    self.store_new_memory(memory, user_id, vector, tags).await?;
                *chunks_created += new_chunks;
                let _ = self
                    .add_memory_history_event(&new_id, "ADD", "", &memory.text, user_id)
                    .await;
                if let Some(contra_id) = &decision.contradicts_memory_id {
                    debug!("CONTRADICT: {} contradicts {}", new_id, contra_id);
                    let _ = self
                        .reasoning_engine
                        .add_relation(&new_id, contra_id, ReasoningType::Contradicts, 80, None)
                        .await;
                }
                added_ids.push(new_id.clone());
                new_id
            }
            MemoryOperation::Delete => {
                if let Some(target_id) = &decision.target_memory_id {
                    debug!("DELETE: removing {} before adding new", target_id);
                    let _ = self
                        .add_memory_history_event(target_id, "DELETE", &memory.text, "", user_id)
                        .await;
                    let _ = self.delete_memory(target_id).await;
                }
                let (new_id, new_chunks) =
                    self.store_new_memory(memory, user_id, vector, tags).await?;
                *chunks_created += new_chunks;
                let _ = self
                    .add_memory_history_event(&new_id, "ADD", "", &memory.text, user_id)
                    .await;
                added_ids.push(new_id.clone());
                new_id
            }
            MemoryOperation::LinkExisting | MemoryOperation::CrossContradict => {
                unreachable!("Phase 1 should not produce cross-user operations");
            }
            MemoryOperation::Add => {
                let (new_id, new_chunks) =
                    self.store_new_memory(memory, user_id, vector, tags).await?;
                *chunks_created += new_chunks;
                let _ = self
                    .add_memory_history_event(&new_id, "ADD", "", &memory.text, user_id)
                    .await;
                self.emit_memory_added(&new_id, user_id, &memory.memory_type)
                    .await;

                let exact_thr = self.config.search_thresholds.exact_duplicate_score;
                if phase1_similar.iter().any(|m| m.score >= exact_thr) {
                    let max_score = phase1_similar
                        .iter()
                        .map(|m| m.score)
                        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                        .unwrap_or(0.0);
                    debug!(
                        "Skipping Phase 2: exact duplicate found in Phase 1 with score {:.3}",
                        max_score
                    );
                } else {
                    self.apply_cross_user_phase(
                        memory,
                        user_id,
                        vector,
                        &new_id,
                        relations_created,
                    )
                    .await?;
                }

                added_ids.push(new_id.clone());
                new_id
            }
        };

        if let Some(agent_id) = agent_id {
            let _ = self
                .link_agent_to_memory(agent_id, &memory_id, "extraction")
                .await;
        }

        Ok(Some(memory_id))
    }

    async fn apply_cross_user_phase(
        &self,
        memory: &ExtractedMemory,
        user_id: &str,
        vector: &[f32],
        new_memory_id: &str,
        _relations_created: &mut usize,
    ) -> Result<(), ToolingError> {
        info!(
            "Phase 2: cross-user dedup for {} (user={})",
            new_memory_id, user_id
        );
        let global_results = self
            .search_engine
            .search_for_dedup(&memory.text, vector, user_id, 5)
            .await
            .unwrap_or_default();

        let cross_user_similar: Vec<SimilarMemory> = global_results
            .iter()
            .filter(|r| {
                let result_user = r
                    .metadata
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                !result_user.is_empty() && result_user != user_id && r.memory_id != new_memory_id
            })
            .map(|r| SimilarMemory {
                id: r.memory_id.clone(),
                content: r.content.clone(),
                score: r.score as f64,
                created_at: Some(r.created_at.clone()),
                user_id: r
                    .metadata
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                is_cross_user: true,
            })
            .collect();

        if cross_user_similar.is_empty() {
            debug!("Phase 2: no cross-user candidates found");
            return Ok(());
        }

        info!(
            "Phase 2: {} cross-user candidates, spawning background LLM decision",
            cross_user_similar.len()
        );

        let memory_text = memory.text.clone();
        let user_id_owned = user_id.to_string();
        let new_mem_id = new_memory_id.to_string();
        let db = self.db.clone();
        let decision_engine = self.decision_engine.clone();

        tokio::spawn(async move {
            let cross_decision = decision_engine
                .decide(&memory_text, &cross_user_similar, &user_id_owned)
                .await;
            info!(
                "Phase 2 bg: LLM decided {:?} (confidence={})",
                cross_decision.operation, cross_decision.confidence
            );

            match cross_decision.operation {
                MemoryOperation::LinkExisting => {
                    if let Some(link_id) = &cross_decision.link_to_memory_id {
                        info!(
                            "Phase 2 bg: LINK_EXISTING user {} → memory {}",
                            user_id_owned, link_id
                        );
                        link_user_to_memory_bg(&db, &user_id_owned, link_id).await;
                    }
                }
                MemoryOperation::CrossContradict => {
                    if let Some(contra_id) = &cross_decision.contradicts_memory_id {
                        info!(
                            "Phase 2 bg: CROSS_CONTRADICT {} ↔ {}",
                            new_mem_id, contra_id
                        );
                        add_contradiction_bg(
                            &db,
                            &new_mem_id,
                            contra_id,
                            cross_decision
                                .conflict_type
                                .as_deref()
                                .unwrap_or("preference"),
                            &cross_decision.reasoning,
                        )
                        .await;
                    }
                }
                MemoryOperation::Noop => {
                    if let Some(existing) = cross_user_similar.first() {
                        info!(
                            "Phase 2 bg: NOOP→link user {} → memory {}",
                            user_id_owned, existing.id
                        );
                        link_user_to_memory_bg(&db, &user_id_owned, &existing.id).await;
                    }
                }
                _ => {
                    debug!("Phase 2 bg: no cross-user action needed");
                }
            }
        });

        Ok(())
    }

    async fn enrich_memory_relations(
        &self,
        memory_id: &str,
        memory: &ExtractedMemory,
        extraction_entities: &[ExtractedEntity],
        similar_memories: &[SimilarMemory],
        decision: &MemoryDecision,
    ) -> Result<(usize, usize), ToolingError> {
        let mut entities_linked = 0usize;
        let mut relations_created = 0usize;

        let should_infer = !similar_memories.is_empty()
            && !matches!(
                decision.operation,
                MemoryOperation::Noop | MemoryOperation::Delete
            );

        info!(
            "enrich_memory_relations: memory={}, similar={}, decision={:?}, should_infer={}",
            safe_truncate(memory_id, 12),
            similar_memories.len(),
            decision.operation,
            should_infer
        );

        if should_infer {
            let context_pairs: Vec<(String, String)> = similar_memories
                .iter()
                .take(5)
                .map(|s| (s.id.clone(), s.content.clone()))
                .collect();

            info!(
                "Calling infer_relations with {} context pairs for {}",
                context_pairs.len(),
                safe_truncate(memory_id, 12)
            );

            match self
                .reasoning_engine
                .infer_relations(memory_id, &memory.text, &context_pairs)
                .await
            {
                Ok(inferred) => {
                    info!("infer_relations returned {} relations", inferred.len());
                    for rel in &inferred {
                        match self
                            .reasoning_engine
                            .add_relation(
                                &rel.from_memory_id,
                                &rel.to_memory_id,
                                rel.relation_type,
                                rel.strength,
                                rel.reasoning_id.as_deref(),
                            )
                            .await
                        {
                            Ok(_) => {
                                relations_created += 1;
                                info!(
                                    "Persisted {} relation: {} -> {} (strength={})",
                                    rel.relation_type.edge_name(),
                                    safe_truncate(memory_id, 12),
                                    safe_truncate(&rel.to_memory_id, 12),
                                    rel.strength
                                );
                            }
                            Err(e) => warn!("Failed to persist inferred relation: {}", e),
                        }
                    }
                }
                Err(e) => warn!("Relation inference failed: {}", e),
            }
        }

        for entity_id in &memory.entities {
            if let Some(entity) = extraction_entities.iter().find(|e| &e.id == entity_id) {
                match self
                    .entity_manager
                    .get_or_create_entity(&entity.name, &entity.entity_type, None)
                    .await
                {
                    Ok(db_entity) => {
                        if let Err(e) = self
                            .entity_manager
                            .link_to_memory(
                                &db_entity.entity_id,
                                memory_id,
                                EntityEdgeType::ExtractedEntity,
                                80,
                                50,
                                "neutral",
                            )
                            .await
                        {
                            warn!(
                                "Failed to link entity {} to memory {}: {}",
                                db_entity.entity_id, memory_id, e
                            );
                        } else {
                            entities_linked += 1;
                            debug!("Linked entity '{}' to memory {}", entity.name, memory_id);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to get/create entity '{}': {}", entity.name, e);
                    }
                }
            }
        }

        for entity in extraction_entities {
            if let Some(ref rels) = entity.relations {
                for rel in rels {
                    self.persist_entity_relation(
                        &entity.id,
                        &rel.target_entity,
                        &rel.relationship_type,
                        rel.strength,
                        extraction_entities,
                    )
                    .await;
                }
            }
        }

        let concept_links: Vec<(String, String, i32)> = {
            let ontology = self.ontology_manager.read();
            ontology
                .map_memory_to_concepts(&memory.text, Some(&memory.memory_type))
                .into_iter()
                .map(|m| {
                    (
                        m.concept.id.clone(),
                        m.concept.name.clone(),
                        (m.confidence * 100.0) as i32,
                    )
                })
                .collect()
        };

        for (concept_id, concept_name, confidence) in concept_links {
            if let Err(e) = self
                .link_memory_to_concept(memory_id, &concept_id, confidence)
                .await
            {
                warn!("Failed to link concept {}: {}", concept_id, e);
            } else {
                debug!("Linked memory {} to concept '{}'", memory_id, concept_name);
            }
        }

        Ok((entities_linked, relations_created))
    }

    async fn resolve_and_persist_extraction_relations(
        &self,
        extraction_relations: &[ExtractedRelation],
        memories_to_store: &[ExtractedMemory],
        stored_memory_ids: &HashMap<usize, String>,
    ) -> Result<usize, ToolingError> {
        let mut relations_created = 0usize;

        let memory_index_to_id: Vec<Option<String>> = (0..memories_to_store.len())
            .map(|i| stored_memory_ids.get(&i).cloned())
            .collect();

        let mut memory_content_to_id: HashMap<String, String> = HashMap::new();
        for (i, mem) in memories_to_store.iter().enumerate() {
            if let Some(id) = stored_memory_ids.get(&i) {
                let normalized = mem.text.to_lowercase();
                memory_content_to_id.insert(normalized, id.clone());
            }
        }

        for relation in extraction_relations {
            let from_id = relation
                .from_memory_index
                .and_then(|idx| memory_index_to_id.get(idx).and_then(|o| o.as_ref()))
                .or_else(|| {
                    if !relation.from_memory_content.is_empty() {
                        memory_content_to_id
                            .get(&relation.from_memory_content.to_lowercase())
                            .or_else(|| {
                                memory_content_to_id
                                    .iter()
                                    .find(|(k, _)| {
                                        k.contains(&relation.from_memory_content.to_lowercase())
                                            || relation
                                                .from_memory_content
                                                .to_lowercase()
                                                .contains(k.as_str())
                                    })
                                    .map(|(_, v)| v)
                            })
                    } else {
                        None
                    }
                });

            let to_id = relation
                .to_memory_index
                .and_then(|idx| memory_index_to_id.get(idx).and_then(|o| o.as_ref()))
                .or_else(|| {
                    if !relation.to_memory_content.is_empty() {
                        memory_content_to_id
                            .get(&relation.to_memory_content.to_lowercase())
                            .or_else(|| {
                                memory_content_to_id
                                    .iter()
                                    .find(|(k, _)| {
                                        k.contains(&relation.to_memory_content.to_lowercase())
                                            || relation
                                                .to_memory_content
                                                .to_lowercase()
                                                .contains(k.as_str())
                                    })
                                    .map(|(_, v)| v)
                            })
                    } else {
                        None
                    }
                });

            if let (Some(from), Some(to)) = (from_id, to_id) {
                let rel_type = match relation.relation_type.to_uppercase().as_str() {
                    "IMPLIES" => ReasoningType::Implies,
                    "BECAUSE" => ReasoningType::Because,
                    "CONTRADICTS" => ReasoningType::Contradicts,
                    "SUPPORTS" => ReasoningType::Supports,
                    _ => ReasoningType::Implies,
                };

                match self
                    .reasoning_engine
                    .add_relation(from, to, rel_type, relation.strength, None)
                    .await
                {
                    Ok(rel) => {
                        relations_created += 1;
                        info!(
                            "Created {} relation: {} -> {}",
                            rel.relation_type.edge_name(),
                            from,
                            to
                        );
                    }
                    Err(e) => {
                        warn!("Failed to create relation: {}", e);
                    }
                }
            } else {
                debug!(
                    "Could not resolve memory IDs for relation (from_idx={:?}, to_idx={:?})",
                    relation.from_memory_index, relation.to_memory_index
                );
            }
        }

        Ok(relations_created)
    }

    pub(crate) async fn store_new_memory(
        &self,
        memory: &ExtractedMemory,
        user_id: &str,
        vector: &[f32],
        context_tags: &str,
    ) -> Result<(String, usize), ToolingError> {
        // Memory.user_id must always match the owning user: personal search (e.g. SmartTraversalV2)
        // filters on this field; empty values break isolation until backfilled.
        if user_id.trim().is_empty() {
            return Err(ToolingError::Memory(
                "user_id must be non-empty when creating a Memory node".to_string(),
            ));
        }

        let memory_id = format!(
            "mem_{}",
            uuid::Uuid::new_v4()
                .to_string()
                .replace("-", "")
                .chars()
                .take(12)
                .collect::<String>()
        );
        let now = chrono::Utc::now().to_rfc3339();

        #[derive(Serialize)]
        struct AddMemoryInput {
            memory_id: String,
            user_id: String,
            content: String,
            memory_type: String,
            certainty: i64,
            importance: i64,
            created_at: String,
            updated_at: String,
            context_tags: String,
            source: String,
            metadata: String,
        }

        let input = AddMemoryInput {
            memory_id: memory_id.clone(),
            // Same string as linkUserToMemory — required for vector-hit user filtering.
            user_id: user_id.to_string(),
            content: memory.text.clone(),
            memory_type: memory.memory_type.clone(),
            certainty: memory.certainty as i64,
            importance: memory.importance as i64,
            created_at: now.clone(),
            updated_at: now.clone(),
            context_tags: context_tags.to_string(),
            source: "llm_extraction".to_string(),
            metadata: "{}".to_string(),
        };

        #[derive(serde::Deserialize)]
        struct AddMemoryResponse {
            memory: MemoryNode,
        }
        #[derive(serde::Deserialize)]
        struct MemoryNode {
            id: String,
        }

        let response: AddMemoryResponse = self
            .db
            .execute_query("addMemory", &input)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        let internal_id = response.memory.id;
        debug!("Memory created: {} (internal: {})", memory_id, internal_id);

        #[derive(Serialize)]
        struct AddEmbeddingInput {
            memory_id: String,
            vector_data: Vec<f64>,
            embedding_model: String,
            created_at: String,
        }

        let embed_input = AddEmbeddingInput {
            memory_id: internal_id,
            vector_data: vector.iter().map(|&x| x as f64).collect(),
            embedding_model: self.embedder.model().to_string(),
            created_at: now.clone(),
        };

        if let Err(e) = self
            .db
            .execute_query::<serde_json::Value, _>("addMemoryEmbedding", &embed_input)
            .await
        {
            warn!("Failed to add embedding for {}: {}", memory_id, e);
        } else {
            debug!("Embedding added for {}", memory_id);
        }

        self.ensure_user_exists(user_id).await;

        #[derive(Serialize)]
        struct LinkUserInput {
            user_id: String,
            memory_id: String,
            context: String,
        }

        if let Err(e) = self
            .db
            .execute_query::<serde_json::Value, _>(
                "linkUserToMemory",
                &LinkUserInput {
                    user_id: user_id.to_string(),
                    memory_id: memory_id.clone(),
                    context: "created".to_string(),
                },
            )
            .await
        {
            warn!(
                "Failed to link user {} to memory {}: {}",
                user_id, memory_id, e
            );
        }

        let mut chunk_count = 0usize;
        if self.chunking_manager.should_chunk(&memory.text) {
            info!(
                "Content exceeds threshold ({} chars), creating chunks",
                memory.text.chars().count()
            );
            match self
                .chunking_manager
                .add_memory_with_chunking(
                    &memory_id,
                    &memory.text,
                    user_id,
                    &memory.memory_type,
                    memory.certainty as i64,
                    memory.importance as i64,
                    "llm_extraction",
                    "",
                    "{}",
                )
                .await
            {
                Ok(result) => {
                    chunk_count = result.chunk_count;
                    info!("Created {} chunks for {}", chunk_count, memory_id);
                }
                Err(e) => {
                    warn!("Failed to chunk memory {}: {}", memory_id, e);
                }
            }
        }

        if let Some(ref context_tag) = memory.context {
            if let Err(e) = self
                .link_memory_to_extracted_context(&memory_id, context_tag)
                .await
            {
                warn!(
                    "Failed to link memory {} to context '{}': {}",
                    memory_id, context_tag, e
                );
            }
        }

        debug!("Stored new memory: {}", memory_id);
        Ok((memory_id, chunk_count))
    }

    async fn store_raw_source(
        &self,
        memory: &ExtractedMemory,
        user_id: &str,
        vector: &[f32],
        context_tags: &str,
    ) -> Result<String, ToolingError> {
        let memory_id = format!(
            "raw_{}",
            uuid::Uuid::new_v4()
                .to_string()
                .replace("-", "")
                .chars()
                .take(12)
                .collect::<String>()
        );
        let now = chrono::Utc::now().to_rfc3339();

        #[derive(Serialize)]
        struct Input {
            memory_id: String,
            user_id: String,
            content: String,
            memory_type: String,
            certainty: i64,
            importance: i64,
            created_at: String,
            updated_at: String,
            context_tags: String,
            source: String,
            metadata: String,
        }

        let input = Input {
            memory_id: memory_id.clone(),
            user_id: user_id.to_string(),
            content: memory.text.clone(),
            memory_type: memory.memory_type.clone(),
            certainty: memory.certainty as i64,
            importance: memory.importance as i64,
            created_at: now.clone(),
            updated_at: now.clone(),
            context_tags: context_tags.to_string(),
            source: "raw_input".to_string(),
            metadata: "{}".to_string(),
        };

        #[derive(serde::Deserialize)]
        struct Resp {
            memory: Node,
        }
        #[derive(serde::Deserialize)]
        struct Node {
            id: String,
        }

        let resp: Resp = self
            .db
            .execute_query("addMemory", &input)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        #[derive(Serialize)]
        struct EmbedInput {
            memory_id: String,
            vector_data: Vec<f64>,
            embedding_model: String,
            created_at: String,
        }

        let _ = self
            .db
            .execute_query::<serde_json::Value, _>(
                "addMemoryEmbedding",
                &EmbedInput {
                    memory_id: resp.memory.id,
                    vector_data: vector.iter().map(|&x| x as f64).collect(),
                    embedding_model: self.embedder.model().to_string(),
                    created_at: now,
                },
            )
            .await;

        self.ensure_user_exists(user_id).await;
        let _ = self
            .db
            .execute_query::<serde_json::Value, _>(
                "linkUserToMemory",
                &serde_json::json!({
                    "user_id": user_id,
                    "memory_id": memory_id,
                    "context": "raw_source",
                }),
            )
            .await;

        Ok(memory_id)
    }

    async fn persist_entity_relation(
        &self,
        from_entity_id: &str,
        target_entity_id: &str,
        relationship_type: &str,
        strength: i64,
        extraction_entities: &[ExtractedEntity],
    ) {
        let from_entity = extraction_entities.iter().find(|e| e.id == from_entity_id);
        let to_entity = extraction_entities
            .iter()
            .find(|e| e.id == target_entity_id);

        let (from_name, from_type) = match from_entity {
            Some(e) => (e.name.as_str(), e.entity_type.as_str()),
            None => return,
        };
        let (to_name, to_type) = match to_entity {
            Some(e) => (e.name.as_str(), e.entity_type.as_str()),
            None => return,
        };

        let from_db = match self
            .entity_manager
            .get_or_create_entity(from_name, from_type, None)
            .await
        {
            Ok(e) => e,
            Err(e) => {
                warn!(
                    "Failed to resolve entity '{}' for relation: {}",
                    from_name, e
                );
                return;
            }
        };
        let to_db = match self
            .entity_manager
            .get_or_create_entity(to_name, to_type, None)
            .await
        {
            Ok(e) => e,
            Err(e) => {
                warn!("Failed to resolve entity '{}' for relation: {}", to_name, e);
                return;
            }
        };

        #[derive(Serialize)]
        struct EntityRelationParams {
            from_id: String,
            to_id: String,
            relationship_type: String,
            strength: i64,
            bidirectional: i64,
        }

        let params = EntityRelationParams {
            from_id: from_db.entity_id.clone(),
            to_id: to_db.entity_id.clone(),
            relationship_type: relationship_type.to_string(),
            strength,
            bidirectional: 0,
        };

        match self
            .db
            .execute_query::<serde_json::Value, _>("addEntityRelation", &params)
            .await
        {
            Ok(_) => {
                info!(
                    "Created entity relation: {} -[{}]-> {}",
                    from_name, relationship_type, to_name
                );
            }
            Err(e) => {
                warn!(
                    "Failed to create entity relation {} -> {}: {}",
                    from_name, to_name, e
                );
            }
        }
    }

    async fn link_memory_to_extracted_context(
        &self,
        memory_id: &str,
        context_tag: &str,
    ) -> Result<(), ToolingError> {
        let context_name = context_tag.trim();
        if context_name.is_empty() {
            return Ok(());
        }

        let context_type = if context_name.contains(':') {
            context_name
                .split(':')
                .next()
                .unwrap_or("general")
                .to_string()
        } else {
            "general".to_string()
        };

        let context_id = {
            #[derive(Serialize)]
            struct GetByNameParams {
                name: String,
            }

            let existing: Option<serde_json::Value> = self
                .db
                .execute_query(
                    "getContextByName",
                    &GetByNameParams {
                        name: context_name.to_string(),
                    },
                )
                .await
                .ok();

            if let Some(ref val) = existing {
                val.get("context_id")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            } else {
                None
            }
        };

        let resolved_id = match context_id {
            Some(id) => id,
            None => {
                let new_id = format!(
                    "ctx_{}",
                    uuid::Uuid::new_v4()
                        .to_string()
                        .replace("-", "")
                        .chars()
                        .take(12)
                        .collect::<String>()
                );

                #[derive(Serialize)]
                struct AddContextParams {
                    context_id: String,
                    name: String,
                    context_type: String,
                    properties: String,
                    parent_context: String,
                }

                let _ = self
                    .db
                    .execute_query::<serde_json::Value, _>(
                        "addContext",
                        &AddContextParams {
                            context_id: new_id.clone(),
                            name: context_name.to_string(),
                            context_type,
                            properties: "{}".to_string(),
                            parent_context: "".to_string(),
                        },
                    )
                    .await;

                debug!("Created new context '{}' ({})", context_name, new_id);
                new_id
            }
        };

        #[derive(Serialize)]
        struct ValidInParams {
            memory_id: String,
            context_id: String,
            priority: i64,
            exclusive: i64,
        }

        self.db
            .execute_query::<serde_json::Value, _>(
                "addMemoryValidIn",
                &ValidInParams {
                    memory_id: memory_id.to_string(),
                    context_id: resolved_id.clone(),
                    priority: 50,
                    exclusive: 0,
                },
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        debug!("Linked memory {} to context '{}'", memory_id, context_name);
        Ok(())
    }
}

async fn link_user_to_memory_bg(db: &crate::db::HelixClient, user_id: &str, memory_id: &str) {
    #[derive(Serialize)]
    struct EnsureUser {
        user_id: String,
        name: String,
    }
    let _ = db
        .execute_query::<serde_json::Value, _>("getUser", &serde_json::json!({"user_id": user_id}))
        .await
        .or_else(|_| {
            futures::executor::block_on(async {
                db.execute_query::<serde_json::Value, _>(
                    "addUser",
                    &EnsureUser {
                        user_id: user_id.to_string(),
                        name: user_id.to_string(),
                    },
                )
                .await
            })
        });

    #[derive(Serialize)]
    struct LinkInput {
        user_id: String,
        memory_id: String,
        context: String,
    }
    if let Err(e) = db
        .execute_query::<serde_json::Value, _>(
            "linkUserToMemory",
            &LinkInput {
                user_id: user_id.to_string(),
                memory_id: memory_id.to_string(),
                context: "cross_user_link".to_string(),
            },
        )
        .await
    {
        warn!(
            "Phase 2 bg: failed to link user {} to memory {}: {}",
            user_id, memory_id, e
        );
        return;
    }

    #[derive(serde::Deserialize)]
    struct UsersResult {
        #[serde(default)]
        users: Vec<serde_json::Value>,
    }
    let user_count = match db
        .execute_query::<UsersResult, _>(
            "getMemoryUsers",
            &serde_json::json!({"memory_id": memory_id}),
        )
        .await
    {
        Ok(r) => r.users.len().max(1) as i64,
        Err(_) => 2,
    };

    #[derive(Serialize)]
    struct UpdateCount {
        memory_id: String,
        user_count: i64,
        updated_at: String,
    }
    let _ = db
        .execute_query::<serde_json::Value, _>(
            "updateMemoryUserCount",
            &UpdateCount {
                memory_id: memory_id.to_string(),
                user_count,
                updated_at: chrono::Utc::now().to_rfc3339(),
            },
        )
        .await;

    info!(
        "Phase 2 bg: linked user {} to memory {} (user_count={})",
        user_id, memory_id, user_count
    );
}

async fn add_contradiction_bg(
    db: &crate::db::HelixClient,
    from_id: &str,
    to_id: &str,
    conflict_type: &str,
    reasoning: &str,
) {
    #[derive(Serialize)]
    struct ContradictInput {
        from_id: String,
        to_id: String,
        resolution: String,
        resolved: i64,
        resolution_strategy: String,
    }
    if let Err(e) = db
        .execute_query::<serde_json::Value, _>(
            "addMemoryContradiction",
            &ContradictInput {
                from_id: from_id.to_string(),
                to_id: to_id.to_string(),
                resolution: reasoning.to_string(),
                resolved: 0,
                resolution_strategy: format!("cross_user_{}", conflict_type),
            },
        )
        .await
    {
        warn!(
            "Phase 2 bg: failed to add contradiction {} → {}: {}",
            from_id, to_id, e
        );
    } else {
        info!(
            "Phase 2 bg: added cross-user contradiction {} → {}",
            from_id, to_id
        );
    }
}
