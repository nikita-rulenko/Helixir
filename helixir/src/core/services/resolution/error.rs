

use thiserror::Error;
use uuid::Uuid;


#[derive(Error, Debug)]
pub enum ResolutionError {
    
    #[error("Memory not found: {0}")]
    NotFound(String),

    
    #[error("Database error: {0}")]
    Database(String),

    
    #[error("Invalid UUID format: {0}")]
    InvalidUuid(String),

    
    #[error("Cache error: {0}")]
    Cache(String),
}


#[derive(Error, Debug)]
pub enum BatchResolutionError {
    
    #[error("Batch resolution failed for {0} IDs")]
    PartialFailure(usize),

    
    #[error("All {0} IDs failed to resolve")]
    TotalFailure(usize),

    
    #[error("Resolution failed for {memory_id}: {error}")]
    SingleFailure {
        memory_id: String,
        error: String,
    },
}


#[derive(Debug)]
pub struct BatchResult {
    
    pub resolved: std::collections::HashMap<String, Uuid>,
    
    pub failed: Vec<(String, String)>,
}

impl BatchResult {
    
    pub fn is_complete(&self) -> bool {
        self.failed.is_empty()
    }

    
    pub fn success_count(&self) -> usize {
        self.resolved.len()
    }

    
    pub fn failure_count(&self) -> usize {
        self.failed.len()
    }
}

