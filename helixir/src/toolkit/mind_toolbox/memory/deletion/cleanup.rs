use serde::{Serialize, Deserialize};
use tracing::{debug, info, warn};
use crate::db::HelixClient;
use super::models::{CleanupStats, DeletionError};


pub async fn cleanup_orphans(
    client: &HelixClient,
    dry_run: bool,
) -> Result<CleanupStats, DeletionError> {
    info!("Starting orphan cleanup (dry_run: {})", dry_run);
    
    let mut stats = CleanupStats {
        dry_run,
        ..Default::default()
    };

    
    debug!("Finding orphaned entities...");
    let orphaned_entities = find_orphaned_entities(client).await?;
    stats.orphaned_entities = orphaned_entities.len();
    
    if !orphaned_entities.is_empty() {
        debug!("Found {} orphaned entities", orphaned_entities.len());
        
        if !dry_run {
            let deleted_count = delete_entities(client, &orphaned_entities).await?;
            stats.deleted_entities = deleted_count;
            info!("Deleted {} orphaned entities", deleted_count);
        }
    }

    
    debug!("Finding orphaned edges...");
    let orphaned_edges = find_orphaned_edges(client).await?;
    stats.orphaned_edges = orphaned_edges.len();
    
    if !orphaned_edges.is_empty() {
        debug!("Found {} orphaned edges", orphaned_edges.len());
        
        if !dry_run {
            let deleted_count = delete_edges(client, &orphaned_edges).await?;
            stats.deleted_edges = deleted_count;
            info!("Deleted {} orphaned edges", deleted_count);
        }
    }

    info!("Orphan cleanup completed: {:?}", stats);
    Ok(stats)
}


async fn find_orphaned_entities(
    client: &HelixClient,
) -> Result<Vec<String>, DeletionError> {
    #[derive(Serialize)]
    struct EmptyParams {}
    
    let entity_ids: Vec<String> = client
        .execute_query("findOrphanedEntities", &EmptyParams {})
        .await
        .map_err(|e| DeletionError::Database(e.to_string()))?;
    
    Ok(entity_ids)
}


async fn find_orphaned_edges(
    client: &HelixClient,
) -> Result<Vec<String>, DeletionError> {
    #[derive(Serialize)]
    struct EmptyParams {}
    
    let edge_ids: Vec<String> = client
        .execute_query("findOrphanedEdges", &EmptyParams {})
        .await
        .map_err(|e| DeletionError::Database(e.to_string()))?;
    
    Ok(edge_ids)
}


async fn delete_entities(
    client: &HelixClient,
    entity_ids: &[String],
) -> Result<usize, DeletionError> {
    if entity_ids.is_empty() {
        return Ok(0);
    }

    #[derive(Serialize)]
    struct DeleteEntitiesParams<'a> {
        entity_ids: &'a [String],
    }

    let result: serde_json::Value = client
        .execute_query(
            "deleteEntitiesBatch",
            &DeleteEntitiesParams { entity_ids },
        )
        .await
        .map_err(|e| DeletionError::Database(e.to_string()))?;

    let deleted_count = result
        .get("deleted_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    Ok(deleted_count)
}


async fn delete_edges(
    client: &HelixClient,
    edge_ids: &[String],
) -> Result<usize, DeletionError> {
    if edge_ids.is_empty() {
        return Ok(0);
    }

    #[derive(Serialize)]
    struct DeleteEdgesParams<'a> {
        edge_ids: &'a [String],
    }

    let result: serde_json::Value = client
        .execute_query(
            "deleteEdgesBatch",
            &DeleteEdgesParams { edge_ids },
        )
        .await
        .map_err(|e| DeletionError::Database(e.to_string()))?;

    let deleted_count = result
        .get("deleted_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    Ok(deleted_count)
}