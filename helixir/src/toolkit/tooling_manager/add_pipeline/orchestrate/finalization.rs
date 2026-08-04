//! Relation inference and raw-source finalization.

use super::*;

impl ToolingManager {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn finalize_relations_and_raw(
        &self,
        infer_jobs: Vec<RelationInferenceJob>,
        extracted_relations: &[ExtractedRelation],
        memories_to_store: &[ExtractedMemory],
        stored_memory_ids: &HashMap<usize, String>,
        raw_message: Option<&str>,
        user_id: &str,
        tags: &str,
        scope: &RbacMemoryScope,
        assigned_by: &str,
        added_ids: &[String],
        mut relations_created: usize,
        mut chunks_created: usize,
    ) -> Result<(usize, usize), ToolingError> {
        // Phase D: relation inference — #96 Lever 1 batches ALL new atoms into
        // ONE LLM call (was one independent call per atom, run concurrently).
        // O(1) model calls per write instead of O(N); the edges are identical.
        if !infer_jobs.is_empty() {
            // #96 Lever 2: the local NLI judge takes the SUPPORTS/CONTRADICTS
            // pairs (sync CPU — block_in_place keeps the runtime honest);
            // only the residual pairs, where implicit causality may hide,
            // still pay the LLM. Often the residual is empty.
            let enabled = self.config.write.nli_route;
            let min_prob = self.config.write.nli_route_min_prob;
            let jobs_backup = infer_jobs.clone();
            let (routed, residual) = match tokio::task::spawn_blocking(move || {
                super::super::nli_route::route_jobs(infer_jobs, enabled, min_prob)
            })
            .await
            {
                Ok(split) => split,
                Err(e) => {
                    warn!("NLI routing task failed ({e}); all pairs go to the LLM");
                    (Vec::new(), jobs_backup)
                }
            };
            if !routed.is_empty() {
                info!(
                    "NLI routed {} relation(s) off the LLM ({} residual job(s))",
                    routed.len(),
                    residual.len()
                );
                relations_created += self.persist_inferred_relations(&routed).await;
            }
            if !residual.is_empty() {
                let inferred = self
                    .reasoning_engine
                    .infer_relations_batch(&residual)
                    .await
                    .unwrap_or_default();
                relations_created += self.persist_inferred_relations(&inferred).await;
            }
        }

        relations_created += self
            .resolve_and_persist_extraction_relations(
                extracted_relations,
                memories_to_store,
                stored_memory_ids,
            )
            .await?;

        // Deterministic causal floor (#66): an explicit connective in the RAW
        // message with >=2 stored atoms and ZERO relations from the whole
        // pipeline gets a BECAUSE edge wired by clause alignment — "reasons
        // in chains" must not depend on the model's mood (or its fallback
        // tier). The LLM path stays first; this fires only when it gave nothing.
        if relations_created == 0
            && stored_memory_ids.len() >= 2
            && let Some(message) = raw_message
            && let Some((cause_text, effect_text)) =
                super::super::connective_backstop::split_causal(message)
        {
            let mut idx: Vec<usize> = stored_memory_ids.keys().copied().collect();
            idx.sort_unstable();
            let atom_texts: Vec<&str> = idx
                .iter()
                .map(|i| memories_to_store[*i].text.as_str())
                .collect();
            if let Some((c, e)) = super::super::connective_backstop::pick_cause_effect(
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

        // #66: structural-connective backstop — "X is part of Y" / "X is a
        // kind of Y" states the edge in plain words, so it must exist
        // regardless of the model's mood. Fires whenever the connective is
        // present and two atoms align; add_relation's duplicate guard makes
        // it a no-op when the LLM already built the edge.
        if stored_memory_ids.len() >= 2
            && let Some(message) = raw_message
            && let Some((edge_type, from_text, to_text)) =
                super::super::connective_backstop::split_structural(message)
        {
            let mut idx: Vec<usize> = stored_memory_ids.keys().copied().collect();
            idx.sort_unstable();
            let atom_texts: Vec<&str> = idx
                .iter()
                .map(|i| memories_to_store[*i].text.as_str())
                .collect();
            if let Some((f, t)) = super::super::connective_backstop::pick_cause_effect(
                &atom_texts,
                &from_text,
                &to_text,
            ) {
                let from = &stored_memory_ids[&idx[f]];
                let to = &stored_memory_ids[&idx[t]];
                match self
                    .reasoning_engine
                    .add_relation(from, to, edge_type, 60, None)
                    .await
                {
                    Ok(_) => {
                        relations_created += 1;
                        info!(
                            "structural backstop: {} {} -> {} (explicit connective in the input)",
                            edge_type.edge_name(),
                            safe_truncate(from, 12),
                            safe_truncate(to, 12)
                        );
                    }
                    Err(e) => debug!("structural backstop skipped: {e}"),
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
                        .store_raw_source(
                            &raw_mem,
                            user_id,
                            &raw_vec,
                            tags,
                            scope.fingerprint_scope().as_deref(),
                        )
                        .await
                    {
                        Ok(raw_id) => {
                            RbacManager::new(self.db.clone())
                                .link_memory_to_scope(&raw_id, scope, assigned_by)
                                .await
                                .map_err(|error| ToolingError::Database(error.to_string()))?;
                            debug!("Raw source stored: {}", raw_id);
                            chunks_created += 1;
                            // #82: family link — every atom points at the raw
                            // it was extracted from, so search can collapse a
                            // raw and its atoms into one result instead of
                            // billing the same content twice.
                            for atom_id in added_ids {
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

        Ok((relations_created, chunks_created))
    }
}
