

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::core::config::HelixirConfig;
use crate::db::HelixClient;
use crate::llm::EmbeddingGenerator;
use crate::llm::providers::base::LlmProvider;
use crate::llm::factory::LlmProviderFactory;
use crate::toolkit::tooling_manager::ToolingManager;


#[derive(Debug, thiserror::Error)]
pub enum HelixirClientError {
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Database error: {0}")]
    Database(String),
    #[error("LLM error: {0}")]
    Llm(String),
    #[error("Embedding error: {0}")]
    Embedding(String),
    #[error("Tooling error: {0}")]
    Tooling(String),
    #[error("Client not initialized")]
    NotInitialized,
    #[error("Operation failed: {0}")]
    Operation(String),
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddMemoryResult {
    pub memories_added: usize,
    pub memory_ids: Vec<String>,
    pub chunks_created: usize,
    pub stats: HashMap<String, serde_json::Value>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub content: String,
    pub score: f32,
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: String,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateResult {
    pub memory_id: String,
    pub updated: bool,
    pub new_content: String,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphResult {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningChainResult {
    pub query: String,
    pub chains: Vec<ReasoningChain>,
    pub total_memories: usize,
    pub deepest_chain: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningChain {
    pub seed: SearchResult,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub content: String,
    pub node_type: String,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub edge_type: String,
    pub weight: f32,
}


pub struct HelixirClient {
    config: HelixirConfig,
    db: Arc<HelixClient>,
    embedder: Arc<EmbeddingGenerator>,
    llm_provider: Arc<dyn LlmProvider>,
    tooling_manager: ToolingManager,
    is_initialized: Arc<AtomicBool>,
}

impl HelixirClient {
    
    pub fn new(config: HelixirConfig) -> Result<Self, HelixirClientError> {
        
        let db = Arc::new(HelixClient::new(&config.host, config.port)
            .map_err(|e| HelixirClientError::Database(e.to_string()))?);

        
        let is_openai_compat = config.embedding_provider == "openai";
        let embedder = Arc::new(EmbeddingGenerator::new(
            config.embedding_provider.clone(),
            if is_openai_compat { "http://localhost:11434".to_string() } else { config.embedding_url.clone() },
            config.embedding_model.clone(),
            config.embedding_api_key.clone(),
            if is_openai_compat { Some(config.embedding_url.clone()) } else { None },
            config.timeout,
            1000,
            300,
            config.embedding_fallback_enabled,
            Some(config.embedding_fallback_url.clone()),
            Some(config.embedding_fallback_model.clone()),
        ));

        
        let llm_provider: Arc<dyn LlmProvider> = LlmProviderFactory::create(
            &config.llm_provider,
            &config.llm_model,
            config.llm_api_key.as_deref(),
            config.llm_base_url.as_deref(),
            f64::from(config.llm_temperature),
        ).into();

        
        let tooling_manager = ToolingManager::new(
            Arc::clone(&db),
            Arc::clone(&embedder),
            Arc::clone(&llm_provider),
        );

        info!("HelixirClient created with ToolingManager");

        Ok(Self {
            config,
            db,
            embedder,
            llm_provider,
            tooling_manager,
            is_initialized: Arc::new(AtomicBool::new(false)),
        })
    }

    
    pub fn from_env() -> Result<Self, HelixirClientError> {
        let config = HelixirConfig::from_env();
        Self::new(config)
    }

    
    pub async fn initialize(&self) -> Result<(), HelixirClientError> {
        if self.is_initialized.load(Ordering::Relaxed) {
            return Ok(());
        }

        
        self.db.health_check().await
            .map_err(|e| HelixirClientError::Database(e.to_string()))?;

        
        self.tooling_manager.initialize().await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        self.is_initialized.store(true, Ordering::Relaxed);
        Ok(())
    }

    
    pub async fn add(
        &self,
        message: &str,
        user_id: &str,
        agent_id: Option<&str>,
        metadata: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<AddMemoryResult, HelixirClientError> {
        self.add_with_tags(message, user_id, agent_id, metadata, None).await
    }

    /// Add memory with optional context tags that are inherited by all extracted facts
    pub async fn add_with_tags(
        &self,
        message: &str,
        user_id: &str,
        agent_id: Option<&str>,
        metadata: Option<HashMap<String, serde_json::Value>>,
        context_tags: Option<&str>,
    ) -> Result<AddMemoryResult, HelixirClientError> {
        self.ensure_initialized().await?;

        let result = self.tooling_manager
            .add_memory(message, user_id, agent_id, metadata, context_tags)
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        Ok(AddMemoryResult {
            memories_added: result.added.len(),
            memory_ids: result.added,
            chunks_created: result.chunks_created,
            stats: result.metadata,
        })
    }

    
    pub async fn search(
        &self,
        query: &str,
        user_id: &str,
        limit: Option<usize>,
        search_mode: Option<&str>,
        temporal_days: Option<f64>,
        graph_depth: Option<usize>,
    ) -> Result<Vec<SearchResult>, HelixirClientError> {
        self.ensure_initialized().await?;

        let mode = search_mode.unwrap_or(&self.config.default_search_mode);
        let results = self.tooling_manager
            .search_memory(query, user_id, limit, mode, temporal_days, graph_depth)
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        Ok(results
            .into_iter()
            .map(|r| SearchResult {
                id: r.memory_id,
                content: r.content,
                score: r.score as f32,
                metadata: r.metadata,
                created_at: r.created_at,
            })
            .collect())
    }

    
    pub async fn update(
        &self,
        memory_id: &str,
        new_content: &str,
        user_id: &str,
    ) -> Result<UpdateResult, HelixirClientError> {
        self.ensure_initialized().await?;

        let updated = self.tooling_manager
            .update_memory(memory_id, new_content, user_id)
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        Ok(UpdateResult {
            memory_id: memory_id.to_string(),
            updated,
            new_content: new_content.to_string(),
        })
    }

    
    pub async fn delete(&self, memory_id: &str) -> Result<bool, HelixirClientError> {
        self.ensure_initialized().await?;

        self.tooling_manager
            .delete_memory(memory_id)
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))
    }

    
    pub async fn get_graph(
        &self,
        user_id: &str,
        memory_id: Option<&str>,
        depth: Option<usize>,
    ) -> Result<GraphResult, HelixirClientError> {
        self.ensure_initialized().await?;

        let (nodes, edges) = self.tooling_manager
            .get_memory_graph(user_id, memory_id, depth.unwrap_or(2))
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        
        Ok(GraphResult {
            nodes: nodes.into_iter().map(|n| GraphNode {
                id: n.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                content: n.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                node_type: n.get("type").and_then(|v| v.as_str()).unwrap_or("memory").to_string(),
                metadata: HashMap::new(),
            }).collect(),
            edges: edges.into_iter().map(|e| GraphEdge {
                source: e.get("source").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                target: e.get("target").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                edge_type: e.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                weight: e.get("weight").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
            }).collect(),
        })
    }

    
    pub async fn search_by_concept(
        &self,
        query: &str,
        user_id: &str,
        concept_type: Option<&str>,
        tags: Option<&str>,
        mode: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<SearchResult>, HelixirClientError> {
        self.ensure_initialized().await?;

        let results = self.tooling_manager
            .search_by_concept(query, user_id, concept_type, tags, mode.unwrap_or("contextual"), limit.unwrap_or(10))
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        
        Ok(results.into_iter().map(|r| SearchResult {
            id: r.memory_id,
            content: r.content,
            score: r.score as f32,
            metadata: r.metadata,
            created_at: r.created_at,
        }).collect())
    }

    
    pub async fn search_reasoning_chain(
        &self,
        query: &str,
        user_id: &str,
        chain_mode: Option<&str>,
        max_depth: Option<usize>,
        limit: Option<usize>,
    ) -> Result<ReasoningChainResult, HelixirClientError> {
        self.ensure_initialized().await?;

        let result = self.tooling_manager
            .search_reasoning_chain(query, user_id, chain_mode.unwrap_or("both"), max_depth.unwrap_or(5), limit.unwrap_or(5))
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        
        let chains = result.chains.into_iter().map(|tc| ReasoningChain {
            seed: SearchResult {
                id: tc.seed.memory_id,
                content: tc.seed.content,
                score: tc.seed.score as f32,
                metadata: tc.seed.metadata,
                created_at: tc.seed.created_at,
            },
            nodes: tc.nodes.into_iter().map(|n| ChainNode {
                memory_id: n.memory_id,
                content: n.content,
                relation: n.relation,
                depth: n.depth,
            }).collect(),
            chain_type: tc.chain_type,
            reasoning_trail: tc.reasoning_trail,
        }).collect();

        Ok(ReasoningChainResult {
            query: query.to_string(),
            chains,
            total_memories: result.total_memories,
            deepest_chain: result.deepest_chain,
        })
    }

    
    pub async fn close(&self) -> Result<(), HelixirClientError> {
        if !self.is_initialized.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.is_initialized.store(false, Ordering::Relaxed);
        Ok(())
    }

    
    async fn ensure_initialized(&self) -> Result<(), HelixirClientError> {
        if !self.is_initialized.load(Ordering::Relaxed) {
            self.initialize().await?;
        }
        Ok(())
    }

    
    pub fn config(&self) -> &HelixirConfig {
        &self.config
    }

    
    pub fn db(&self) -> &HelixClient {
        &self.db
    }

    
    pub fn embedder(&self) -> &EmbeddingGenerator {
        &self.embedder
    }

    
    pub fn llm_provider(&self) -> &dyn LlmProvider {
        &*self.llm_provider
    }

    
    pub fn tooling(&self) -> &ToolingManager {
        &self.tooling_manager
    }
}

impl Drop for HelixirClient {
    fn drop(&mut self) {
        if self.is_initialized.load(Ordering::Relaxed) {
            self.is_initialized.store(false, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let config = HelixirConfig::default();
        let client = HelixirClient::new(config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_client_from_env() {
        std::env::set_var("HELIX_HOST", "localhost");
        std::env::set_var("HELIX_PORT", "6969");
        let client = HelixirClient::from_env();
        assert!(client.is_ok());
    }

    #[test]
    fn test_config_access() {
        let config = HelixirConfig::default();
        let client = HelixirClient::new(config).unwrap();
        
        assert_eq!(client.config().host, "localhost");
        assert_eq!(client.config().port, 6969);
    }
}
