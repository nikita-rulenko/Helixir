

use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelixirConfig {
    
    pub host: String,
    pub port: u16,
    pub instance: String,
    pub api_key: Option<String>,
    pub timeout: u64,
    pub max_retries: u32,

    
    pub llm_provider: String,
    pub llm_model: String,
    pub llm_api_key: Option<String>,
    pub llm_base_url: Option<String>,
    pub llm_temperature: f32,

    
    pub llm_fallback_enabled: bool,
    pub llm_fallback_url: String,
    pub llm_fallback_model: String,

    
    pub embedding_provider: String,
    pub embedding_model: String,
    pub embedding_url: String,
    pub embedding_api_key: Option<String>,

    
    pub embedding_fallback_enabled: bool,
    pub embedding_fallback_url: String,
    pub embedding_fallback_model: String,

    
    pub default_certainty: u8,
    pub default_importance: u8,

    
    pub default_search_limit: usize,
    pub default_search_mode: String,
    pub vector_search_enabled: bool,
    pub graph_search_enabled: bool,
    pub bm25_search_enabled: bool,
}

impl HelixirConfig {
    
    pub fn new(host: &str, port: u16) -> Self {
        Self {
            host: host.to_string(),
            port,
            instance: "dev".to_string(),
            api_key: None,
            timeout: 30,
            max_retries: 3,

            llm_provider: "cerebras".to_string(),
            llm_model: "llama-3.3-70b".to_string(),
            llm_api_key: None,
            llm_base_url: None,
            llm_temperature: 0.3,

            llm_fallback_enabled: true,
            llm_fallback_url: "http://localhost:11434".to_string(),
            llm_fallback_model: "llama3.2".to_string(),

            embedding_provider: "ollama".to_string(),
            embedding_model: "nomic-embed-text".to_string(),
            embedding_url: "http://localhost:11434".to_string(),
            embedding_api_key: None,

            embedding_fallback_enabled: true,
            embedding_fallback_url: "http://localhost:11434".to_string(),
            embedding_fallback_model: "nomic-embed-text".to_string(),

            default_certainty: 80,
            default_importance: 50,

            default_search_limit: 10,
            default_search_mode: "recent".to_string(),
            vector_search_enabled: true,
            graph_search_enabled: true,
            bm25_search_enabled: true,
        }
    }

    
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    
    pub fn from_env() -> Self {
        let mut config = Self::new(
            &std::env::var("HELIX_HOST").unwrap_or_else(|_| "localhost".to_string()),
            std::env::var("HELIX_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(6969),
        );

        if let Ok(instance) = std::env::var("HELIX_INSTANCE") {
            config.instance = instance;
        }
        if let Ok(provider) = std::env::var("HELIX_LLM_PROVIDER") {
            config.llm_provider = provider;
        }
        if let Ok(model) = std::env::var("HELIX_LLM_MODEL") {
            config.llm_model = model;
        }
        if let Ok(key) = std::env::var("HELIX_LLM_API_KEY") {
            config.llm_api_key = Some(key);
        }
        if let Ok(provider) = std::env::var("HELIX_EMBEDDING_PROVIDER") {
            config.embedding_provider = provider;
        }
        if let Ok(model) = std::env::var("HELIX_EMBEDDING_MODEL") {
            config.embedding_model = model;
        }
        if let Ok(url) = std::env::var("HELIX_EMBEDDING_URL") {
            config.embedding_url = url;
        }
        if let Ok(key) = std::env::var("HELIX_EMBEDDING_API_KEY") {
            config.embedding_api_key = Some(key);
        }

        config
    }
}

impl Default for HelixirConfig {
    fn default() -> Self {
        Self::new("localhost", 6969)
    }
}

