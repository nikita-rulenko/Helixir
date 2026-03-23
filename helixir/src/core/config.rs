

use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchThresholds {
    pub similarity_threshold: f64,
    pub exact_duplicate_score: f64,
    pub min_vector_score: f64,
    pub min_combined_score: f64,
    pub vector_weight: f64,
    pub temporal_weight: f64,
    pub graph_semantic_weight: f64,
    pub graph_graph_weight: f64,
    pub graph_temporal_weight: f64,
    pub default_temporal_days: f64,
    pub bm25_k1: f64,
    pub bm25_b: f64,
}

impl Default for SearchThresholds {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.70,
            exact_duplicate_score: 0.98,
            min_vector_score: 0.5,
            min_combined_score: 0.3,
            vector_weight: 0.7,
            temporal_weight: 0.3,
            graph_semantic_weight: 0.3,
            graph_graph_weight: 0.5,
            graph_temporal_weight: 0.2,
            default_temporal_days: 30.0,
            bm25_k1: 1.5,
            bm25_b: 0.75,
        }
    }
}


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

    pub search_thresholds: SearchThresholds,

    pub max_facts_per_call: usize,
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

            search_thresholds: SearchThresholds::default(),

            max_facts_per_call: 15,
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
        if let Ok(val) = std::env::var("HELIX_MAX_FACTS_PER_CALL") {
            if let Ok(n) = val.parse::<usize>() {
                config.max_facts_per_call = n;
            }
        }

        config
    }
}

impl Default for HelixirConfig {
    fn default() -> Self {
        Self::new("localhost", 6969)
    }
}

