

use super::embeddings::EmbeddingGenerator;
use super::providers::base::LlmProvider;
use super::providers::cerebras::CerebrasProvider;
use super::providers::fallback::LlmProviderWithFallback;
use super::providers::ollama::OllamaProvider;
use crate::core::config::HelixirConfig;
use crate::{DEFAULT_CACHE_SIZE, DEFAULT_CACHE_TTL, DEFAULT_OLLAMA_URL};


pub struct LlmProviderFactory;

impl LlmProviderFactory {
    
    
    #[must_use]
    pub fn create(
        provider: &str,
        model: &str,
        api_key: Option<&str>,
        base_url: Option<&str>,
        temperature: f64,
    ) -> Box<dyn LlmProvider> {
        match provider {
            "cerebras" => Box::new(CerebrasProvider::new(
                api_key.unwrap_or_default().to_string(),
                model.to_string(),
                temperature,
            )),
            "ollama" => Box::new(OllamaProvider::new(
                model.to_string(),
                base_url.unwrap_or(DEFAULT_OLLAMA_URL).to_string(),
                temperature,
            )),
            _ => panic!("Unknown provider: {provider}. Supported: cerebras, ollama"),
        }
    }

    
    #[must_use]
    pub fn create_with_fallback(
        primary: std::sync::Arc<dyn LlmProvider>,
        fallback_enabled: bool,
        fallback_url: Option<&str>,
        fallback_model: &str,
        fallback_temperature: f64,
    ) -> LlmProviderWithFallback {
        LlmProviderWithFallback::new(
            primary,
            fallback_enabled,
            fallback_url.map(String::from),
            Some(fallback_model.to_string()),
            fallback_temperature,
        )
    }
}


pub struct EmbeddingProviderFactory;

impl EmbeddingProviderFactory {
    
    #[must_use]
    pub fn from_config(config: &HelixirConfig) -> EmbeddingGenerator {
        
        
        let is_openai_compat = config.embedding_provider == "openai";
        EmbeddingGenerator::new(
            config.embedding_provider.clone(),
            if is_openai_compat { "http://localhost:11434".to_string() } else { config.embedding_url.clone() },
            config.embedding_model.clone(),
            config.embedding_api_key.clone(),
            if is_openai_compat { Some(config.embedding_url.clone()) } else { None },
            config.timeout,
            DEFAULT_CACHE_SIZE,
            DEFAULT_CACHE_TTL,
            config.embedding_fallback_enabled,
            Some(config.embedding_fallback_url.clone()),
            Some(config.embedding_fallback_model.clone()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_ollama_provider() {
        let provider = LlmProviderFactory::create(
            "ollama",
            "llama3.1:8b",
            None,
            None,
            0.7,
        );
        assert_eq!(provider.provider_name(), "ollama");
    }

    #[test]
    fn test_create_cerebras_provider() {
        let provider = LlmProviderFactory::create(
            "cerebras",
            "llama-3.3-70b",
            Some("test-key"),
            None,
            0.3,
        );
        assert_eq!(provider.provider_name(), "cerebras");
    }

    #[test]
    #[should_panic(expected = "Unknown provider")]
    fn test_unknown_provider_panics() {
        LlmProviderFactory::create("unknown", "model", None, None, 0.5);
    }
}
