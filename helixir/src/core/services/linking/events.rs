

use serde::{Deserialize, Serialize};
use uuid::Uuid;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkCreatedEvent {
    
    pub from_chunk_id: String,
    
    pub to_chunk_id: String,
    
    pub edge_type: String,
    
    pub edge_id: Option<Uuid>,
    
    pub correlation_id: Option<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkingCompleteEvent {
    
    pub memory_id: String,
    
    pub edges_created: usize,
    
    pub errors: usize,
    
    pub duration_ms: f64,
    
    pub correlation_id: Option<String>,
}

