//! [`EmbeddingGenerator`] struct, constructor, accessors.
//!
//! Per-call methods (`generate`, `generate_batch`) live in sibling
//! [`super::single`] and [`super::batch`] modules.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use reqwest::Client;
use tracing::info;

use super::cache::EmbeddingCache;
use super::config::{DEFAULT_FALLBACK_MODEL, DEFAULT_FALLBACK_URL, EmbeddingConfig};

pub struct EmbeddingGenerator {
    pub(super) provider: String,
    pub(super) base_url: String,
    pub(super) model: String,
    pub(super) api_key: Option<String>,
    pub(super) client: Client,
    pub(super) cache: EmbeddingCache,

    pub(super) fallback_enabled: bool,
    pub(super) fallback_url: String,
    pub(super) fallback_model: String,
    pub(super) using_fallback: AtomicBool,
    pub(super) fallback_count: AtomicUsize,
}

impl EmbeddingGenerator {
    pub fn new(config: EmbeddingConfig) -> Self {
        let provider = config.provider.to_lowercase();
        let model = config.model;
        let base_url = config.base_url.trim_end_matches('/').to_string();
        let fallback_url = if config.fallback_url.is_empty() {
            DEFAULT_FALLBACK_URL.to_string()
        } else {
            config.fallback_url
        };
        let fallback_model = if config.fallback_model.is_empty() {
            DEFAULT_FALLBACK_MODEL.to_string()
        } else {
            config.fallback_model
        };

        info!(
            "EmbeddingGenerator initialized: provider={}, model={}, base_url={}, cache={}",
            provider, model, base_url, config.cache_size
        );

        Self {
            provider,
            base_url,
            model,
            api_key: config.api_key,
            client: Client::builder()
                .timeout(Duration::from_secs(config.timeout_secs))
                .build()
                .expect("Failed to create HTTP client"),
            cache: EmbeddingCache::new(config.cache_size, config.cache_ttl),
            fallback_enabled: config.fallback_enabled,
            fallback_url,
            fallback_model,
            using_fallback: AtomicBool::new(false),
            fallback_count: AtomicUsize::new(0),
        }
    }

    /// Endpoint the primary provider posts to. Falls back to a provider-specific
    /// default only if `base_url` was passed empty. Returns an owned `String`
    /// so call sites can `format!()` it directly; allocation is negligible
    /// next to an HTTP round-trip.
    pub(super) fn primary_url(&self, ollama_default: &str, openai_default: &str) -> String {
        if !self.base_url.is_empty() {
            return self.base_url.clone();
        }
        match self.provider.as_str() {
            "ollama" => ollama_default.to_string(),
            "openai" => openai_default.to_string(),
            _ => String::new(),
        }
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

    /// Endpoint actually used for primary requests. Exposed for tests + diagnostics.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    #[cfg(test)]
    pub(super) fn primary_url_for_test(&self, ollama_default: &str, openai_default: &str) -> String {
        self.primary_url(ollama_default, openai_default)
    }
}
