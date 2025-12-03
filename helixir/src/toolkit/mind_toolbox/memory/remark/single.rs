use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn, error};
use crate::db::HelixClient;
use crate::llm::extractor::LlmExtractor;
use crate::toolkit::mind_toolbox::entity::EntityManager;
use crate::toolkit::mind_toolbox::ontology::OntologyManager;
use super::models::{RemarkResult, UnmarkedMemory};


pub async fn remark_single_memory(
    db_client: &HelixClient,
    llm_extractor: &LlmExtractor,
    entity_manager: &EntityManager,
    ontology_manager: &OntologyManager,
    memory: &UnmarkedMemory,
) -> RemarkResult {
    let start = Instant::now();
    let memory_id = &memory.memory_id;
    let content = &memory.content;
    
    if memory_id.is_empty() || content.is_empty() {
        warn!("Skipping memory with missing ID or content");
        return RemarkResult::failure(memory_id.clone(), "Missing ID or content".to_string());
    }
    
    debug!("Remarking memory: {}...", crate::safe_truncate(memory_id, 8));
    
    
    let extraction = match llm_extractor.extract(
        content,
        true,  
        true,  
        false, 
    ).await {
        Ok(e) => e,
        Err(e) => {
            error!("LLM extraction failed for {}: {}", memory_id, e);
            return RemarkResult::failure(memory_id.clone(), format!("LLM extraction failed: {}", e));
        }
    };
    
    let mut entities_added = 0;
    let mut concepts_added = 0;
    
    
    for entity in extraction.entities.iter() {
        match entity_manager.create_entity(entity).await {
            Ok(entity_dict) => {
                if let Some(entity_id) = entity_dict.get("entity_id").and_then(|v| v.as_str()) {
                    match entity_manager.link_extracted_entity(memory_id, entity_id, 90).await {
                        Ok(_) => {
                            entities_added += 1;
                            debug!("Linked entity '{}' to memory {}", entity.name, crate::safe_truncate(memory_id, 8));
                        }
                        Err(e) => warn!("Failed to link entity '{}': {}", entity.name, e),
                    }
                }
            }
            Err(e) => warn!("Failed to create entity '{}': {}", entity.name, e),
        }
    }
    
    
    if ontology_manager.is_loaded() {
        let concept_mapper = ontology_manager.get_concept_mapper();
        let mapped_concepts = concept_mapper.map_text_to_concepts(content);
        
        for (concept_id, link_type, confidence) in mapped_concepts {
            let query_name = if link_type == "INSTANCE_OF" {
                "linkMemoryToInstanceOf"
            } else {
                "linkMemoryToCategory"
            };
            
            let params = serde_json::json!({
                "memory_id": memory_id,
                "concept_id": concept_id,
                "confidence": confidence,
            });
            
            match db_client.execute_query::<serde_json::Value, _>(query_name, &params).await {
                Ok(_) => {
                    concepts_added += 1;
                    debug!("Linked concept '{}' to memory {}", concept_id, crate::safe_truncate(memory_id, 8));
                }
                Err(e) => warn!("Failed to link concept '{}': {}", concept_id, e),
            }
        }
    }
    
    
    for concept_name in extraction.concepts.iter() {
        if let Some(concept) = ontology_manager.get_concept(concept_name) {
            let params = serde_json::json!({
                "memory_id": memory_id,
                "concept_id": concept.concept_id,
                "confidence": 90,
            });
            
            match db_client.execute_query::<serde_json::Value, _>("linkMemoryToInstanceOf", &params).await {
                Ok(_) => {
                    concepts_added += 1;
                    debug!("Linked LLM concept '{}' to memory {}", concept_name, crate::safe_truncate(memory_id, 8));
                }
                Err(e) => warn!("Failed to link LLM concept '{}': {}", concept_name, e),
            }
        }
    }
    
    let duration_ms = start.elapsed().as_millis() as u64;
    
    info!(
        "Re-marked memory {}: {} entities, {} concepts in {}ms",
        crate::safe_truncate(memory_id, 8),
        entities_added,
        concepts_added,
        duration_ms
    );
    
    RemarkResult::success(memory_id.clone(), entities_added, concepts_added, duration_ms)
}