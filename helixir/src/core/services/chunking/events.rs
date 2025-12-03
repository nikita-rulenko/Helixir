

use serde::{Deserialize, Serialize};
use uuid::Uuid;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkingStartedEvent {
    
    pub memory_id: String,
    
    pub internal_id: Uuid,
    
    pub content_length: usize,
    
    pub estimated_chunks: usize,
    
    pub chunking_strategy: String,
    
    pub correlation_id: Option<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkCreatedEvent {
    
    pub chunk_id: String,
    
    pub chunk_internal_id: Option<Uuid>,
    
    pub parent_memory_id: String,
    
    pub parent_internal_id: Uuid,
    
    pub position: usize,
    
    pub content: String,
    
    pub token_count: usize,
    
    pub total_chunks: usize,
    
    pub correlation_id: Option<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkingCompleteEvent {
    
    pub memory_id: String,
    
    pub chunks_created: usize,
    
    pub links_created: usize,
    
    pub chains_created: usize,
    
    pub duration_ms: f64,
    
    pub success: bool,
    
    pub correlation_id: Option<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkingFailedEvent {
    
    pub memory_id: String,
    
    pub stage: String,
    
    pub error: String,
    
    pub correlation_id: Option<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkLinkedEvent {
    
    pub chunk_id: String,
    
    pub parent_memory_id: String,
    
    pub position: usize,
    
    pub correlation_id: Option<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkChainedEvent {
    
    pub from_chunk_id: String,
    
    pub to_chunk_id: String,
    
    pub edge_id: Option<Uuid>,
    
    pub position: usize,
    
    pub correlation_id: Option<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCreatedEvent {
    
    pub memory_id: String,
    
    pub internal_id: Option<Uuid>,
    
    pub content: String,
    
    pub needs_chunking: bool,
    
    pub user_id: String,
    
    pub correlation_id: Option<String>,
}

impl MemoryCreatedEvent {
    
    pub fn new(memory_id: String, content: String, user_id: String) -> Self {
        Self {
            memory_id,
            internal_id: None,
            needs_chunking: content.len() >= 1000,
            content,
            user_id,
            correlation_id: None,
        }
    }

    
    pub fn with_internal_id(mut self, id: Uuid) -> Self {
        self.internal_id = Some(id);
        self
    }

    
    pub fn with_correlation_id(mut self, id: String) -> Self {
        self.correlation_id = Some(id);
        self
    }
}

