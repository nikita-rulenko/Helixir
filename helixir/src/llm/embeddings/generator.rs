//! [`EmbeddingGenerator`] struct, constructor, accessors.
//!
//! Per-call methods (`generate`, `generate_batch`) live in sibling
//! [`super::single`] and [`super::batch`] modules.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, info};

use super::cache::{CacheNamespace, EmbeddingCache, EmbeddingCacheDiagnostics};
use super::config::{DEFAULT_FALLBACK_MODEL, DEFAULT_FALLBACK_URL, EmbeddingConfig};

pub struct EmbeddingGenerator {
    pub(super) provider: String,
    pub(super) base_url: String,
    pub(super) model: String,
    pub(super) api_key: Option<String>,
    pub(super) client: Client,
    pub(super) cache: EmbeddingCache,
    pub(super) primary_cache_namespace: CacheNamespace,
    pub(super) fallback_cache_namespace: CacheNamespace,

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

        let persistent_cache_enabled =
            std::env::var("HELIXIR_EMBED_CACHE_PATH").is_ok_and(|path| !path.trim().is_empty());
        let cache_epoch = env_string("HELIXIR_EMBED_CACHE_EPOCH");
        let primary_endpoint = match provider.as_str() {
            "ollama" if base_url.is_empty() => DEFAULT_FALLBACK_URL,
            "openai" if base_url.is_empty() => "https://api.openai.com/v1",
            _ => &base_url,
        };
        let primary_revision = resolve_cache_revision(
            &env_string("HELIXIR_EMBED_MODEL_REVISION"),
            &provider,
            primary_endpoint,
            &model,
            persistent_cache_enabled,
        );
        let fallback_revision = resolve_cache_revision(
            &env_string("HELIXIR_EMBED_FALLBACK_MODEL_REVISION"),
            "ollama",
            &fallback_url,
            &fallback_model,
            persistent_cache_enabled && config.fallback_enabled && provider != "ollama",
        );
        let primary_cache_namespace = CacheNamespace::new(
            &provider,
            primary_endpoint,
            &model,
            &primary_revision,
            env_dimension("HELIXIR_EMBED_DIMENSION"),
            &cache_epoch,
        );
        let fallback_cache_namespace = CacheNamespace::new(
            "ollama",
            &fallback_url,
            &fallback_model,
            &fallback_revision,
            env_dimension("HELIXIR_EMBED_FALLBACK_DIMENSION"),
            &cache_epoch,
        );

        // algo-opt R2: opt-in persistent embedding cache. Namespace identity
        // prevents primary/fallback or provider/endpoint/model drift from
        // reusing vectors from another embedding space.
        let cache = match std::env::var("HELIXIR_EMBED_CACHE_PATH") {
            Ok(path) if !path.trim().is_empty() => EmbeddingCache::with_persistence(
                config.cache_size,
                config.cache_ttl,
                std::path::Path::new(path.trim()),
                &[
                    primary_cache_namespace.clone(),
                    fallback_cache_namespace.clone(),
                ],
            ),
            _ => EmbeddingCache::new(config.cache_size, config.cache_ttl),
        };

        Self {
            provider,
            base_url,
            model,
            api_key: config.api_key,
            client: Client::builder()
                .timeout(Duration::from_secs(config.timeout_secs))
                .build()
                .expect("Failed to create HTTP client"),
            cache,
            primary_cache_namespace,
            fallback_cache_namespace,
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

    /// Return bounded cache counters without exposing cached text or vectors.
    pub fn cache_diagnostics(&self) -> EmbeddingCacheDiagnostics {
        self.cache.diagnostics()
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
    pub(super) fn primary_url_for_test(
        &self,
        ollama_default: &str,
        openai_default: &str,
    ) -> String {
        self.primary_url(ollama_default, openai_default)
    }
}

fn env_string(name: &str) -> String {
    std::env::var(name).unwrap_or_default().trim().to_string()
}

fn env_dimension(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
}

#[derive(Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaTag>,
}

#[derive(Deserialize)]
struct OllamaTag {
    model: Option<String>,
    name: Option<String>,
    digest: Option<String>,
}

fn resolve_cache_revision(
    configured: &str,
    provider: &str,
    endpoint: &str,
    model: &str,
    detect: bool,
) -> String {
    if !configured.is_empty() || !detect || provider != "ollama" {
        return configured.to_string();
    }
    let endpoint = endpoint.trim_end_matches('/').to_string();
    let model = model.to_string();
    let log_model = model.clone();
    let detected = std::thread::spawn(move || detect_ollama_revision(&endpoint, &model))
        .join()
        .ok()
        .flatten();
    if let Some(revision) = detected {
        debug!(
            model = log_model,
            revision, "Detected Ollama model digest for cache namespace"
        );
        revision
    } else {
        String::new()
    }
}

fn detect_ollama_revision(endpoint: &str, requested: &str) -> Option<String> {
    let response = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_millis(500))
        .timeout(Duration::from_millis(750))
        .build()
        .ok()?
        .get(format!("{}/api/tags", endpoint.trim_end_matches('/')))
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .json::<OllamaTagsResponse>()
        .ok()?;
    response.models.into_iter().find_map(|entry| {
        let name = entry.model.or(entry.name)?;
        let matches = name == requested
            || (!requested.contains(':') && name == format!("{requested}:latest"));
        matches
            .then_some(entry.digest)
            .flatten()
            .filter(|digest| !digest.is_empty())
    })
}

#[cfg(test)]
mod revision_tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use super::detect_ollama_revision;

    #[test]
    fn ollama_digest_invalidates_an_implicit_latest_alias() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let read = stream.read(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request[..read]).starts_with("GET /api/tags "));
            let body = r#"{"models":[{"name":"nomic-embed-text:latest","digest":"sha256:new"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        assert_eq!(
            detect_ollama_revision(&endpoint, "nomic-embed-text").as_deref(),
            Some("sha256:new")
        );
        server.join().unwrap();
    }
}
