//! Batched embedding path: cache lookup per item → batched primary call →
//! per-item Ollama fallback.

use std::sync::atomic::Ordering;

use tracing::{debug, info};

use super::error::EmbeddingError;
use super::generator::EmbeddingGenerator;
use super::wire::{
    OllamaEmbeddingRequest, OllamaEmbeddingResponse, OpenAIBatchEmbeddingRequest,
    OpenAIEmbeddingResponse,
};

impl EmbeddingGenerator {
    pub async fn generate_batch(
        &self,
        texts: &[&str],
        use_cache: bool,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        if texts.len() == 1 {
            return Ok(vec![self.generate(texts[0], use_cache).await?]);
        }

        let mut results: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        let mut uncached_indices = Vec::new();
        let mut uncached_texts = Vec::new();

        if use_cache {
            for (i, text) in texts.iter().enumerate() {
                if let Some(cached) = self.cache.get(text) {
                    debug!("Batch cache HIT for: {}...", crate::safe_truncate(text, 50));
                    results[i] = Some(cached);
                } else {
                    uncached_indices.push(i);
                    uncached_texts.push(text.to_string());
                }
            }
        } else {
            for (i, text) in texts.iter().enumerate() {
                uncached_indices.push(i);
                uncached_texts.push(text.to_string());
            }
        }

        if uncached_texts.is_empty() {
            return Ok(results.into_iter().map(|r| r.unwrap()).collect());
        }

        info!(
            "Batch embedding: {} total, {} cached, {} to generate",
            texts.len(),
            texts.len() - uncached_texts.len(),
            uncached_texts.len()
        );

        let embeddings_result = match self.provider.as_str() {
            "openai" => self.generate_batch_openai(&uncached_texts).await,
            "ollama" => self.generate_batch_ollama(&uncached_texts).await,
            other => return Err(EmbeddingError::NotImplemented(other.to_string())),
        };

        let embeddings = match embeddings_result {
            Ok(embs) => {
                self.using_fallback.store(false, Ordering::SeqCst);
                embs
            }
            Err(e) => {
                debug!("Batch primary embedding failed, trying fallback: {}", e);
                if self.fallback_enabled && self.provider != "ollama" {
                    self.fallback_batch_to_ollama(&uncached_texts, &e).await?
                } else {
                    return Err(e);
                }
            }
        };

        for (idx, embedding) in uncached_indices.into_iter().zip(embeddings) {
            if use_cache {
                self.cache.set(texts[idx], embedding.clone());
            }
            results[idx] = Some(embedding);
        }

        Ok(results.into_iter().map(|r| r.unwrap()).collect())
    }

    async fn generate_batch_openai(
        &self,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| EmbeddingError::InvalidResponse("API key required".to_string()))?;

        let api_url = self.primary_url("", "https://api.openai.com/v1");

        let request = OpenAIBatchEmbeddingRequest {
            model: self.model.clone(),
            input: texts.to_vec(),
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

        if response.data.len() != texts.len() {
            return Err(EmbeddingError::InvalidResponse(format!(
                "Expected {} embeddings, got {}",
                texts.len(),
                response.data.len()
            )));
        }

        let mut sorted = response.data;
        sorted.sort_by_key(|d| d.index);
        Ok(sorted.into_iter().map(|d| d.embedding).collect())
    }

    async fn generate_batch_ollama(
        &self,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        use futures::future::join_all;

        let futures: Vec<_> = texts
            .iter()
            .map(|text| self.generate_ollama(text))
            .collect();
        let results = join_all(futures).await;
        results.into_iter().collect()
    }

    async fn generate_fallback_ollama_single(
        &self,
        text: &str,
    ) -> Result<Vec<f32>, EmbeddingError> {
        let request = OllamaEmbeddingRequest {
            model: self.fallback_model.clone(),
            prompt: text.to_string(),
        };

        let response = self
            .client
            .post(format!("{}/api/embeddings", self.fallback_url))
            .json(&request)
            .send()
            .await?
            .error_for_status()
            .map_err(EmbeddingError::Http)?
            .json::<OllamaEmbeddingResponse>()
            .await?;

        Ok(response.embedding)
    }

    async fn fallback_batch_to_ollama(
        &self,
        texts: &[String],
        original_error: &EmbeddingError,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        use futures::future::join_all;

        info!(
            "Using fallback Ollama ({}/{}) for batch of {} - primary unavailable",
            self.fallback_url,
            self.fallback_model,
            texts.len()
        );

        let futures: Vec<_> = texts
            .iter()
            .map(|text| self.generate_fallback_ollama_single(text))
            .collect();
        let results = join_all(futures).await;

        let mut embeddings = Vec::with_capacity(texts.len());
        for result in results {
            match result {
                Ok(emb) => embeddings.push(emb),
                Err(e) => {
                    return Err(EmbeddingError::BothFailed(
                        original_error.to_string(),
                        e.to_string(),
                    ));
                }
            }
        }

        self.using_fallback.store(true, Ordering::SeqCst);
        self.fallback_count.fetch_add(texts.len(), Ordering::SeqCst);

        info!(
            "Fallback batch successful! count={}, total_fallbacks={}",
            embeddings.len(),
            self.fallback_count.load(Ordering::SeqCst)
        );

        Ok(embeddings)
    }
}
