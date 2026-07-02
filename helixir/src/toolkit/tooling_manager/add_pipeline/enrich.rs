//! Post-store enrichment: LLM-inferred reasoning relations + entity-linking
//! + ontology concept linking + downstream entity↔entity edges.
//!
//! Also resolves explicit `ExtractedRelation`s emitted by the LLM into
//! stored reasoning edges.

use std::collections::HashMap;

use tracing::{debug, info, warn};

use crate::llm::extractor::{ExtractedEntity, ExtractedMemory, ExtractedRelation};
use crate::toolkit::mind_toolbox::entity::EntityEdgeType;
use crate::toolkit::mind_toolbox::reasoning::ReasoningType;

use super::super::ToolingManager;
use super::super::types::ToolingError;
use crate::safe_truncate;

impl ToolingManager {
    /// One relation-inference LLM call for a freshly stored memory, persisting
    /// whatever it finds. Separated from the store loop so the orchestrator can
    /// run these independent calls CONCURRENTLY — sequential per-atom inference
    /// used to stack K× model latency onto every multi-atom write.
    pub(super) async fn infer_and_persist_relations(
        &self,
        memory_id: &str,
        memory_text: &str,
        context_pairs: &[(String, String)],
    ) -> usize {
        let mut relations_created = 0usize;

        info!(
            "Calling infer_relations with {} context pairs for {}",
            context_pairs.len(),
            safe_truncate(memory_id, 12)
        );

        match self
            .reasoning_engine
            .infer_relations(memory_id, memory_text, context_pairs)
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

        relations_created
    }

    /// Entity linking + entity↔entity edges + ontology concept mapping for one
    /// stored memory. Pure DB work — no LLM. (The inference half of the old
    /// `enrich_memory_relations` lives in `infer_and_persist_relations`.)
    pub(super) async fn link_memory_semantics(
        &self,
        memory_id: &str,
        memory: &ExtractedMemory,
        extraction_entities: &[ExtractedEntity],
    ) -> Result<usize, ToolingError> {
        let mut entities_linked = 0usize;

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
                                self.config.write.entity_link_strength as i32,
                                self.config.write.entity_link_confidence as i32,
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

        Ok(entities_linked)
    }

    /// Deferred entity enrichment for memories stored WITHOUT extraction
    /// (FastThink fast commit): one extraction call for the entities only,
    /// linked to the already-stored memories. Runs in a background task —
    /// off the caller's critical path by design.
    pub(crate) async fn extract_and_link_entities(
        &self,
        text: &str,
        user_id: &str,
        memory_ids: &[String],
    ) -> usize {
        let extraction = match self.extractor.extract(text, user_id, true, false).await {
            Ok(e) => e,
            Err(e) => {
                warn!("Deferred entity enrichment: extraction failed: {e}");
                return 0;
            }
        };

        let mut linked = 0usize;
        for entity in &extraction.entities {
            match self
                .entity_manager
                .get_or_create_entity(&entity.name, &entity.entity_type, None)
                .await
            {
                Ok(db_entity) => {
                    for memory_id in memory_ids {
                        match self
                            .entity_manager
                            .link_to_memory(
                                &db_entity.entity_id,
                                memory_id,
                                EntityEdgeType::ExtractedEntity,
                                self.config.write.entity_link_strength as i32,
                                self.config.write.entity_link_confidence as i32,
                                "neutral",
                            )
                            .await
                        {
                            Ok(()) => linked += 1,
                            Err(e) => warn!(
                                "Deferred entity enrichment: link {} -> {} failed: {e}",
                                db_entity.entity_id, memory_id
                            ),
                        }
                    }
                }
                Err(e) => warn!(
                    "Deferred entity enrichment: get/create '{}' failed: {e}",
                    entity.name
                ),
            }
        }
        info!(
            "Deferred entity enrichment: {} links across {} memories",
            linked,
            memory_ids.len()
        );
        linked
    }

    pub(super) async fn resolve_and_persist_extraction_relations(
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
                // Full arsenal incl. associative edges (RELATES_TO/PART_OF/IS_A).
                // Unknown tokens fall back to RELATES_TO, never a false IMPLIES.
                let rel_type = ReasoningType::from_token(&relation.relation_type);

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
}
