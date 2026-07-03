use super::providers::base::LlmProvider;
use super::providers::fallback::LlmProviderWithFallback;
use super::providers::ollama::OllamaProvider;
use super::providers::openai_compat::OpenAiCompatProvider;
use crate::{DEFAULT_CEREBRAS_URL, DEFAULT_DEEPSEEK_URL, DEFAULT_OLLAMA_URL};
use tracing::{info, warn};

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

    /// Resolve `config.llm_fallback_chain` into concrete providers, in order.
    /// The resilience strategy in one line: smart remote → cheap remote →
    /// local selfhost — the agent survives quota exhaustion AND a network
    /// outage. Skips (with a warning, never a boot failure): tiers equal to
    /// the primary, tiers missing credentials, and unknown names.
    #[must_use]
    pub fn resolve_fallback_tiers(
        config: &crate::core::config::HelixirConfig,
    ) -> Vec<std::sync::Arc<dyn LlmProvider>> {
        let mut tiers: Vec<std::sync::Arc<dyn LlmProvider>> = Vec::new();
        if !config.llm_fallback_enabled {
            return tiers;
        }
        let temperature = f64::from(config.llm_temperature);
        let timeout = config.llm_runtime.request_timeout_secs;

        for name in &config.llm_fallback_chain {
            if *name == config.llm_provider {
                info!("fallback tier '{name}' skipped: it is already the primary");
                continue;
            }
            match name.as_str() {
                "deepseek" => match config.deepseek_api_key.as_deref().filter(|k| !k.is_empty()) {
                    Some(key) => tiers.push(
                        Self::create(
                            "deepseek",
                            &config.deepseek_model,
                            Some(key),
                            None,
                            temperature,
                            timeout,
                        )
                        .into(),
                    ),
                    None => {
                        warn!("fallback tier 'deepseek' skipped: HELIX_DEEPSEEK_API_KEY not set")
                    }
                },
                "ollama" => tiers.push(
                    Self::create(
                        "ollama",
                        &config.llm_fallback_model,
                        None,
                        Some(&config.llm_fallback_url),
                        temperature,
                        timeout,
                    )
                    .into(),
                ),
                // Cerebras-as-fallback reuses llm_api_key: it only makes sense
                // when the primary is keyless (ollama), so the key is free.
                "cerebras" => match config.llm_api_key.as_deref().filter(|k| !k.is_empty()) {
                    Some(key) => tiers.push(
                        Self::create(
                            "cerebras",
                            &config.llm_model,
                            Some(key),
                            None,
                            temperature,
                            timeout,
                        )
                        .into(),
                    ),
                    None => warn!("fallback tier 'cerebras' skipped: HELIX_LLM_API_KEY not set"),
                },
                other => warn!(
                    "unknown fallback tier '{other}' skipped. Supported: cerebras, deepseek, ollama"
                ),
            }
        }
        tiers
    }

    /// Wrap the primary in the resolved fallback chain; identity passthrough
    /// when no tier survived resolution (nothing to fall back to).
    #[must_use]
    pub fn create_chained(
        primary: std::sync::Arc<dyn LlmProvider>,
        config: &crate::core::config::HelixirConfig,
    ) -> std::sync::Arc<dyn LlmProvider> {
        let tiers = Self::resolve_fallback_tiers(config);
        if tiers.is_empty() {
            primary
        } else {
            std::sync::Arc::new(LlmProviderWithFallback::new_chain(primary, true, tiers))
        }
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

    // ---- fallback-chain resolution ----

    use crate::core::config::HelixirConfig;

    #[test]
    fn chain_without_deepseek_key_degrades_to_ollama_only() {
        // default chain is ["deepseek", "ollama"]; no key ⇒ deepseek skipped,
        // NOT a boot failure.
        let config = HelixirConfig::default();
        assert_eq!(config.llm_fallback_chain, vec!["deepseek", "ollama"]);
        let tiers = LlmProviderFactory::resolve_fallback_tiers(&config);
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].provider_name(), "ollama");
    }

    #[test]
    fn chain_with_deepseek_key_yields_both_tiers_in_order() {
        let mut config = HelixirConfig::default();
        config.deepseek_api_key = Some("test-key".to_string());
        let tiers = LlmProviderFactory::resolve_fallback_tiers(&config);
        let names: Vec<&str> = tiers.iter().map(|t| t.provider_name()).collect();
        assert_eq!(names, vec!["deepseek", "ollama"]);
        assert_eq!(tiers[0].model_name(), config.deepseek_model);
    }

    #[test]
    fn chain_skips_tier_equal_to_primary_and_unknown_names() {
        let mut config = HelixirConfig::default();
        config.llm_provider = "ollama".to_string();
        config.llm_fallback_chain = vec!["ollama".to_string(), "gpt5".to_string()];
        let tiers = LlmProviderFactory::resolve_fallback_tiers(&config);
        assert!(tiers.is_empty(), "primary-dup and unknown must both skip");
    }

    #[test]
    fn chain_disabled_resolves_empty() {
        let mut config = HelixirConfig::default();
        config.deepseek_api_key = Some("test-key".to_string());
        config.llm_fallback_enabled = false;
        assert!(LlmProviderFactory::resolve_fallback_tiers(&config).is_empty());
    }

    #[test]
    fn cerebras_tier_reuses_primary_key_when_primary_is_keyless() {
        // ollama-primary users can chain up to a remote: ollama → cerebras.
        let mut config = HelixirConfig::default();
        config.llm_provider = "ollama".to_string();
        config.llm_api_key = Some("cb-key".to_string());
        config.llm_fallback_chain = vec!["cerebras".to_string()];
        let tiers = LlmProviderFactory::resolve_fallback_tiers(&config);
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].provider_name(), "cerebras");
    }
}
