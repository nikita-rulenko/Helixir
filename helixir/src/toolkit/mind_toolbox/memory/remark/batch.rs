use std::sync::Arc;
use chrono::Utc;
use tokio::time::{sleep, Duration};
use tracing::{info, warn, error};
use crate::db::HelixClient;
use crate::llm::extractor::LlmExtractor;
use crate::toolkit::mind_toolbox::entity::EntityManager;
use crate::toolkit::mind_toolbox::ontology::OntologyManager;
use super::models::{RemarkResult, RemarkStats, UnmarkedMemory};
use super::single::remark_single_memory;


pub async fn get_unmarked_memories(
    db_client: &HelixClient,
    user_id: &str,
    limit: usize,
) -> Result<Vec<UnmarkedMemory>, String> {
    info!("Querying unmarked memories (limit={})", limit);
    
    
    let params = serde_json::json!({
        "user_id": user_id,
        "limit": limit,
    });
    
    #[derive(Debug, serde::Deserialize)]
    struct QueryResult {
        memories: Vec<UnmarkedMemory>,
    }
    
    match db_client.execute_query::<QueryResult, _>("getUserMemories", &params).await {
        Ok(result) => {
            info!("Found {} memories to check for markup", result.memories.len());
            Ok(result.memories)
        }
        Err(e) => {
            error!("Failed to query unmarked memories: {}", e);
            Err(format!("Query failed: {}", e))
        }
    }
}


pub async fn remark_batch(
    db_client: &HelixClient,
    llm_extractor: &LlmExtractor,
    entity_manager: &EntityManager,
    ontology_manager: &OntologyManager,
    memories: Vec<UnmarkedMemory>,
    batch_size: usize,
) -> RemarkStats {
    let mut stats = RemarkStats::new();
    stats.started_at = Some(Utc::now());
    
    let total_batches = (memories.len() + batch_size - 1) / batch_size;
    
    info!("Starting batch remark: {} memories in {} batches", memories.len(), total_batches);
    
    for (batch_num, chunk) in memories.chunks(batch_size).enumerate() {
        info!(
            "Processing batch {}/{} ({} memories)...",
            batch_num + 1,
            total_batches,
            chunk.len()
        );
        
        for memory in chunk {
            let result = remark_single_memory(
                db_client,
                llm_extractor,
                entity_manager,
                ontology_manager,
                memory,
            ).await;
            
            stats.add_result(&result);
        }
        
        
        if batch_num + 1 < total_batches {
            sleep(Duration::from_secs(1)).await;
        }
    }
    
    stats.completed_at = Some(Utc::now());
    
    info!(
        "Batch complete: {} processed, {} entities, {} concepts, {} failures (success rate: {:.1}%)",
        stats.total_processed,
        stats.total_entities,
        stats.total_concepts,
        stats.failures,
        stats.success_rate() * 100.0
    );
    
    stats
}


pub async fn remark_all_unmarked(
    db_client: &HelixClient,
    llm_extractor: &LlmExtractor,
    entity_manager: &EntityManager,
    ontology_manager: &OntologyManager,
    user_id: &str,
    batch_size: usize,
) -> Result<RemarkStats, String> {
    info!("Starting remark_all_unmarked for user: {}", user_id);
    
    let memories = get_unmarked_memories(db_client, user_id, 1000).await?;
    
    if memories.is_empty() {
        info!("No unmarked memories found for user: {}", user_id);
        return Ok(RemarkStats::default());
    }
    
    let stats = remark_batch(
        db_client,
        llm_extractor,
        entity_manager,
        ontology_manager,
        memories,
        batch_size,
    ).await;
    
    Ok(stats)
}