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

Output a JSON OBJECT with a single key "relations" whose value is an array. Each element:
{"existing_index": 0, "type": "IMPLIES|BECAUSE|CONTRADICTS|SUPPORTS", "strength": 0-100}
Example: {"relations": [{"existing_index": 0, "type": "SUPPORTS", "strength": 75}]}

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
- Output ONLY a valid JSON object {"relations": [...]}, no markdown, no explanation
- If truly no connection exists (completely unrelated topics), output {"relations": []}"#;

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
                match parse_relations_response(
                    &response,
                    similar_memories,
                    new_memory_id,
                    new_memory_content,
                ) {
                    // Parsed a relation list (possibly empty). An empty list is a
                    // genuine "no connection" answer — NOT a failure, so no retry.
                    Some(relations) => {
                        info!(
                            "LLM inferred {} relations for {}",
                            relations.len(),
                            crate::safe_truncate(new_memory_id, 12)
                        );
                        Ok(relations)
                    }
                    // Could not parse a relation list at all — one stricter retry.
                    None => {
                        warn!(
                            "infer_relations: unparseable response, retrying (first 200b: {})",
                            &response.chars().take(200).collect::<String>()
                        );
                        let retry_prompt = format!(
                            "{}\n\nIMPORTANT: Output ONLY a valid JSON object of the form {{\"relations\": [...]}}. No markdown, no explanation. Example: {{\"relations\":[{{\"existing_index\":0,\"type\":\"SUPPORTS\",\"strength\":75}}]}}",
                            user_prompt
                        );
                        match llm
                            .generate(system_prompt, &retry_prompt, Some("json_object"))
                            .await
                        {
                            Ok((retry_response, _)) => {
                                let retry_relations = parse_relations_response(
                                    &retry_response,
                                    similar_memories,
                                    new_memory_id,
                                    new_memory_content,
                                )
                                .unwrap_or_default();
                                debug!("LLM inferred {} relations (retry)", retry_relations.len());
                                Ok(retry_relations)
                            }
                            Err(e) => {
                                warn!("LLM inference retry failed: {}", e);
                                Ok(Vec::new())
                            }
                        }
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

/// Parse an `infer_relations` model response into relations.
///
/// Returns `None` ONLY when the response cannot be parsed as a relation list at
/// all — that is the sole case worth a retry. `Some(vec)`, INCLUDING an empty
/// vec, means the model answered; a genuine "no relations" (`{"relations": []}`)
/// is therefore NOT retried, saving a wasted LLM round-trip. Accepts the object
/// form `{"relations": [...]}` (what `json_object` mode forces DeepSeek/OpenAI to
/// return — see #95), a bare array (back-compat), the `results`/`data` keys, or
/// an array embedded in surrounding prose.
fn parse_relations_response(
    response: &str,
    similar_memories: &[(String, String)],
    new_memory_id: &str,
    new_memory_content: &str,
) -> Option<Vec<ReasoningRelation>> {
    let arr: Vec<serde_json::Value> = serde_json::from_str::<Vec<serde_json::Value>>(response)
        .ok()
        .or_else(|| {
            let obj = serde_json::from_str::<serde_json::Value>(response).ok()?;
            obj.get("relations")
                .or_else(|| obj.get("results"))
                .or_else(|| obj.get("data"))
                .and_then(|v| v.as_array())
                .cloned()
        })
        .or_else(|| {
            let start = response.find('[')?;
            let end = response.rfind(']')?;
            serde_json::from_str::<Vec<serde_json::Value>>(&response[start..=end]).ok()
        })?;

    Some(
        arr.iter()
            .filter_map(|r| {
                let idx = r.get("existing_index")?.as_u64()? as usize;
                let (target_id, target_content) = similar_memories.get(idx)?;
                Some(ReasoningRelation {
                    peer_memory_id: String::new(),
                    peer_memory_content: String::new(),
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
    )
}

#[cfg(test)]
mod tests {
    use super::parse_relations_response;
    use super::super::types::ReasoningType;

    fn cands() -> Vec<(String, String)> {
        vec![
            ("mem_a".to_string(), "Rust is a systems language".to_string()),
            (
                "mem_b".to_string(),
                "The compiler enforces ownership".to_string(),
            ),
        ]
    }

    #[test]
    fn parses_object_wrapper() {
        // The json_object shape DeepSeek is forced to return (#95).
        let rels = parse_relations_response(
            r#"{"relations":[{"existing_index":1,"type":"BECAUSE","strength":80}]}"#,
            &cands(),
            "mem_new",
            "new",
        )
        .expect("object wrapper must parse");
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].to_memory_id, "mem_b");
        assert_eq!(rels[0].relation_type, ReasoningType::Because);
        assert_eq!(rels[0].strength, 80);
    }

    #[test]
    fn parses_bare_array_backcompat() {
        let rels = parse_relations_response(
            r#"[{"existing_index":0,"type":"SUPPORTS","strength":60}]"#,
            &cands(),
            "mem_new",
            "new",
        )
        .expect("bare array must still parse");
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].relation_type, ReasoningType::Supports);
    }

    #[test]
    fn empty_relations_is_an_answer_not_a_failure() {
        // {"relations": []} is a genuine "no connection" — Some(empty), no retry.
        let r = parse_relations_response(r#"{"relations":[]}"#, &cands(), "m", "n");
        assert!(r.is_some(), "empty list must NOT trigger a retry");
        assert!(r.unwrap().is_empty());
    }

    #[test]
    fn unparseable_is_none_and_triggers_retry() {
        assert!(parse_relations_response("not json at all", &cands(), "m", "n").is_none());
        assert!(
            parse_relations_response("{}", &cands(), "m", "n").is_none(),
            "an object with no relation list is a parse miss, worth a retry"
        );
    }

    #[test]
    fn alternative_key_and_out_of_range_index_are_tolerated() {
        // 'results' key is accepted; an out-of-range index is skipped, not fatal.
        let rels = parse_relations_response(
            r#"{"results":[{"existing_index":9,"type":"SUPPORTS","strength":50}]}"#,
            &cands(),
            "m",
            "n",
        )
        .expect("parsed even though the index is bogus");
        assert!(rels.is_empty());
    }
}
