use super::providers::base::LlmProvider;
use super::providers::cerebras::CerebrasProvider;
use super::providers::fallback::LlmProviderWithFallback;
use super::providers::ollama::OllamaProvider;
use crate::DEFAULT_OLLAMA_URL;

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
                base_url.unwrap_or(DEFAULT_OLLAMA_URL).to_string(),
                model.to_string(),
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

// Embedding provider construction lives in `HelixirClient::new` using the
// typed `EmbeddingConfig` struct from `crate::llm::embeddings`. A dedicated
// factory here previously duplicated that wiring (with the same
// `is_openai_compat` workaround) but was never called — removed to keep
// embedding-pipeline configuration in exactly one place (issue #7).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_ollama_provider() {
        let provider = LlmProviderFactory::create("ollama", "llama3.1:8b", None, None, 0.7);
        assert_eq!(provider.provider_name(), "ollama");
        assert_eq!(provider.model_name(), "llama3.1:8b");
    }

    #[test]
    fn test_ollama_custom_base_url() {
        let provider = LlmProviderFactory::create(
            "ollama",
            "gemma2:9b",
            None,
            Some("http://192.168.1.100:11434"),
            0.5,
        );
        assert_eq!(provider.provider_name(), "ollama");
        assert_eq!(provider.model_name(), "gemma2:9b");
    }

    #[test]
    fn test_create_cerebras_provider() {
        let provider =
            LlmProviderFactory::create("cerebras", "llama-3.3-70b", Some("test-key"), None, 0.3);
        assert_eq!(provider.provider_name(), "cerebras");
    }

    #[test]
    #[should_panic(expected = "Unknown provider")]
    fn test_unknown_provider_panics() {
        let _ = LlmProviderFactory::create("unknown", "model", None, None, 0.5);
    }
}
