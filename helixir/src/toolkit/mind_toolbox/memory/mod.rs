

pub mod models;
pub mod crud;
pub mod evolution;
pub mod context;
pub mod retrieval;


pub use models::{Memory, Entity, EntityType, MemoryStats, Context, MemoryBuilder};
pub use crud::{MemoryCrud, CrudError};
pub use evolution::{MemoryEvolution, EvolutionError, EvolutionResult};
pub use context::{ContextManager, ContextDef, ContextError};
pub use retrieval::{RetrievalManager, RetrievalResult, RetrievalDepth, RetrievalError};

use crate::db::HelixClient;
use std::sync::Arc;
use crate::llm::embeddings::EmbeddingGenerator;


pub struct MemoryManager {
    pub crud: MemoryCrud,
}

impl MemoryManager {
    pub fn new(client: HelixClient, embedder: Option<Arc<EmbeddingGenerator>>) -> Self {
        Self {
            crud: MemoryCrud::new(client, embedder),
        }
    }

    pub async fn add_memory(
        &self,
        content: String,
        user_id: String,
        memory_type: Option<String>,
        certainty: Option<i64>,
        importance: Option<i64>,
        source: Option<String>,
        context_tags: Option<String>,
        metadata: Option<String>,
    ) -> Result<Memory, CrudError> {
        self.crud.add_memory(content, user_id, memory_type, certainty, importance, source, context_tags, metadata).await
    }

    pub async fn get_memory(&self, memory_id: &str) -> Result<Option<Memory>, CrudError> {
        self.crud.get_memory(memory_id).await
    }
}
