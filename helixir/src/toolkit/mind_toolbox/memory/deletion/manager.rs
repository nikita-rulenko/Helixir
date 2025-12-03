use std::sync::Arc;
use tracing::info;
use crate::db::HelixClient;
use super::models::{DeletionResult, RestoreResult, CleanupStats, DeletionError, DeletionStrategy};
use super::soft::{soft_delete, undelete};
use super::hard::hard_delete;
use super::cleanup::cleanup_orphans;


pub struct DeletionManager {
    client: Arc<HelixClient>,
}

impl DeletionManager {
    pub fn new(client: Arc<HelixClient>) -> Self {
        info!("Initializing DeletionManager");
        Self { client }
    }
    
    
    pub async fn soft_delete(
        &self,
        memory_id: &str,
        deleted_by: &str,
        reason: Option<&str>,
    ) -> Result<DeletionResult, DeletionError> {
        soft_delete(&self.client, memory_id, deleted_by, reason).await
    }
    
    
    pub async fn hard_delete(
        &self,
        memory_id: &str,
        deleted_by: &str,
        cascade: bool,
    ) -> Result<DeletionResult, DeletionError> {
        hard_delete(&self.client, memory_id, deleted_by, cascade).await
    }
    
    
    pub async fn undelete(
        &self,
        memory_id: &str,
        restored_by: &str,
    ) -> Result<RestoreResult, DeletionError> {
        undelete(&self.client, memory_id, restored_by).await
    }
    
    
    pub async fn cleanup_orphans(
        &self,
        dry_run: bool,
    ) -> Result<CleanupStats, DeletionError> {
        cleanup_orphans(&self.client, dry_run).await
    }
    
    
    pub async fn delete(
        &self,
        memory_id: &str,
        deleted_by: &str,
        strategy: DeletionStrategy,
        reason: Option<&str>,
    ) -> Result<DeletionResult, DeletionError> {
        match strategy {
            DeletionStrategy::Soft => {
                self.soft_delete(memory_id, deleted_by, reason).await
            }
            DeletionStrategy::Hard => {
                self.hard_delete(memory_id, deleted_by, false).await
            }
            DeletionStrategy::Cascade => {
                self.hard_delete(memory_id, deleted_by, true).await
            }
        }
    }
}