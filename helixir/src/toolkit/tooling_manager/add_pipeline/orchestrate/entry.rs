//! Public add-memory entry points and extraction setup.

use super::*;

impl ToolingManager {
    pub async fn add_memory(
        &self,
        message: &str,
        user_id: &str,
        agent_id: Option<&str>,
        _metadata: Option<HashMap<String, serde_json::Value>>,
        context_tags: Option<&str>,
    ) -> Result<AddMemoryResult, ToolingError> {
        self.add_memory_scoped(
            message,
            user_id,
            agent_id,
            _metadata,
            context_tags,
            &RbacMemoryScope::Legacy,
            user_id,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn add_memory_scoped(
        &self,
        message: &str,
        user_id: &str,
        agent_id: Option<&str>,
        _metadata: Option<HashMap<String, serde_json::Value>>,
        context_tags: Option<&str>,
        scope: &RbacMemoryScope,
        assigned_by: &str,
    ) -> Result<AddMemoryResult, ToolingError> {
        let preview: String = message.chars().take(50).collect();
        let tags = context_tags.unwrap_or("");
        info!(
            "Adding memory for user={}: {}... [tags={}]",
            user_id, preview, tags
        );

        debug!("Step 1: LLM extraction");
        // #34 2b: an adopted charter rule is a VERBATIM artifact, not a story
        // to atomize — extraction would rephrase it and the rule tag (stamped
        // by prefix in decide.rs) would never land. Deterministic single-atom
        // path, no LLM.
        let extraction = if message.trim_start().starts_with("Charter rule [") {
            info!("charter-rule write: verbatim single-atom path (no extraction)");
            crate::llm::extractor::ExtractionResult {
                memories: vec![crate::llm::extractor::ExtractedMemory {
                    text: message.trim().to_string(),
                    memory_type: "fact".to_string(),
                    certainty: 95,
                    importance: 70,
                    entities: vec![],
                    context: None,
                }],
                entities: vec![],
                relations: vec![],
            }
        } else {
            self.extractor
                .extract(message, user_id, true, true)
                .await
                .map_err(|e| ToolingError::Extraction(e.to_string()))?
        };

        info!(
            "Extracted {} memories, {} entities, {} relations",
            extraction.memories.len(),
            extraction.entities.len(),
            extraction.relations.len()
        );

        // #79: example-leak firewall — drop atoms that resemble a prompt's
        // worked example while being ungrounded in the user's actual message
        // (the signature of a fabricated memory; observed live from a weak
        // model). Relation indices are remapped over the survivors.
        let mut index_map: Vec<Option<usize>> = Vec::with_capacity(extraction.memories.len());
        let mut kept = Vec::with_capacity(extraction.memories.len());
        for m in extraction.memories {
            if crate::llm::example_guard::is_example_leak(&m.text, message) {
                warn!(
                    "example-leak atom dropped (ungrounded copy of a worked example): '{}'",
                    crate::safe_truncate(&m.text, 80)
                );
                index_map.push(None);
            } else {
                index_map.push(Some(kept.len()));
                kept.push(m);
            }
        }
        let relations: Vec<crate::llm::extractor::ExtractedRelation> = extraction
            .relations
            .into_iter()
            .filter_map(|mut r| {
                let from = *index_map.get(r.from_memory_index?)?;
                let to = *index_map.get(r.to_memory_index?)?;
                match (from, to) {
                    (Some(f), Some(t)) => {
                        r.from_memory_index = Some(f);
                        r.to_memory_index = Some(t);
                        Some(r)
                    }
                    _ => None,
                }
            })
            .collect();

        let memories_to_store = self.prepare_memories_for_storage(kept, message);
        self.run_add_pipeline(
            memories_to_store,
            &extraction.entities,
            &relations,
            Some(message),
            user_id,
            agent_id,
            tags,
            scope,
            assigned_by,
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
        self.add_prepared_memories_scoped(
            memories,
            user_id,
            agent_id,
            context_tags,
            &RbacMemoryScope::Legacy,
            user_id,
        )
        .await
    }

    pub async fn add_prepared_memories_scoped(
        &self,
        memories: Vec<ExtractedMemory>,
        user_id: &str,
        agent_id: Option<&str>,
        context_tags: Option<&str>,
        scope: &RbacMemoryScope,
        assigned_by: &str,
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
            scope,
            assigned_by,
        )
        .await
    }
}
