

use std::collections::HashMap;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::db::HelixClient;
use super::models::Memory;
use crate::toolkit::mind_toolbox::search::{SearchEngine, SearchError};


#[derive(Error, Debug)]
pub enum RetrievalError {
    #[error("Search failed: {0}")]
    Search(#[from] SearchError),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Reconstruction failed: {0}")]
    Reconstruction(String),
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetrievalDepth {
    
    Shallow,
    
    Medium,
    
    Deep,
}

impl Default for RetrievalDepth {
    fn default() -> Self {
        Self::Medium
    }
}

impl From<&str> for RetrievalDepth {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "shallow" => Self::Shallow,
            "deep" => Self::Deep,
            _ => Self::Medium,
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    
    pub memories: Vec<Memory>,
    
    pub chunks_reconstructed: usize,
    
    pub context_memories: Vec<Memory>,
    
    pub reasoning_chains: Vec<ReasoningChain>,
    
    pub entities: Vec<EntityRef>,
    
    pub metadata: HashMap<String, serde_json::Value>,
}

impl RetrievalResult {
    
    pub fn empty() -> Self {
        Self {
            memories: Vec::new(),
            chunks_reconstructed: 0,
            context_memories: Vec::new(),
            reasoning_chains: Vec::new(),
            entities: Vec::new(),
            metadata: HashMap::new(),
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningChain {
    pub from_memory_id: String,
    pub to_memory_id: String,
    pub relation_type: String,
    pub strength: i32,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRef {
    pub entity_id: String,
    pub name: String,
    pub entity_type: String,
}


pub struct ChunkReconstructor {
    client: Arc<HelixClient>,
}

impl ChunkReconstructor {
    pub fn new(client: Arc<HelixClient>) -> Self {
        info!("ChunkReconstructor initialized");
        Self { client }
    }

    
    pub async fn reconstruct_memory(&self, memory_id: &str) -> Result<(String, usize), RetrievalError> {
        debug!("Reconstructing memory: {}", crate::safe_truncate(memory_id, 12));

        #[derive(Serialize)]
        struct GetChunksParams {
            memory_id: String,
        }

        #[derive(Deserialize)]
        struct ChunkData {
            text: String,
            position: i32,
        }

        #[derive(Deserialize)]
        struct ChunksResult {
            has_chunks: bool,
            content: Option<String>,
            chunks: Option<Vec<ChunkData>>,
        }

        match self.client.execute_query::<ChunksResult, _>(
            "getMemoryWithChunks",
            &GetChunksParams { memory_id: memory_id.to_string() },
        ).await {
            Ok(result) => {
                if !result.has_chunks {
                    
                    let content = result.content.unwrap_or_default();
                    debug!("No chunks for memory, returning direct content");
                    return Ok((content, 0));
                }

                
                if let Some(mut chunks) = result.chunks {
                    chunks.sort_by_key(|c| c.position);
                    let full_content: String = chunks.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join(" ");
                    let chunk_count = chunks.len();
                    
                    info!("✅ Reconstructed {} chunks for memory {}", chunk_count, crate::safe_truncate(memory_id, 12));
                    Ok((full_content, chunk_count))
                } else {
                    Ok((String::new(), 0))
                }
            }
            Err(e) => {
                warn!("Failed to get chunks for memory {}: {}", memory_id, e);
                
                Ok((String::new(), 0))
            }
        }
    }
}


pub struct ContextAssembler {
    client: Arc<HelixClient>,
}

impl ContextAssembler {
    pub fn new(client: Arc<HelixClient>) -> Self {
        info!("ContextAssembler initialized");
        Self { client }
    }

    
    pub async fn gather_context(
        &self,
        memory_id: &str,
        include_reasoning: bool,
        include_entities: bool,
        max_depth: usize,
    ) -> Result<(Vec<Memory>, Vec<ReasoningChain>, Vec<EntityRef>), RetrievalError> {
        debug!(
            "Gathering context for memory {} (reasoning={}, entities={}, depth={})",
            crate::safe_truncate(memory_id, 12),
            include_reasoning,
            include_entities,
            max_depth
        );

        let context_memories = Vec::new();
        let mut reasoning_chains = Vec::new();
        let mut entities = Vec::new();

        
        if include_reasoning {
            #[derive(Serialize)]
            struct GetRelationsParams {
                memory_id: String,
                max_depth: usize,
            }

            #[derive(Deserialize)]
            struct RelationData {
                from_id: String,
                to_id: String,
                relation_type: String,
                strength: i32,
            }

            if let Ok(relations) = self.client.execute_query::<Vec<RelationData>, _>(
                "getMemoryReasoningRelations",
                &GetRelationsParams {
                    memory_id: memory_id.to_string(),
                    max_depth,
                },
            ).await {
                reasoning_chains = relations
                    .into_iter()
                    .map(|r| ReasoningChain {
                        from_memory_id: r.from_id,
                        to_memory_id: r.to_id,
                        relation_type: r.relation_type,
                        strength: r.strength,
                    })
                    .collect();
                debug!("Found {} reasoning relations", reasoning_chains.len());
            }
        }

        
        if include_entities {
            #[derive(Serialize)]
            struct GetEntitiesParams {
                memory_id: String,
            }

            #[derive(Deserialize)]
            struct EntityData {
                entity_id: String,
                name: String,
                entity_type: String,
            }

            if let Ok(entity_list) = self.client.execute_query::<Vec<EntityData>, _>(
                "getMemoryEntities",
                &GetEntitiesParams { memory_id: memory_id.to_string() },
            ).await {
                entities = entity_list
                    .into_iter()
                    .map(|e| EntityRef {
                        entity_id: e.entity_id,
                        name: e.name,
                        entity_type: e.entity_type,
                    })
                    .collect();
                debug!("Found {} entities", entities.len());
            }
        }

        Ok((context_memories, reasoning_chains, entities))
    }
}


pub struct RetrievalManager {
    search_engine: Arc<SearchEngine>,
    reconstructor: ChunkReconstructor,
    assembler: ContextAssembler,
}

impl RetrievalManager {
    
    pub fn new(client: Arc<HelixClient>, search_engine: Arc<SearchEngine>) -> Self {
        info!("RetrievalManager initialized");
        Self {
            search_engine,
            reconstructor: ChunkReconstructor::new(Arc::clone(&client)),
            assembler: ContextAssembler::new(client),
        }
    }

    
    pub async fn retrieve(
        &self,
        query: &str,
        query_embedding: &[f32],
        user_id: &str,
        depth: RetrievalDepth,
        limit: usize,
        include_reasoning: bool,
        include_entities: bool,
    ) -> Result<RetrievalResult, RetrievalError> {
        info!(
            "Retrieving: '{}...' [depth={:?}, limit={}]",
            crate::safe_truncate(query, 50),
            depth,
            limit
        );

        
        let mode = match depth {
            RetrievalDepth::Shallow => "recent",
            RetrievalDepth::Medium => "contextual",
            RetrievalDepth::Deep => "deep",
        };

        let search_results = self.search_engine
            .search(query, query_embedding, user_id, limit, mode, None)
            .await?;

        
        let now = chrono::Utc::now().to_rfc3339();
        let mut memories: Vec<Memory> = search_results
            .iter()
            .map(|r| Memory {
                memory_id: r.memory_id.clone(),
                content: r.content.clone(),
                memory_type: r.metadata.get("memory_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("fact")
                    .to_string(),
                user_id: user_id.to_string(),
                certainty: 100,
                importance: 50,
                created_at: now.clone(),
                updated_at: now.clone(),
                valid_from: now.clone(),
                valid_until: String::new(),
                immutable: 0,
                verified: 0,
                source: String::new(),
                context_tags: String::new(),
                metadata: String::new(),
                is_deleted: 0,
                deleted_at: String::new(),
                deleted_by: String::new(),
                concepts: Vec::new(),
            })
            .collect();

        let mut total_chunks = 0;

        
        if depth != RetrievalDepth::Shallow {
            for memory in &mut memories {
                match self.reconstructor.reconstruct_memory(&memory.memory_id).await {
                    Ok((full_content, chunks)) => {
                        if chunks > 0 {
                            memory.content = full_content;
                            total_chunks += chunks;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to reconstruct memory {}: {}", memory.memory_id, e);
                    }
                }
            }
        }

        
        let mut all_context_memories = Vec::new();
        let mut all_reasoning_chains = Vec::new();
        let mut all_entities = Vec::new();

        if depth != RetrievalDepth::Shallow {
            let max_depth = match depth {
                RetrievalDepth::Medium => 1,
                RetrievalDepth::Deep => 2,
                _ => 0,
            };

            for memory in &memories {
                match self.assembler.gather_context(
                    &memory.memory_id,
                    include_reasoning,
                    include_entities,
                    max_depth,
                ).await {
                    Ok((ctx_mems, chains, ents)) => {
                        all_context_memories.extend(ctx_mems);
                        all_reasoning_chains.extend(chains);
                        all_entities.extend(ents);
                    }
                    Err(e) => {
                        warn!("Failed to gather context for {}: {}", memory.memory_id, e);
                    }
                }
            }
        }

        
        let mut metadata = HashMap::new();
        metadata.insert("depth".to_string(), serde_json::json!(format!("{:?}", depth)));
        metadata.insert("query".to_string(), serde_json::json!(query));
        metadata.insert("mode".to_string(), serde_json::json!(mode));

        info!(
            "✅ Retrieved {} memories ({} chunks, {} context, {} reasoning, {} entities)",
            memories.len(),
            total_chunks,
            all_context_memories.len(),
            all_reasoning_chains.len(),
            all_entities.len()
        );

        Ok(RetrievalResult {
            memories,
            chunks_reconstructed: total_chunks,
            context_memories: all_context_memories,
            reasoning_chains: all_reasoning_chains,
            entities: all_entities,
            metadata,
        })
    }
}

impl std::fmt::Debug for RetrievalManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetrievalManager")
            .field("search", &"SearchEngine")
            .field("reconstruct", &"ChunkReconstructor")
            .field("assemble", &"ContextAssembler")
            .finish()
    }
}

