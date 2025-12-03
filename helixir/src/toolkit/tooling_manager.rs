

use std::collections::HashMap;
use std::sync::Arc;


fn safe_truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

use serde::{Deserialize, Serialize};
use tracing::{info, debug, warn};

use crate::db::HelixClient;
use crate::llm::decision::{LLMDecisionEngine, MemoryDecision, MemoryOperation, SimilarMemory};
use crate::llm::extractor::LlmExtractor;
use crate::llm::providers::base::LlmProvider;
use crate::llm::EmbeddingGenerator;
use crate::toolkit::mind_toolbox::chunking::{ChunkingManager, ChunkingError, DEFAULT_THRESHOLD};
use crate::toolkit::mind_toolbox::entity::{EntityManager, EntityEdgeType, EntityError};
use crate::toolkit::mind_toolbox::ontology::{OntologyManager, OntologyError};
use crate::toolkit::mind_toolbox::reasoning::{ReasoningEngine, ReasoningType, ReasoningError};
use crate::toolkit::mind_toolbox::search::{SearchEngine, SearchEngineConfig, SearchError};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddMemoryResult {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub deleted: Vec<String>,
    pub skipped: usize,
    pub entities_extracted: usize,
    pub reasoning_relations_created: usize,
    pub chunks_created: usize,
    pub metadata: HashMap<String, serde_json::Value>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMemoryResult {
    pub memory_id: String,
    pub content: String,
    pub score: f64,
    pub method: String,
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: String,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningChainSearchResult {
    pub chains: Vec<ToolingReasoningChain>,
    pub total_memories: usize,
    pub deepest_chain: usize,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolingReasoningChain {
    pub seed: SearchMemoryResult,
    pub nodes: Vec<ChainNode>,
    pub chain_type: String,
    pub reasoning_trail: String,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainNode {
    pub memory_id: String,
    pub content: String,
    pub relation: String,
    pub depth: usize,
}


#[derive(Debug, thiserror::Error)]
pub enum ToolingError {
    #[error("Embedding failed: {0}")]
    Embedding(String),
    #[error("Extraction failed: {0}")]
    Extraction(String),
    #[error("Chunking failed: {0}")]
    Chunking(#[from] ChunkingError),
    #[error("Entity operation failed: {0}")]
    Entity(#[from] EntityError),
    #[error("Ontology operation failed: {0}")]
    Ontology(#[from] OntologyError),
    #[error("Reasoning operation failed: {0}")]
    Reasoning(#[from] ReasoningError),
    #[error("Memory operation failed: {0}")]
    Memory(String),
    #[error("Search failed: {0}")]
    Search(#[from] SearchError),
    #[error("Database error: {0}")]
    Database(String),
}


pub struct ToolingManager {
    db: Arc<HelixClient>,
    embedder: Arc<EmbeddingGenerator>,
    llm_provider: Arc<dyn LlmProvider>,
    extractor: LlmExtractor<Arc<dyn LlmProvider>>,
    decision_engine: LLMDecisionEngine,
    chunking_manager: ChunkingManager,
    entity_manager: EntityManager,
    ontology_manager: parking_lot::RwLock<OntologyManager>,
    reasoning_engine: ReasoningEngine,
    search_engine: SearchEngine,
}

impl ToolingManager {
    
    pub fn new(
        db: Arc<HelixClient>,
        embedder: Arc<EmbeddingGenerator>,
        llm_provider: Arc<dyn LlmProvider>,
    ) -> Self {
        info!("ToolingManager initialized with full pipeline");
        
        
        let extractor = LlmExtractor::new(Arc::clone(&llm_provider));
        
        
        let decision_engine = LLMDecisionEngine::new(Arc::clone(&llm_provider));
        
        
        let chunking_manager = ChunkingManager::new(
            Arc::clone(&db),
            Some(Arc::clone(&embedder)),
        );
        
        
        let entity_manager = EntityManager::new(Arc::clone(&db), 1000);
        
        
        let ontology_manager = parking_lot::RwLock::new(OntologyManager::new(Arc::clone(&db)));
        
        
        let reasoning_engine = ReasoningEngine::new(
            Arc::clone(&db),
            Some(Arc::clone(&llm_provider)),
            500,
        );
        
        
        let search_engine = SearchEngine::new(
            Arc::clone(&db),
            Arc::clone(&embedder),
            SearchEngineConfig::default(),
        );
        
        Self { 
            db, 
            embedder, 
            llm_provider, 
            extractor,
            decision_engine,
            chunking_manager,
            entity_manager,
            ontology_manager,
            reasoning_engine,
            search_engine,
        }
    }

    pub async fn initialize(&self) -> Result<(), ToolingError> {
        info!("Initializing ToolingManager - loading ontology");
        
        let needs_load = {
            let ontology = self.ontology_manager.read();
            !ontology.is_loaded()
        };
        
        if needs_load {
            let db = Arc::clone(&self.db);
            let mut ontology_manager = OntologyManager::new(db);
            ontology_manager.load().await.map_err(|e| {
                warn!("Failed to load ontology: {}", e);
                ToolingError::from(e)
            })?;
            
            *self.ontology_manager.write() = ontology_manager;
            info!("Ontology loaded successfully");
        }
        Ok(())
    }

    
    pub async fn add_memory(
        &self,
        message: &str,
        user_id: &str,
        _agent_id: Option<&str>,
        _metadata: Option<HashMap<String, serde_json::Value>>,
        context_tags: Option<&str>,
    ) -> Result<AddMemoryResult, ToolingError> {
        
        let preview: String = message.chars().take(50).collect();
        let tags = context_tags.unwrap_or("");
        info!("Adding memory for user={}: {}... [tags={}]", user_id, preview, tags);

        
        debug!("Step 1: LLM extraction");
        let extraction = self
            .extractor
            .extract(message, user_id, true, true)
            .await
            .map_err(|e| ToolingError::Extraction(e.to_string()))?;

        info!(
            "Extracted {} memories, {} entities, {} relations",
            extraction.memories.len(),
            extraction.entities.len(),
            extraction.relations.len()
        );

        let mut added_ids = Vec::new();
        let mut updated_ids = Vec::new();
        let mut skipped = 0usize;
        let mut entities_linked = 0usize;
        let mut relations_created = 0usize;
        let mut chunks_created = 0usize;

        
        let memories_to_store = if extraction.memories.is_empty() {
            debug!("No memories extracted, storing original message");
            vec![crate::llm::extractor::ExtractedMemory {
                text: message.to_string(),
                memory_type: "fact".to_string(),
                certainty: 50,
                importance: 50,
                entities: vec![],
            }]
        } else {
            extraction.memories
        };

        
        for memory in &memories_to_store {
            debug!("Processing memory: {}...", safe_truncate(&memory.text, 30));

            
            let vector = self
                .embedder
                .generate(&memory.text, true)
                .await
                .map_err(|e| ToolingError::Embedding(e.to_string()))?;

            
            let similar_results = self.search_engine
                .search(&memory.text, &vector, user_id, 5, "contextual", None)
                .await
                .unwrap_or_default();

            let similar_memories: Vec<SimilarMemory> = similar_results
                .iter()
                .map(|r| SimilarMemory {
                    id: r.memory_id.clone(),
                    content: r.content.clone(),
                    score: r.score as f64,
                    created_at: None,
                })
                .collect();

            
            let decision = self.decision_engine
                .decide(&memory.text, &similar_memories, user_id)
                .await;

            debug!(
                "Decision: {:?} (confidence={}, target={:?})",
                decision.operation, decision.confidence, decision.target_memory_id
            );

            
            let memory_id = match decision.operation {
                MemoryOperation::Noop => {
                    debug!("NOOP: skipping duplicate memory");
                    skipped += 1;
                    continue;
                }
                MemoryOperation::Update => {
                    
                    if let (Some(target_id), Some(merged)) = (&decision.target_memory_id, &decision.merged_content) {
                        debug!("UPDATE: updating {} with merged content", target_id);
                        self.update_memory_internal(target_id, merged, &vector).await?;
                        updated_ids.push(target_id.to_string());
                        target_id.to_string()
                    } else {
                        
                        let (new_id, new_chunks) = self.store_new_memory(&memory, user_id, &vector, tags).await?;
                        chunks_created += new_chunks;
                        new_id
                    }
                }
                MemoryOperation::Supersede => {
                    
                    let (new_id, new_chunks) = self.store_new_memory(&memory, user_id, &vector, tags).await?;
                    chunks_created += new_chunks;
                    if let Some(old_id) = &decision.supersedes_memory_id {
                        debug!("SUPERSEDE: {} supersedes {}", new_id, old_id);
                        
                        let _ = self.reasoning_engine
                            .add_relation(&new_id, old_id, ReasoningType::Supports, 90, None)
                            .await;
                    }
                    added_ids.push(new_id.clone());
                    new_id
                }
                MemoryOperation::Contradict => {
                    
                    let (new_id, new_chunks) = self.store_new_memory(&memory, user_id, &vector, tags).await?;
                    chunks_created += new_chunks;
                    if let Some(contra_id) = &decision.contradicts_memory_id {
                        debug!("CONTRADICT: {} contradicts {}", new_id, contra_id);
                        let _ = self.reasoning_engine
                            .add_relation(&new_id, contra_id, ReasoningType::Contradicts, 80, None)
                            .await;
                    }
                    added_ids.push(new_id.clone());
                    new_id
                }
                MemoryOperation::Delete => {
                    
                    if let Some(target_id) = &decision.target_memory_id {
                        debug!("DELETE: removing {} before adding new", target_id);
                        let _ = self.delete_memory(target_id).await;
                    }
                    let (new_id, new_chunks) = self.store_new_memory(&memory, user_id, &vector, tags).await?;
                    chunks_created += new_chunks;
                    added_ids.push(new_id.clone());
                    new_id
                }
                MemoryOperation::Add => {
                    
                    let (new_id, new_chunks) = self.store_new_memory(&memory, user_id, &vector, tags).await?;
                    chunks_created += new_chunks;
                    added_ids.push(new_id.clone());
                    new_id
                }
            };

            
            for entity_id in &memory.entities {
                
                if let Some(entity) = extraction.entities.iter().find(|e| &e.id == entity_id) {
                    
                    match self.entity_manager.get_or_create_entity(
                        &entity.name,
                        &entity.entity_type, 
                        None, 
                    ).await {
                        Ok(db_entity) => {
                            
                            if let Err(e) = self.entity_manager.link_to_memory(
                                &db_entity.entity_id,
                                &memory_id,
                                EntityEdgeType::ExtractedEntity,
                                80,  
                                50,  
                                "neutral", 
                            ).await {
                                warn!("Failed to link entity {} to memory {}: {}", db_entity.entity_id, memory_id, e);
                            } else {
                                entities_linked += 1;
                                debug!("Linked entity '{}' to memory {}", entity.name, memory_id);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to get/create entity '{}': {}", entity.name, e);
                        }
                    }
                }
            }

            
            let concept_links: Vec<(String, String, i32)> = {
                let ontology = self.ontology_manager.read();
                if ontology.is_loaded() {
                    ontology.map_memory_to_concepts(&memory.text, Some(&memory.memory_type))
                        .into_iter()
                        .map(|m| (m.concept.id.clone(), m.concept.name.clone(), (m.confidence * 100.0) as i32))
                        .collect()
                } else {
                    Vec::new()
                }
            };
            
            for (concept_id, concept_name, confidence) in concept_links {
                
                if let Err(e) = self.link_memory_to_concept(&memory_id, &concept_id, confidence).await {
                    warn!("Failed to link concept {}: {}", concept_id, e);
                } else {
                    debug!("Linked memory {} to concept '{}'", memory_id, concept_name);
                }
            }
        }

        
        let mut memory_content_to_id: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for (idx, mem) in memories_to_store.iter().enumerate() {
            if idx < added_ids.len() {
                
                let normalized = mem.text.to_lowercase();
                memory_content_to_id.insert(normalized.clone(), added_ids[idx].clone());
                
                let short_key: String = normalized.chars().take(100).collect();
                if short_key.len() < normalized.len() {
                    memory_content_to_id.insert(short_key, added_ids[idx].clone());
                }
            }
        }

        for relation in &extraction.relations {
            debug!(
                "Processing relation: '{}' --{}-> '{}'",
                safe_truncate(&relation.from_memory_content, 30),
                relation.relation_type,
                safe_truncate(&relation.to_memory_content, 30)
            );

            
            let from_id = memory_content_to_id.get(&relation.from_memory_content.to_lowercase())
                .or_else(|| {
                    
                    memory_content_to_id.iter()
                        .find(|(k, _)| {
                            k.contains(&relation.from_memory_content.to_lowercase()) ||
                            relation.from_memory_content.to_lowercase().contains(k.as_str())
                        })
                        .map(|(_, v)| v)
                });

            let to_id = memory_content_to_id.get(&relation.to_memory_content.to_lowercase())
                .or_else(|| {
                    memory_content_to_id.iter()
                        .find(|(k, _)| {
                            k.contains(&relation.to_memory_content.to_lowercase()) ||
                            relation.to_memory_content.to_lowercase().contains(k.as_str())
                        })
                        .map(|(_, v)| v)
                });

            if let (Some(from), Some(to)) = (from_id, to_id) {
                
                let rel_type = match relation.relation_type.to_uppercase().as_str() {
                    "IMPLIES" => ReasoningType::Implies,
                    "BECAUSE" => ReasoningType::Because,
                    "CONTRADICTS" => ReasoningType::Contradicts,
                    "SUPPORTS" => ReasoningType::Supports,
                    _ => ReasoningType::Implies, 
                };

                
                match self.reasoning_engine.add_relation(
                    from,
                    to,
                    rel_type,
                    80, 
                    None, 
                ).await {
                    Ok(rel) => {
                        relations_created += 1;
                        debug!("Created {} relation: {} -> {}", rel.relation_type.edge_name(), from, to);
                    }
                    Err(e) => {
                        warn!("Failed to create relation: {}", e);
                    }
                }
            } else {
                debug!(
                    "Could not find memory IDs for relation: '{}' -> '{}'",
                    safe_truncate(&relation.from_memory_content, 30),
                    safe_truncate(&relation.to_memory_content, 30)
                );
            }
        }

        info!(
            "Memory pipeline complete: {} added, {} updated, {} skipped, {} entities, {} relations",
            added_ids.len(),
            updated_ids.len(),
            skipped,
            entities_linked,
            relations_created
        );

        
        let mut metadata = HashMap::new();
        metadata.insert(
            "provider".to_string(),
            serde_json::Value::String(self.llm_provider.provider_name().to_string()),
        );
        metadata.insert(
            "model".to_string(),
            serde_json::Value::String(self.llm_provider.model_name().to_string()),
        );
        metadata.insert(
            "user_id".to_string(),
            serde_json::Value::String(user_id.to_string()),
        );

        Ok(AddMemoryResult {
            added: added_ids,
            updated: updated_ids,
            deleted: vec![],
            skipped,
            entities_extracted: entities_linked,
            reasoning_relations_created: relations_created,
            chunks_created,
            metadata,
        })
    }

    
    async fn store_new_memory(
        &self,
        memory: &crate::llm::extractor::ExtractedMemory,
        user_id: &str,
        vector: &[f32],
        context_tags: &str,
    ) -> Result<(String, usize), ToolingError> {
        let memory_id = format!(
            "mem_{}",
            uuid::Uuid::new_v4()
                .to_string()
                .replace("-", "")
                .chars()
                .take(12)
                .collect::<String>()
        );
        let now = chrono::Utc::now().to_rfc3339();

        
        #[derive(Serialize)]
        struct AddMemoryInput {
            memory_id: String,
            user_id: String,
            content: String,
            memory_type: String,
            certainty: i64,
            importance: i64,
            created_at: String,
            updated_at: String,
            context_tags: String,
            source: String,
            metadata: String,
        }

        let input = AddMemoryInput {
            memory_id: memory_id.clone(),
            user_id: user_id.to_string(),
            content: memory.text.clone(),
            memory_type: memory.memory_type.clone(),
            certainty: memory.certainty as i64,
            importance: memory.importance as i64,
            created_at: now.clone(),
            updated_at: now.clone(),
            context_tags: context_tags.to_string(),
            source: "llm_extraction".to_string(),
            metadata: "{}".to_string(),
        };

        
        #[derive(Deserialize)]
        struct AddMemoryResponse {
            memory: MemoryNode,
        }
        #[derive(Deserialize)]
        struct MemoryNode {
            id: String,  
        }
        
        let response: AddMemoryResponse = self.db
            .execute_query("addMemory", &input)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        
        let internal_id = response.memory.id;
        debug!("Memory created: {} (internal: {})", memory_id, internal_id);

        
        #[derive(Serialize)]
        struct AddEmbeddingInput {
            memory_id: String,      
            vector_data: Vec<f64>,  
            embedding_model: String,
            created_at: String,
        }

        let embed_input = AddEmbeddingInput {
            memory_id: internal_id,
            vector_data: vector.iter().map(|&x| x as f64).collect(),
            embedding_model: self.embedder.model().to_string(),
            created_at: now.clone(),
        };
        
        if let Err(e) = self.db
            .execute_query::<serde_json::Value, _>("addMemoryEmbedding", &embed_input)
            .await 
        {
            warn!("Failed to add embedding for {}: {}", memory_id, e);
        } else {
            debug!("Embedding added for {}", memory_id);
        }

        
        #[derive(Serialize)]
        struct LinkUserInput {
            user_id: String,
            memory_id: String,
            context: String,
        }

        let _ = self.db
            .execute_query::<serde_json::Value, _>("linkUserToMemory", &LinkUserInput {
                user_id: user_id.to_string(),
                memory_id: memory_id.clone(),
                context: "created".to_string(),
            })
            .await;
        

        let mut chunk_count = 0usize;
        if self.chunking_manager.should_chunk(&memory.text) {
            info!(
                "📦 Content exceeds threshold ({} chars), creating chunks",
                memory.text.chars().count()
            );
            match self.chunking_manager.add_memory_with_chunking(
                &memory_id,
                &memory.text,
                user_id,
                &memory.memory_type,
                memory.certainty as i64,
                memory.importance as i64,
                "llm_extraction",
                "",
                "{}",
            ).await {
                Ok(result) => {
                    chunk_count = result.chunk_count;
                    info!("✅ Created {} chunks for {}", chunk_count, memory_id);
                }
                Err(e) => {
                    warn!("Failed to chunk memory {}: {}", memory_id, e);
                }
            }
        }

        debug!("Stored new memory: {}", memory_id);
        Ok((memory_id, chunk_count))
    }

    
    async fn update_memory_internal(
        &self,
        memory_id: &str,
        new_content: &str,
        vector: &[f32],
    ) -> Result<(), ToolingError> {
        #[derive(Serialize)]
        struct UpdateInput {
            memory_id: String,
            content: String,
            vector: Vec<f32>,
        }

        self.db
            .execute_query::<(), _>("updateMemory", &UpdateInput {
                memory_id: memory_id.to_string(),
                content: new_content.to_string(),
                vector: vector.to_vec(),
            })
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        debug!("Updated memory: {}", memory_id);
        Ok(())
    }

    
    async fn link_memory_to_concept(
        &self,
        memory_id: &str,
        concept_id: &str,
        confidence: i32,
    ) -> Result<(), ToolingError> {
        #[derive(serde::Deserialize)]
        struct LinkResponse {
            #[serde(default)]
            link: serde_json::Value,
        }
        
        self.db
            .execute_query::<LinkResponse, _>(
                "linkMemoryToInstanceOf",
                &serde_json::json!({
                    "memory_id": memory_id,
                    "concept_id": concept_id,
                    "confidence": confidence as i64,
                }),
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        debug!("Linked memory {} to concept {}", memory_id, concept_id);
        Ok(())
    }

    
    pub async fn search_memory(
        &self,
        query: &str,
        user_id: &str,
        limit: Option<usize>,
        mode: &str,
        temporal_days: Option<f64>,
        _graph_depth: Option<usize>,
    ) -> Result<Vec<SearchMemoryResult>, ToolingError> {
        info!(
            "Searching: '{}...' [mode={}, limit={:?}, temporal_days={:?}]", 
            safe_truncate(query, 50), mode, limit, temporal_days
        );

        
        let query_embedding = self
            .embedder
            .generate(query, true)
            .await
            .map_err(|e| ToolingError::Embedding(e.to_string()))?;

        
        let results = self
            .search_engine
            .search(query, &query_embedding, user_id, limit.unwrap_or(10), mode, temporal_days)
            .await?;

        info!("Found {} memories via SearchEngine [method={}]", 
            results.len(),
            results.first().map(|r| r.method.as_str()).unwrap_or("none")
        );

        
        Ok(results
            .into_iter()
            .map(|r| SearchMemoryResult {
                memory_id: r.memory_id,
                content: r.content,
                score: r.score as f64,
                method: r.method,
                metadata: r.metadata,
                created_at: r.created_at,
            })
            .collect())
    }

    /// Search memories by context tag (e.g., "incomplete_thought")
    pub async fn search_by_tag(
        &self,
        tag: &str,
        limit: usize,
    ) -> Result<Vec<SearchMemoryResult>, ToolingError> {
        info!("Searching by tag: {} [limit={}]", tag, limit);

        #[derive(serde::Deserialize)]
        struct TaggedMemory {
            memory_id: String,
            content: String,
            context_tags: String,
            created_at: String,
        }

        // HelixDB returns the array directly under the query return name
        #[derive(serde::Deserialize)]
        struct QueryResult {
            memories: Vec<TaggedMemory>,
        }

        let result: QueryResult = self
            .db
            .execute_query(
                "searchByContextTag",
                &serde_json::json!({
                    "tag": tag,
                    "limit": limit as i64
                }),
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        info!("Found {} memories with tag '{}'", result.memories.len(), tag);

        Ok(result
            .memories
            .into_iter()
            .map(|m| SearchMemoryResult {
                memory_id: m.memory_id,
                content: m.content,
                score: 1.0, // Tag match is exact
                method: "tag_search".to_string(),
                metadata: HashMap::new(),
                created_at: m.created_at,
            })
            .collect())
    }

    
    pub async fn update_memory(
        &self,
        memory_id: &str,
        new_content: &str,
        _user_id: &str,
    ) -> Result<bool, ToolingError> {
        info!("Updating memory: {}", memory_id);

        
        let vector = self
            .embedder
            .generate(new_content, true)
            .await
            .map_err(|e| ToolingError::Embedding(e.to_string()))?;

        let now = chrono::Utc::now().to_rfc3339();

        
        #[derive(serde::Deserialize)]
        struct GetMemResult {
            #[serde(default)]
            memory: Option<MemNode>,
        }
        #[derive(serde::Deserialize)]
        struct MemNode {
            #[serde(default)]
            id: String,
        }

        let mem_result: GetMemResult = self.db
            .execute_query("getMemory", &serde_json::json!({"memory_id": memory_id}))
            .await
            .map_err(|e| ToolingError::Database(format!("Failed to get memory: {}", e)))?;

        let internal_id = match mem_result.memory {
            Some(m) if !m.id.is_empty() => m.id,
            _ => return Err(ToolingError::Database(format!("Memory {} not found", memory_id))),
        };

        
        #[derive(Serialize)]
        struct UpdateByIdParams {
            id: String,
            content: String,
            certainty: i64,
            importance: i64,
            updated_at: String,
        }
        
        let params = UpdateByIdParams {
            id: internal_id.clone(),
            content: new_content.to_string(),
            certainty: 80,
            importance: 50,
            updated_at: now.clone(),
        };

        let _result: serde_json::Value = self.db
            .execute_query("updateMemoryById", &params)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        
        debug!("Memory {} (id={}) updated successfully", memory_id, internal_id);

        
        #[derive(serde::Deserialize)]
        struct MemoryResult {
            #[serde(default)]
            memory: Option<MemoryData>,
        }
        #[derive(serde::Deserialize)]
        struct MemoryData {
            #[serde(default)]
            id: String,
        }

        
        if let Ok(result) = self.db.execute_query::<MemoryResult, _>(
            "getMemory",
            &serde_json::json!({"memory_id": memory_id}),
        ).await {
            if let Some(mem) = result.memory {
                if !mem.id.is_empty() {
                    #[derive(serde::Deserialize)]
                    struct EmbeddingResult {
                        #[serde(default)]
                        embedding: serde_json::Value,
                    }

                    let _ = self.db.execute_query::<EmbeddingResult, _>(
                        "addMemoryEmbedding",
                        &serde_json::json!({
                            "memory_id": mem.id,
                            "vector_data": vector.iter().map(|&x| x as f64).collect::<Vec<f64>>(),
                            "embedding_model": self.embedder.model(),
                            "created_at": now,
                        }),
                    ).await;
                }
            }
        }

        Ok(true)
    }

    
    pub async fn delete_memory(&self, memory_id: &str) -> Result<bool, ToolingError> {
        info!("Deleting memory: {}", memory_id);

        #[derive(Serialize)]
        struct DeleteInput {
            memory_id: String,
        }

        self.db
            .execute_query::<(), _>("deleteMemory", &DeleteInput {
                memory_id: memory_id.to_string(),
            })
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        Ok(true)
    }

    
    pub async fn get_memory_graph(
        &self,
        user_id: &str,
        memory_id: Option<&str>,
        depth: usize,
    ) -> Result<(Vec<serde_json::Value>, Vec<serde_json::Value>), ToolingError> {
        info!("Getting memory graph for user={}, memory={:?}, depth={}", user_id, memory_id, depth);

        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut visited = std::collections::HashSet::new();

        
        let start_ids: Vec<String> = if let Some(mid) = memory_id {
            vec![mid.to_string()]
        } else {
            
            #[derive(serde::Deserialize)]
            struct UserMemoriesResult {
                #[serde(default)]
                memories: Vec<MemoryNode>,
            }
            #[derive(serde::Deserialize)]
            struct MemoryNode {
                memory_id: String,
                #[serde(default)]
                content: String,
            }

            match self.db.execute_query::<UserMemoriesResult, _>(
                "getUserMemories",
                &serde_json::json!({"user_id": user_id, "limit": 10i64}),
            ).await {
                Ok(result) => result.memories.into_iter().map(|m| m.memory_id).collect(),
                Err(_) => Vec::new(),
            }
        };

        if start_ids.is_empty() {
            return Ok((nodes, edges));
        }

        
        let mut current_ids = start_ids;
        let mut current_depth = 0;

        while current_depth < depth && !current_ids.is_empty() {
            let mut next_ids = Vec::new();

            for mid in &current_ids {
                if visited.contains(mid) {
                    continue;
                }
                visited.insert(mid.clone());

                
                #[derive(serde::Deserialize)]
                struct MemoryResult {
                    #[serde(default)]
                    memory: Option<MemoryData>,
                }
                #[derive(serde::Deserialize)]
                struct MemoryData {
                    memory_id: String,
                    #[serde(default)]
                    content: String,
                    #[serde(default)]
                    memory_type: String,
                }

                if let Ok(result) = self.db.execute_query::<MemoryResult, _>(
                    "getMemory",
                    &serde_json::json!({"memory_id": mid}),
                ).await {
                    if let Some(mem) = result.memory {
                        nodes.push(serde_json::json!({
                            "id": mem.memory_id,
                            "content": mem.content,
                            "type": mem.memory_type,
                        }));
                    }
                }

                
                #[derive(serde::Deserialize, Default)]
                struct ConnectionsResult {
                    #[serde(default)]
                    implies_out: Vec<ConnectedMemory>,
                    #[serde(default)]
                    implies_in: Vec<ConnectedMemory>,
                    #[serde(default)]
                    because_out: Vec<ConnectedMemory>,
                    #[serde(default)]
                    because_in: Vec<ConnectedMemory>,
                    #[serde(default)]
                    contradicts_out: Vec<ConnectedMemory>,
                    #[serde(default)]
                    contradicts_in: Vec<ConnectedMemory>,
                    #[serde(default)]
                    relation_out: Vec<ConnectedMemory>,
                    #[serde(default)]
                    relation_in: Vec<ConnectedMemory>,
                }
                #[derive(serde::Deserialize)]
                struct ConnectedMemory {
                    memory_id: String,
                    #[serde(default)]
                    content: String,
                }

                if let Ok(conns) = self.db.execute_query::<ConnectionsResult, _>(
                    "getMemoryLogicalConnections",
                    &serde_json::json!({"memory_id": mid}),
                ).await {
                    
                    for conn in conns.implies_out {
                        edges.push(serde_json::json!({
                            "source": mid,
                            "target": conn.memory_id,
                            "type": "IMPLIES",
                            "weight": 1.0,
                        }));
                        next_ids.push(conn.memory_id);
                    }
                    for conn in conns.implies_in {
                        edges.push(serde_json::json!({
                            "source": conn.memory_id,
                            "target": mid,
                            "type": "IMPLIES",
                            "weight": 1.0,
                        }));
                        next_ids.push(conn.memory_id);
                    }
                    
                    for conn in conns.because_out {
                        edges.push(serde_json::json!({
                            "source": mid,
                            "target": conn.memory_id,
                            "type": "BECAUSE",
                            "weight": 1.0,
                        }));
                        next_ids.push(conn.memory_id);
                    }
                    for conn in conns.because_in {
                        edges.push(serde_json::json!({
                            "source": conn.memory_id,
                            "target": mid,
                            "type": "BECAUSE",
                            "weight": 1.0,
                        }));
                        next_ids.push(conn.memory_id);
                    }
                    
                    for conn in conns.contradicts_out {
                        edges.push(serde_json::json!({
                            "source": mid,
                            "target": conn.memory_id,
                            "type": "CONTRADICTS",
                            "weight": 1.0,
                        }));
                        next_ids.push(conn.memory_id);
                    }
                    
                    for conn in conns.relation_out {
                        edges.push(serde_json::json!({
                            "source": mid,
                            "target": conn.memory_id,
                            "type": "SUPPORTS",
                            "weight": 1.0,
                        }));
                        next_ids.push(conn.memory_id);
                    }
                }
            }

            current_ids = next_ids;
            current_depth += 1;
        }

        info!("Graph built: {} nodes, {} edges", nodes.len(), edges.len());
        Ok((nodes, edges))
    }

    
    pub async fn search_reasoning_chain(
        &self,
        query: &str,
        user_id: &str,
        chain_mode: &str,
        max_depth: usize,
        limit: usize,
    ) -> Result<ReasoningChainSearchResult, ToolingError> {
        info!("Reasoning chain search: '{}...' mode={} depth={} limit={}", 
            safe_truncate(query, 30), chain_mode, max_depth, limit);

        
        let query_embedding = self
            .embedder
            .generate(query, true)
            .await
            .map_err(|e| ToolingError::Embedding(e.to_string()))?;

        let seed_results = self
            .search_engine
            .search(query, &query_embedding, user_id, limit, "contextual", None)
            .await?;

        if seed_results.is_empty() {
            debug!("No seed memories found for query");
            return Ok(ReasoningChainSearchResult {
                chains: Vec::new(),
                total_memories: 0,
                deepest_chain: 0,
            });
        }

        
        let mut all_chains = Vec::new();
        let mut max_chain_depth = 0;
        let mut total_memories = 0;

        for seed in &seed_results {
            match self.reasoning_engine.get_chain(&seed.memory_id, chain_mode, max_depth).await {
                Ok(chain) => {
                    if !chain.relations.is_empty() {
                        let chain_depth = chain.depth;
                        max_chain_depth = max_chain_depth.max(chain_depth);
                        total_memories += chain.relations.len();

                        all_chains.push(ToolingReasoningChain {
                            seed: SearchMemoryResult {
                                memory_id: seed.memory_id.clone(),
                                content: seed.content.clone(),
                                score: seed.score as f64,
                                method: seed.method.clone(),
                                metadata: seed.metadata.clone(),
                                created_at: seed.created_at.clone(),
                            },
                            nodes: chain.relations.iter().map(|r| ChainNode {
                                memory_id: r.to_memory_id.clone(),
                                content: r.to_memory_content.clone(),
                                relation: r.relation_type.edge_name().to_string(),
                                depth: 0,
                            }).collect(),
                            chain_type: chain.chain_type.clone(),
                            reasoning_trail: chain.reasoning_trail.clone(),
                        });
                    }
                }
                Err(e) => {
                    debug!("Failed to get chain for {}: {}", seed.memory_id, e);
                }
            }
        }

        info!("Found {} chains, max_depth={}, total_memories={}", 
            all_chains.len(), max_chain_depth, total_memories);

        Ok(ReasoningChainSearchResult {
            chains: all_chains,
            total_memories,
            deepest_chain: max_chain_depth,
        })
    }

    
    pub async fn search_by_concept(
        &self,
        query: &str,
        user_id: &str,
        concept_type: Option<&str>,
        tags: Option<&str>,
        mode: &str,
        limit: usize,
    ) -> Result<Vec<SearchMemoryResult>, ToolingError> {
        info!("Concept search: '{}...' type={:?} tags={:?}", 
            safe_truncate(query, 30), concept_type, tags);

        
        let query_embedding = self
            .embedder
            .generate(query, true)
            .await
            .map_err(|e| ToolingError::Embedding(e.to_string()))?;

        let candidates = self
            .search_engine
            .search(query, &query_embedding, user_id, limit * 3, mode, None)
            .await?;

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        
        let mut results = Vec::new();
        
        for candidate in candidates {
            
            #[derive(serde::Deserialize)]
            struct ConceptsResult {
                #[serde(default)]
                instance_of: Vec<ConceptNode>,
                #[serde(default)]
                belongs_to: Vec<ConceptNode>,
            }
            
            #[derive(serde::Deserialize)]
            struct ConceptNode {
                #[serde(default)]
                concept_id: String,
                #[serde(default)]
                name: String,
            }

            if let Ok(concepts) = self.db
                .execute_query::<ConceptsResult, _>(
                    "getMemoryConcepts",
                    &serde_json::json!({"memory_id": candidate.memory_id}),
                )
                .await
            {
                
                let matches_type = match concept_type {
                    Some(ct) => {
                        let has_db_link = concepts.instance_of.iter().any(|c| 
                            c.name.to_lowercase() == ct.to_lowercase() ||
                            c.concept_id.to_lowercase().contains(&ct.to_lowercase())
                        );
                        
                        if has_db_link {
                            true
                        } else {
                            let ontology = self.ontology_manager.read();
                            if ontology.is_loaded() {
                                let mapped = ontology.map_memory_to_concepts(&candidate.content, None);
                                mapped.iter().any(|m| 
                                    m.concept.name.to_lowercase() == ct.to_lowercase() ||
                                    m.concept.id.to_lowercase() == ct.to_lowercase()
                                )
                            } else {
                                false
                            }
                        }
                    }
                    None => true,
                };

                
                let matches_tags = match tags {
                    Some(t) => {
                        let tag_list: Vec<&str> = t.split(',').map(|s| s.trim()).collect();
                        tag_list.iter().any(|tag| 
                            candidate.content.to_lowercase().contains(&tag.to_lowercase())
                        )
                    }
                    None => true,
                };

                if matches_type && matches_tags {
                    results.push(SearchMemoryResult {
                        memory_id: candidate.memory_id,
                        content: candidate.content,
                        score: candidate.score as f64,
                        method: format!("concept_search_{}", mode),
                        metadata: candidate.metadata,
                        created_at: candidate.created_at,
                    });

                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }

        info!("Concept search found {} results", results.len());
        Ok(results)
    }
}
