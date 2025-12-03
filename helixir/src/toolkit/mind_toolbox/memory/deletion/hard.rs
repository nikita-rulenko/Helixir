use chrono::Utc;
use serde::Serialize;
use tracing::{debug, info, warn, error};
use crate::db::HelixClient;
use super::models::{DeletionResult, DeletionStrategy, DeletionError};

#[derive(Serialize)]
struct HardDeleteInput {
    memory_id: String,
}

#[derive(Serialize)]
struct DeleteEdgesInput {
    memory_id: String,
}

#[derive(Serialize)]
struct EdgeCountInput {
    memory_id: String,
}


pub async fn hard_delete(
    client: &HelixClient,
    memory_id: &str,
    deleted_by: &str,
    cascade: bool,
) -> Result<DeletionResult, DeletionError> {
    warn!("HARD DELETE requested for memory {} by user {} - THIS IS IRREVERSIBLE!", memory_id, deleted_by);

    let edges_affected = if cascade {
        debug!("Cascade delete enabled - removing edges for memory {}", memory_id);
        match cascade_delete_edges(client, memory_id).await {
            Ok(count) => {
                info!("Successfully deleted {} edges for memory {}", count, memory_id);
                count
            }
            Err(e) => {
                error!("Failed to cascade delete edges for memory {}: {}", memory_id, e);
                return Err(e);
            }
        }
    } else {
        0
    };

    debug!("Executing hard delete for memory {}", memory_id);
    let delete_input = HardDeleteInput {
        memory_id: memory_id.to_string(),
    };

    match client.execute_query::<bool, _>("hardDeleteMemory", &delete_input).await {
        Ok(success) => {
            if success {
                info!("Successfully hard deleted memory {}", memory_id);
                Ok(DeletionResult {
                    memory_id: memory_id.to_string(),
                    strategy: DeletionStrategy::Hard,
                    success: true,
                    deleted_by: deleted_by.to_string(),
                    deleted_at: Utc::now(),
                    reason: Some("Hard delete requested".to_string()),
                    edges_affected,
                })
            } else {
                error!("Hard delete query returned false for memory {}", memory_id);
                Err(DeletionError::Database(format!("Hard delete failed for memory {}", memory_id)))
            }
        }
        Err(e) => {
            error!("Failed to hard delete memory {}: {}", memory_id, e);
            Err(DeletionError::Database(e.to_string()))
        }
    }
}


async fn cascade_delete_edges(
    client: &HelixClient,
    memory_id: &str,
) -> Result<usize, DeletionError> {
    debug!("Counting edges for memory {} before cascade delete", memory_id);
    
    let count_input = EdgeCountInput {
        memory_id: memory_id.to_string(),
    };

    let edge_count = match client.execute_query::<usize, _>("getMemoryEdgeCount", &count_input).await {
        Ok(count) => {
            debug!("Found {} edges connected to memory {}", count, memory_id);
            count
        }
        Err(e) => {
            warn!("Could not count edges for memory {}: {}", memory_id, e);
            0
        }
    };

    if edge_count == 0 {
        debug!("No edges to delete for memory {}", memory_id);
        return Ok(0);
    }

    debug!("Deleting all edges for memory {}", memory_id);
    let delete_input = DeleteEdgesInput {
        memory_id: memory_id.to_string(),
    };

    match client.execute_query::<bool, _>("deleteMemoryEdges", &delete_input).await {
        Ok(success) => {
            if success {
                info!("Successfully deleted {} edges for memory {}", edge_count, memory_id);
                Ok(edge_count)
            } else {
                error!("Edge deletion query returned false for memory {}", memory_id);
                Err(DeletionError::Database(format!("Failed to delete edges for memory {}", memory_id)))
            }
        }
        Err(e) => {
            error!("Failed to delete edges for memory {}: {}", memory_id, e);
            Err(DeletionError::Database(e.to_string()))
        }
    }
}