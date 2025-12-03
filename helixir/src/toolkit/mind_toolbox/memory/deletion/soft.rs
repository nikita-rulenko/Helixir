use chrono::Utc;
use serde::Serialize;
use tracing::{debug, error, info, warn};
use crate::db::HelixClient;
use super::models::{DeletionResult, DeletionStrategy, RestoreResult, DeletionError};

#[derive(Serialize)]
struct SoftDeleteInput {
    memory_id: String,
    deleted_by: String,
    deleted_at: String,
    reason: String,
}

#[derive(Serialize)]
struct RestoreInput {
    memory_id: String,
    restored_by: String,
    restored_at: String,
}

#[derive(Serialize)]
struct GetMemoryInput {
    memory_id: String,
}


pub async fn soft_delete(
    client: &HelixClient,
    memory_id: &str,
    deleted_by: &str,
    reason: Option<&str>,
) -> Result<DeletionResult, DeletionError> {
    debug!("Attempting soft delete for memory: {}", memory_id);

    
    let get_input = GetMemoryInput {
        memory_id: memory_id.to_string(),
    };

    match client.execute_query::<serde_json::Value, _>("getMemory", &get_input).await {
        Ok(_) => {
            debug!("Memory {} exists, proceeding with soft delete", memory_id);
        }
        Err(e) => {
            warn!("Memory {} not found: {}", memory_id, e);
            return Err(DeletionError::NotFound(memory_id.to_string()));
        }
    }

    
    let delete_input = SoftDeleteInput {
        memory_id: memory_id.to_string(),
        deleted_by: deleted_by.to_string(),
        deleted_at: Utc::now().to_rfc3339(),
        reason: reason.unwrap_or("").to_string(),
    };

    match client.execute_query::<serde_json::Value, _>("softDeleteMemory", &delete_input).await {
        Ok(_) => {
            info!("Successfully soft deleted memory: {}", memory_id);
            Ok(DeletionResult {
                memory_id: memory_id.to_string(),
                strategy: DeletionStrategy::Soft,
                success: true,
                deleted_by: deleted_by.to_string(),
                deleted_at: Utc::now(),
                reason: reason.map(|s| s.to_string()),
                edges_affected: 0,
            })
        }
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("already deleted") {
                warn!("Memory {} already deleted: {}", memory_id, e);
                Err(DeletionError::AlreadyDeleted(memory_id.to_string()))
            } else {
                error!("Failed to soft delete memory {}: {}", memory_id, e);
                Err(DeletionError::Database(err_str))
            }
        }
    }
}


pub async fn undelete(
    client: &HelixClient,
    memory_id: &str,
    restored_by: &str,
) -> Result<RestoreResult, DeletionError> {
    debug!("Attempting to restore memory: {}", memory_id);

    
    let get_input = GetMemoryInput {
        memory_id: memory_id.to_string(),
    };

    match client.execute_query::<serde_json::Value, _>("getMemory", &get_input).await {
        Ok(_) => {
            debug!("Memory {} exists, proceeding with restore", memory_id);
        }
        Err(e) => {
            warn!("Memory {} not found: {}", memory_id, e);
            return Err(DeletionError::NotFound(memory_id.to_string()));
        }
    }

    
    let restore_input = RestoreInput {
        memory_id: memory_id.to_string(),
        restored_by: restored_by.to_string(),
        restored_at: Utc::now().to_rfc3339(),
    };

    match client.execute_query::<serde_json::Value, _>("restoreMemory", &restore_input).await {
        Ok(_) => {
            info!("Successfully restored memory: {}", memory_id);
            Ok(RestoreResult {
                memory_id: memory_id.to_string(),
                success: true,
                restored_by: restored_by.to_string(),
                restored_at: Utc::now(),
            })
        }
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("hard deleted") {
                warn!("Cannot restore hard-deleted memory {}: {}", memory_id, e);
                Err(DeletionError::CannotRestore(memory_id.to_string()))
            } else {
                error!("Failed to restore memory {}: {}", memory_id, e);
                Err(DeletionError::Database(err_str))
            }
        }
    }
}