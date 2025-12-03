use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeletionStrategy {
    Soft,     
    Hard,     
    Cascade,  
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletionResult {
    pub memory_id: String,
    pub strategy: DeletionStrategy,
    pub success: bool,
    pub deleted_by: String,
    pub deleted_at: DateTime<Utc>,
    pub reason: Option<String>,
    pub edges_affected: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResult {
    pub memory_id: String,
    pub success: bool,
    pub restored_by: String,
    pub restored_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupStats {
    pub orphaned_entities: usize,
    pub orphaned_edges: usize,
    pub deleted_entities: usize,
    pub deleted_edges: usize,
    pub dry_run: bool,
}

impl Default for CleanupStats {
    fn default() -> Self {
        Self {
            orphaned_entities: 0,
            orphaned_edges: 0,
            deleted_entities: 0,
            deleted_edges: 0,
            dry_run: false,
        }
    }
}

#[derive(Debug, Error)]
pub enum DeletionError {
    #[error("Memory not found: {0}")]
    NotFound(String),
    #[error("Memory already deleted: {0}")]
    AlreadyDeleted(String),
    #[error("Cannot restore hard-deleted memory: {0}")]
    CannotRestore(String),
    #[error("Database error: {0}")]
    Database(String),
}