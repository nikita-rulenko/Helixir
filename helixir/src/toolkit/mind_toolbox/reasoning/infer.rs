//! LLM-driven relation inference: [`ReasoningEngine::infer_relations`].
//!
//! Given a new memory and a slice of `(id, content)` candidates discovered
//! by similarity search, ask the LLM which of them carry a logical
//! relationship and of what type. The contract: no LLM provider → no
//! relations (silent), empty candidates → no relations (silent), any
//! parse failure → one retry then empty.

use tracing::{debug, info, warn};

use super::engine::ReasoningEngine;
use super::types::{ReasoningError, ReasoningRelation, ReasoningType};

impl ReasoningEngine {
    pub async fn infer_relations(
        &self,
        new_memory_id: &str,
        new_memory_content: &str,
        similar_memories: &[(String, String)],
    ) -> Result<Vec<ReasoningRelation>, ReasoningError> {
        let Some(ref llm) = self.llm_provider else {
            return Ok(Vec::new());
        };

        if similar_memories.is_empty() {
            return Ok(Vec::new());
        }

        let system_prompt = r#"You are a reasoning engine that finds logical connections between memories. You MUST find at least one relationship if the memories share ANY topic, entity, or context.

Output a JSON array. Each element:
{"existing_index": 0, "type": "IMPLIES|BECAUSE|CONTRADICTS|SUPPORTS", "strength": 0-100}

Relation types:
- SUPPORTS: they share the same topic, reinforce each other, or provide evidence for the same conclusion (MOST COMMON — use when in doubt)
- IMPLIES: one logically leads to or suggests the other
- BECAUSE: one is a cause/reason for the other
- CONTRADICTS: they conflict or are incompatible

Rules:
- If both memories mention the same project, person, technology, or concept → at minimum SUPPORTS (strength 50-70)
- If one memory is a consequence of another → IMPLIES (strength 60-90)
- If one memory explains why another is true → BECAUSE (strength 60-90)
- Include relations with strength >= 40
- Output ONLY a valid JSON array, no markdown, no explanation
- If truly no connection exists (completely unrelated topics), output []"#;

        let context_str: String = similar_memories
            .iter()
            .enumerate()
            .map(|(i, (_, content))| format!("[{}] {}", i, content))
            .collect::<Vec<_>>()
            .join("\n");

        let user_prompt = format!(
            "NEW memory: {}\n\nEXISTING memories:\n{}",
            new_memory_content, context_str
        );

        let parse_relations = |response: &str| -> Vec<ReasoningRelation> {
            let parsed = serde_json::from_str::<Vec<serde_json::Value>>(response).or_else(|_| {
                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(response) {
                    if let Some(arr) = obj
                        .get("relations")
                        .or_else(|| obj.get("results"))
                        .or_else(|| obj.get("data"))
                    {
                        if let Some(arr) = arr.as_array() {
                            return Ok(arr.clone());
                        }
                    }
                }
                if let Some(start) = response.find('[') {
                    if let Some(end) = response.rfind(']') {
                        return serde_json::from_str(&response[start..=end]);
                    }
                }
                Err(serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "no array",
                )))
            });

            match parsed {
                Ok(inferred) => inferred
                    .iter()
                    .filter_map(|r| {
                        let idx = r.get("existing_index")?.as_u64()? as usize;
                        let (target_id, target_content) = similar_memories.get(idx)?;
                        Some(ReasoningRelation {
                            relation_id: format!(
                                "inferred_{}_{}",
                                crate::safe_truncate(new_memory_id, 8),
                                crate::safe_truncate(target_id, 8)
                            ),
                            from_memory_id: new_memory_id.to_string(),
                            to_memory_id: target_id.clone(),
                            to_memory_content: target_content.clone(),
                            from_memory_content: new_memory_content.to_string(),
                            relation_type: match r.get("type")?.as_str()? {
                                "IMPLIES" => ReasoningType::Implies,
                                "BECAUSE" => ReasoningType::Because,
                                "CONTRADICTS" => ReasoningType::Contradicts,
                                _ => ReasoningType::Supports,
                            },
                            strength: r.get("strength")?.as_i64()? as i32,
                            reasoning_id: Some("llm_inferred".to_string()),
                        })
                    })
                    .collect(),
                Err(_) => Vec::new(),
            }
        };

        match llm
            .generate(system_prompt, &user_prompt, Some("json_object"))
            .await
        {
            Ok((response, _metadata)) => {
                info!(
                    "infer_relations LLM response ({}b): {}",
                    response.len(),
                    &response.chars().take(200).collect::<String>()
                );
                let relations = parse_relations(&response);
                if !relations.is_empty() {
                    info!(
                        "LLM inferred {} relations for {}",
                        relations.len(),
                        crate::safe_truncate(new_memory_id, 12)
                    );
                    return Ok(relations);
                }

                warn!("First infer_relations attempt returned 0 relations, retrying");
                let retry_prompt = format!(
                    "{}\n\nIMPORTANT: Output ONLY a valid JSON array. No markdown, no explanation. Example: [{{\"existing_index\":0,\"type\":\"SUPPORTS\",\"strength\":75}}]",
                    user_prompt
                );
                match llm
                    .generate(system_prompt, &retry_prompt, Some("json_object"))
                    .await
                {
                    Ok((retry_response, _)) => {
                        let retry_relations = parse_relations(&retry_response);
                        debug!("LLM inferred {} relations (retry)", retry_relations.len());
                        Ok(retry_relations)
                    }
                    Err(e) => {
                        warn!("LLM inference retry failed: {}", e);
                        Ok(Vec::new())
                    }
                }
            }
            Err(e) => {
                warn!("LLM inference failed (non-critical): {}", e);
                Ok(Vec::new())
            }
        }
    }
}
