use super::providers::base::LlmProvider;
use super::providers::fallback::LlmProviderWithFallback;
use super::providers::ollama::OllamaProvider;
use super::providers::openai_compat::OpenAiCompatProvider;
use crate::{DEFAULT_CEREBRAS_URL, DEFAULT_DEEPSEEK_URL, DEFAULT_OLLAMA_URL};

pub struct LlmProviderFactory;

impl LlmProviderFactory {
    #[must_use]
    pub fn create(
        provider: &str,
        model: &str,
        api_key: Option<&str>,
        base_url: Option<&str>,
        temperature: f64,
        request_timeout_secs: u64,
    ) -> Box<dyn LlmProvider> {
        match provider {
            // Cerebras and DeepSeek are both OpenAI-compatible; they differ
            // only in endpoint, auth, and whether thinking mode is disabled.
            "cerebras" => Box::new(OpenAiCompatProvider::new(
                "cerebras",
                base_url.unwrap_or(DEFAULT_CEREBRAS_URL),
                api_key.unwrap_or_default(),
                model,
                temperature,
                request_timeout_secs,
                false,
            )),
            "deepseek" => Box::new(OpenAiCompatProvider::new(
                "deepseek",
                base_url.unwrap_or(DEFAULT_DEEPSEEK_URL),
                api_key.unwrap_or_default(),
                model,
                temperature,
                request_timeout_secs,
                // DeepSeek V4 defaults to thinking mode; turn it off for
                // clean, fast JSON on the extraction/decision path.
                true,
            )),
            "ollama" => Box::new(OllamaProvider::with_timeout(
                base_url.unwrap_or(DEFAULT_OLLAMA_URL).to_string(),
                model.to_string(),
                temperature,
                request_timeout_secs,
            )),
            _ => panic!("Unknown provider: {provider}. Supported: cerebras, deepseek, ollama"),
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
        // The local fallback is always Ollama in production; the wrapper takes
        // it as an injected `LlmProvider` so the failover decision is unit-
        // testable with mocks (see fallback.rs tests).
        let fallback: std::sync::Arc<dyn LlmProvider> = std::sync::Arc::new(OllamaProvider::new(
            fallback_url.unwrap_or(DEFAULT_OLLAMA_URL).to_string(),
            fallback_model.to_string(),
            fallback_temperature,
        ));
        LlmProviderWithFallback::new(primary, fallback_enabled, fallback)
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
        let provider = LlmProviderFactory::create("ollama", "llama3.1:8b", None, None, 0.7, 600);
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
            600,
        );
        assert_eq!(provider.provider_name(), "ollama");
        assert_eq!(provider.model_name(), "gemma2:9b");
    }

    #[test]
    fn test_create_cerebras_provider() {
        let provider = LlmProviderFactory::create(
            "cerebras",
            "llama-3.3-70b",
            Some("test-key"),
            None,
            0.3,
            600,
        );
        assert_eq!(provider.provider_name(), "cerebras");
    }

    #[test]
    fn test_create_deepseek_provider() {
        let provider = LlmProviderFactory::create(
            "deepseek",
            "deepseek-v4-flash",
            Some("test-key"),
            None,
            0.3,
            600,
        );
        assert_eq!(provider.provider_name(), "deepseek");
        assert_eq!(provider.model_name(), "deepseek-v4-flash");
    }

    #[test]
    #[should_panic(expected = "Unknown provider")]
    fn test_unknown_provider_panics() {
        let _ = LlmProviderFactory::create("unknown", "model", None, None, 0.5, 600);
    }
}
