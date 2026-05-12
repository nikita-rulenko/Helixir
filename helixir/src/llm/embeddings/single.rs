//! Per-text embedding path: cache lookup → primary provider → Ollama fallback.

use std::sync::atomic::Ordering;

use tracing::{debug, info};

use super::error::EmbeddingError;
use super::generator::EmbeddingGenerator;
use super::wire::{OllamaEmbeddingRequest, OllamaEmbeddingResponse};
use super::wire::{OpenAIEmbeddingRequest, OpenAIEmbeddingResponse};

impl EmbeddingGenerator {
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
                debug!(
                    "Primary embedding provider unavailable, trying fallback: {}",
                    e
                );
                if self.fallback_enabled && self.provider != "ollama" {
                    self.fallback_to_ollama(text, use_cache, &e).await
                } else {
                    Err(e)
                }
            }
        }
    }

    pub(super) async fn generate_ollama(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let base = self.primary_url(super::config::DEFAULT_FALLBACK_URL, "");
        let request = OllamaEmbeddingRequest {
            model: self.model.clone(),
            prompt: text.to_string(),
        };

        let response = self
            .client
            .post(format!("{base}/api/embeddings"))
            .json(&request)
            .send()
            .await?
            .error_for_status()
            .map_err(EmbeddingError::Http)?
            .json::<OllamaEmbeddingResponse>()
            .await?;

        Ok(response.embedding)
    }

    pub(super) async fn generate_openai(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| EmbeddingError::InvalidResponse("API key required".to_string()))?;

        let api_url = self.primary_url("", "https://api.openai.com/v1");

        let request = OpenAIEmbeddingRequest {
            model: self.model.clone(),
            input: text.to_string(),
        };

        let response = self
            .client
            .post(format!("{api_url}/embeddings"))
            .header("Authorization", format!("Bearer {api_key}"))
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
            .map_err(|e| EmbeddingError::BothFailed(original_error.to_string(), e.to_string()))?
            .error_for_status()
            .map_err(|e| EmbeddingError::BothFailed(original_error.to_string(), e.to_string()))?
            .json::<OllamaEmbeddingResponse>()
            .await
            .map_err(|e| EmbeddingError::BothFailed(original_error.to_string(), e.to_string()))?;

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
}
