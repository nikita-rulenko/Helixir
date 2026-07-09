pub mod context;
pub mod crud;
pub mod evolution;
pub mod models;
pub mod retrieval;

pub use context::{ContextDef, ContextError, ContextManager};
pub use crud::{CrudError, MemoryCrud, NewMemory};
pub use evolution::{EvolutionError, EvolutionResult, MemoryEvolution};
pub use models::{Context, Entity, EntityType, Memory, MemoryBuilder, MemoryStats};
pub use retrieval::{RetrievalDepth, RetrievalError, RetrievalManager, RetrievalResult};

use crate::db::HelixClient;
use crate::llm::embeddings::EmbeddingGenerator;
use std::sync::Arc;

pub struct MemoryManager {
    pub crud: MemoryCrud,
}

impl MemoryManager {
    pub fn new(client: HelixClient, embedder: Option<Arc<EmbeddingGenerator>>) -> Self {
        Self {
            crud: MemoryCrud::new(client, embedder),
        }
    }

    pub async fn add_memory(&self, new: NewMemory) -> Result<Memory, CrudError> {
        self.crud.add_memory(new).await
    }

    pub async fn get_memory(&self, memory_id: &str) -> Result<Option<Memory>, CrudError> {
        self.crud.get_memory(memory_id).await
    }
}
