

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, info, warn};

const DEFAULT_FALLBACK_URL: &str = "http://localhost:11434";
const DEFAULT_FALLBACK_MODEL: &str = "nomic-embed-text";


#[derive(Error, Debug)]
pub enum EmbeddingError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON parsing failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Empty text")]
    EmptyText,

    #[error("Provider not implemented: {0}")]
    NotImplemented(String),

    #[error("Both primary and fallback failed: primary={0}, fallback={1}")]
    BothFailed(String, String),
}


#[derive(Serialize)]
struct OllamaEmbeddingRequest {
    model: String,
    prompt: String,
}

#[derive(Deserialize)]
struct OllamaEmbeddingResponse {
    embedding: Vec<f32>,
}

#[derive(Serialize)]
struct OpenAIEmbeddingRequest {
    model: String,
    input: String,
}

#[derive(Deserialize)]
struct OpenAIEmbeddingResponse {
    data: Vec<OpenAIEmbeddingData>,
}

#[derive(Deserialize)]
struct OpenAIEmbeddingData {
    embedding: Vec<f32>,
}


struct CacheEntry {
    embedding: Vec<f32>,
    created_at: Instant,
}

struct EmbeddingCache {
    cache: RwLock<HashMap<String, CacheEntry>>,
    max_size: usize,
    ttl: Duration,
}

impl EmbeddingCache {
    fn new(max_size: usize, ttl_secs: u64) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            max_size,
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    fn get(&self, text: &str) -> Option<Vec<f32>> {
        let cache = self.cache.read().unwrap();
        if let Some(entry) = cache.get(text) {
            if entry.created_at.elapsed() < self.ttl {
                return Some(entry.embedding.clone());
            }
        }
        None
    }

    fn set(&self, text: &str, embedding: Vec<f32>) {
        let mut cache = self.cache.write().unwrap();
        if cache.len() >= self.max_size {
            
            if let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, v)| v.created_at)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&oldest_key);
            }
        }
        cache.insert(
            text.to_string(),
            CacheEntry {
                embedding,
                created_at: Instant::now(),
            },
        );
    }

    fn clear(&self) {
        self.cache.write().unwrap().clear();
    }

    fn len(&self) -> usize {
        self.cache.read().unwrap().len()
    }
}


pub struct EmbeddingGenerator {
    provider: String,
    ollama_url: String,
    model: String,
    api_key: Option<String>,
    base_url: Option<String>,
    client: Client,
    cache: EmbeddingCache,

    
    fallback_enabled: bool,
    fallback_url: String,
    fallback_model: String,
    using_fallback: AtomicBool,
    fallback_count: AtomicUsize,
}

impl EmbeddingGenerator {
    
    pub fn new(
        provider: impl Into<String>,
        ollama_url: impl Into<String>,
        model: impl Into<String>,
        api_key: Option<String>,
        base_url: Option<String>,
        timeout_secs: u64,
        cache_size: usize,
        cache_ttl: u64,
        fallback_enabled: bool,
        fallback_url: Option<String>,
        fallback_model: Option<String>,
    ) -> Self {
        let provider = provider.into().to_lowercase();
        let model = model.into();
        let ollama_url = ollama_url.into();
        let fallback_url = fallback_url.unwrap_or_else(|| DEFAULT_FALLBACK_URL.to_string());
        let fallback_model = fallback_model.unwrap_or_else(|| DEFAULT_FALLBACK_MODEL.to_string());

        info!(
            "EmbeddingGenerator initialized: provider={}, model={}, cache={}",
            provider, model, cache_size
        );

        Self {
            provider,
            ollama_url,
            model,
            api_key,
            base_url,
            client: Client::builder()
                .timeout(Duration::from_secs(timeout_secs))
                .build()
                .expect("Failed to create HTTP client"),
            cache: EmbeddingCache::new(cache_size, cache_ttl),
            fallback_enabled,
            fallback_url,
            fallback_model,
            using_fallback: AtomicBool::new(false),
            fallback_count: AtomicUsize::new(0),
        }
    }

    
    pub async fn generate(&self, text: &str, use_cache: bool) -> Result<Vec<f32>, EmbeddingError> {
        if text.trim().is_empty() {
            return Err(EmbeddingError::EmptyText);
        }

        
        if use_cache {
            if let Some(cached) = self.cache.get(text) {
                debug!("Cache HIT for: {}...", crate::safe_truncate(text, 50));
                return Ok(cached);
            }
        }

        
        let result = match self.provider.as_str() {
            "ollama" => self.generate_ollama(text).await,
            "openai" => self.generate_openai(text).await,
            other => Err(EmbeddingError::NotImplemented(other.to_string())),
        };

        match result {
            Ok(embedding) => {
                if use_cache {
                    self.cache.set(text, embedding.clone());
                }
                self.using_fallback.store(false, Ordering::SeqCst);
                Ok(embedding)
            }
            Err(e) => {
                debug!("Primary embedding provider unavailable, trying fallback: {}", e);
                if self.fallback_enabled && self.provider != "ollama" {
                    self.fallback_to_ollama(text, use_cache, &e).await
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn generate_ollama(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let request = OllamaEmbeddingRequest {
            model: self.model.clone(),
            prompt: text.to_string(),
        };

        let response = self
            .client
            .post(format!("{}/api/embeddings", self.ollama_url))
            .json(&request)
            .send()
            .await?
            .error_for_status()
            .map_err(EmbeddingError::Http)?
            .json::<OllamaEmbeddingResponse>()
            .await?;

        Ok(response.embedding)
    }

    async fn generate_openai(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| EmbeddingError::InvalidResponse("API key required".to_string()))?;

        let api_url = self
            .base_url
            .as_ref()
            .map(|u| u.trim_end_matches('/').to_string())
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

        let request = OpenAIEmbeddingRequest {
            model: self.model.clone(),
            input: text.to_string(),
        };

        let response = self
            .client
            .post(format!("{}/embeddings", api_url))
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&request)
            .send()
            .await?
            .error_for_status()
            .map_err(EmbeddingError::Http)?
            .json::<OpenAIEmbeddingResponse>()
            .await?;

        response
            .data
            .first()
            .map(|d| d.embedding.clone())
            .ok_or_else(|| EmbeddingError::InvalidResponse("No embedding in response".to_string()))
    }

    async fn fallback_to_ollama(
        &self,
        text: &str,
        use_cache: bool,
        original_error: &EmbeddingError,
    ) -> Result<Vec<f32>, EmbeddingError> {
        info!(
            "Using fallback Ollama ({}/{}) - primary unavailable",
            self.fallback_url, self.fallback_model
        );

        let request = OllamaEmbeddingRequest {
            model: self.fallback_model.clone(),
            prompt: text.to_string(),
        };

        let response = self
            .client
            .post(format!("{}/api/embeddings", self.fallback_url))
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                EmbeddingError::BothFailed(original_error.to_string(), e.to_string())
            })?
            .error_for_status()
            .map_err(|e| {
                EmbeddingError::BothFailed(original_error.to_string(), e.to_string())
            })?
            .json::<OllamaEmbeddingResponse>()
            .await
            .map_err(|e| {
                EmbeddingError::BothFailed(original_error.to_string(), e.to_string())
            })?;

        let embedding = response.embedding;

        if use_cache {
            self.cache.set(text, embedding.clone());
        }

        self.using_fallback.store(true, Ordering::SeqCst);
        self.fallback_count.fetch_add(1, Ordering::SeqCst);

        info!(
            "Fallback successful! dims={}, total_fallbacks={}",
            embedding.len(),
            self.fallback_count.load(Ordering::SeqCst)
        );

        Ok(embedding)
    }

    
    pub fn is_using_fallback(&self) -> bool {
        self.using_fallback.load(Ordering::SeqCst)
    }

    
    pub fn fallback_count(&self) -> usize {
        self.fallback_count.load(Ordering::SeqCst)
    }

    
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    
    pub fn clear_cache(&self) {
        self.cache.clear();
        info!("Embedding cache cleared");
    }

    
    pub fn reset_fallback_state(&self) {
        self.using_fallback.store(false, Ordering::SeqCst);
        info!("Fallback state reset");
    }

    
    pub fn model(&self) -> String {
        self.model.clone()
    }

    
    pub fn provider(&self) -> String {
        self.provider.clone()
    }
}
