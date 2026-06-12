//! Embedding configuration and provider defaults.

pub(super) const DEFAULT_FALLBACK_URL: &str = "http://localhost:11434";
pub(super) const DEFAULT_FALLBACK_MODEL: &str = "nomic-embed-text";

/// Configuration for [`super::EmbeddingGenerator`].
///
/// Replaces the previous 11-positional-argument constructor. Both the
/// primary and the fallback provider are described here in named fields
/// so the meaning of each URL is unambiguous at call sites.
///
/// `base_url` is the endpoint of the **primary** provider (the one named
/// in `provider`). For `ollama` it is the Ollama host (e.g.
/// `http://localhost:11434`); for `openai`-compatible providers it is the
/// API root (e.g. `https://openrouter.ai/api/v1` or `https://api.openai.com/v1`).
/// An empty string means "use the provider's built-in default".
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub timeout_secs: u64,
    pub cache_size: usize,
    pub cache_ttl: u64,
    pub fallback_enabled: bool,
    pub fallback_url: String,
    pub fallback_model: String,
}
