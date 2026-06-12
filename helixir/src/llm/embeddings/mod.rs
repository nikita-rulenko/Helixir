//! Embedding generation: cache-fronted HTTP client over Ollama / OpenAI-compatible
//! providers, with optional Ollama fallback when the primary endpoint fails.
//!
//! Layout:
//! - [`config`]    — [`EmbeddingConfig`] + provider defaults.
//! - [`error`]     — [`EmbeddingError`].
//! - [`wire`]      — request/response DTOs for Ollama and OpenAI batch/single.
//! - [`cache`]     — in-process LRU+TTL embedding cache.
//! - [`generator`] — [`EmbeddingGenerator`] struct, constructor, accessors.
//! - [`single`]    — `generate` (one text) + provider routing + single-shot fallback.
//! - [`batch`]     — `generate_batch` (many texts) + batched provider routing
//!   + per-item fallback fan-out.

mod batch;
mod cache;
mod config;
mod error;
mod generator;
mod single;
mod wire;

pub use config::EmbeddingConfig;
pub use error::EmbeddingError;
pub use generator::EmbeddingGenerator;

#[cfg(test)]
mod tests {
    use super::*;

    fn ollama_cfg() -> EmbeddingConfig {
        EmbeddingConfig {
            provider: "ollama".into(),
            base_url: "http://localhost:11434".into(),
            model: "nomic-embed-text".into(),
            api_key: None,
            timeout_secs: 5,
            cache_size: 16,
            cache_ttl: 60,
            fallback_enabled: false,
            fallback_url: String::new(),
            fallback_model: String::new(),
        }
    }

    fn openai_cfg() -> EmbeddingConfig {
        EmbeddingConfig {
            provider: "openai".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            model: "nomic-embed-text-v1.5".into(),
            api_key: Some("test-key".into()),
            timeout_secs: 5,
            cache_size: 16,
            cache_ttl: 60,
            fallback_enabled: true,
            fallback_url: "http://localhost:11434".into(),
            fallback_model: "nomic-embed-text".into(),
        }
    }

    #[test]
    fn config_routes_ollama_url_to_base_url() {
        let generator = EmbeddingGenerator::new(ollama_cfg());
        assert_eq!(generator.provider(), "ollama");
        assert_eq!(generator.model(), "nomic-embed-text");
        assert_eq!(generator.base_url(), "http://localhost:11434");
    }

    #[test]
    fn config_routes_openai_compat_url_to_base_url() {
        let generator = EmbeddingGenerator::new(openai_cfg());
        assert_eq!(generator.provider(), "openai");
        assert_eq!(generator.base_url(), "https://openrouter.ai/api/v1");
    }

    #[test]
    fn trailing_slash_is_normalized() {
        let mut cfg = openai_cfg();
        cfg.base_url = "https://openrouter.ai/api/v1/".into();
        let generator = EmbeddingGenerator::new(cfg);
        assert_eq!(generator.base_url(), "https://openrouter.ai/api/v1");
    }

    #[test]
    fn empty_base_url_falls_back_to_provider_default() {
        let mut cfg = openai_cfg();
        cfg.base_url = String::new();
        let generator = EmbeddingGenerator::new(cfg);
        assert_eq!(
            generator.primary_url_for_test("", "https://api.openai.com/v1"),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn provider_name_is_lowercased() {
        let mut cfg = ollama_cfg();
        cfg.provider = "OLLAMA".into();
        let generator = EmbeddingGenerator::new(cfg);
        assert_eq!(generator.provider(), "ollama");
    }
}
