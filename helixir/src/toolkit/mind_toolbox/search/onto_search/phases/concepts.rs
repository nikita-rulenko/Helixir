

use tracing::debug;
use crate::db::HelixClient;
use super::super::config::OntoSearchConfig;
use super::super::models::{ConceptMatch, OntoSearchResult, TagMatch};


pub async fn load_memory_concepts(client: &HelixClient, memory_id: &str) -> Vec<String> {
    let params = serde_json::json!({"memory_id": memory_id});

    #[derive(serde::Deserialize, Default)]
    struct ConceptResult {
        #[serde(default)]
        instance_of: Vec<serde_json::Value>,
        #[serde(default)]
        belongs_to: Vec<serde_json::Value>,
    }

    let result: ConceptResult = client
        .execute_query("getMemoryConcepts", &params)
        .await
        .unwrap_or_default();

    let mut concepts = Vec::new();
    for c in result.instance_of.iter().chain(&result.belongs_to) {
        if let Some(id) = c.get("concept_id").and_then(|v| v.as_str()) {
            concepts.push(id.to_string());
        }
    }
    concepts
}


pub fn calculate_concept_overlap(
    query_concepts: &[ConceptMatch],
    memory_concepts: &[String],
    config: &OntoSearchConfig,
) -> f64 {
    if query_concepts.is_empty() || memory_concepts.is_empty() {
        return 0.0;
    }

    let mut total = 0.0;
    let max_score: f64 = query_concepts.iter().map(|c| c.confidence).sum();

    for qc in query_concepts {
        if memory_concepts.contains(&qc.concept_id) {
            total += qc.confidence + config.boost_exact_concept_match;
        }
    }

    if max_score > 0.0 { (total / max_score).min(1.0) } else { 0.0 }
}


pub fn calculate_tag_overlap(query_tags: &[String], content: &str, config: &OntoSearchConfig) -> f64 {
    if query_tags.is_empty() { return 0.0; }

    let content_lower = content.to_lowercase();
    let matches = query_tags.iter().filter(|t| content_lower.contains(t.as_str())).count();
    let score = matches as f64 / query_tags.len() as f64;
    (score + config.boost_tag_match).min(1.0)
}


pub async fn score_by_concepts_and_tags(
    client: &HelixClient,
    results: &mut [OntoSearchResult],
    query_concepts: &[ConceptMatch],
    query_tags: &[String],
    config: &OntoSearchConfig,
) {
    for result in results.iter_mut() {
        
        let memory_concepts = load_memory_concepts(client, &result.memory_id).await;
        result.concept_score = calculate_concept_overlap(query_concepts, &memory_concepts, config);

        
        for qc in query_concepts {
            if memory_concepts.contains(&qc.concept_id) {
                result.matched_concepts.push(qc.clone());
            }
        }

        
        result.tag_score = calculate_tag_overlap(query_tags, &result.content, config);

        
        let content_lower = result.content.to_lowercase();
        for tag in query_tags {
            if content_lower.contains(tag) {
                result.matched_tags.push(TagMatch { tag: tag.clone(), score: 1.0 });
            }
        }

        debug!("Scored {}: concept={:.2}, tag={:.2}", crate::safe_truncate(&result.memory_id, 8), result.concept_score, result.tag_score);
    }
}

