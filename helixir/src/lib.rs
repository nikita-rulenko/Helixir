pub mod agents;
pub mod core;
pub mod db;
pub mod llm;
pub mod mcp;
pub mod toolkit;
pub mod utils;

pub use utils::{safe_truncate, safe_truncate_ellipsis};

pub use core::config::HelixirConfig;
pub use core::error::{HelixirError, Result};
pub use db::{HelixClient, HelixClientError};
pub use llm::embeddings::EmbeddingGenerator;

// Canonical shared defaults — the single home for these strings/values, so
// config.rs (and providers) reference them instead of re-hardcoding.
pub const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";
pub const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";
/// The primary LLM model default. llama-3.3-70b was retired from the Cerebras
/// account (404 → fresh installs couldn't extract), so the default is now an
/// available model. NOTE: a fully-local default (ollama/specialist pipeline, #56)
/// is the intended direction — this Cerebras id is a stopgap so a no-env install
/// works out of the box.
pub const DEFAULT_LLM_MODEL: &str = "gpt-oss-120b";
/// Local fallback model when the remote chain (Cerebras/DeepSeek) errors.
/// llama3.2:3b won the 2026-07 M1 bake-off (7 models, full add pipeline +
/// causal-contract e2e): contract 2/2 in 95s vs 421s (qwen3.5:2b) and ~2×
/// faster than qwen2.5:7b at 2GB, with no example-leak hallucination
/// (qwen2.5:3b copied the prompt's worked example into a stored memory —
/// disqualified). The deeper multi-step-reasoning feature (think_commit)
/// degrades on any local model — remote-only capability, not a size knob.
pub const DEFAULT_LLM_FALLBACK_MODEL: &str = "llama3.2:3b";
/// OpenAI-compatible chat-completions endpoints for the hosted providers.
pub const DEFAULT_CEREBRAS_URL: &str = "https://api.cerebras.ai/v1/chat/completions";
pub const DEFAULT_DEEPSEEK_URL: &str = "https://api.deepseek.com/chat/completions";
/// DeepSeek default: the cheap, fast tier ($0.14/$0.28 per 1M tok), run in
/// non-thinking mode for clean JSON. ~2x cheaper than Cerebras and far faster
/// than any local model, at comparable extraction quality.
pub const DEFAULT_DEEPSEEK_MODEL: &str = "deepseek-v4-flash";
pub const DEFAULT_HELIX_PORT: u16 = 6969;
pub const DEFAULT_CACHE_SIZE: usize = 1000;
pub const DEFAULT_CACHE_TTL: u64 = 300;
/// Ollama HTTP request timeout (seconds). Generous by default so weak hardware
/// running a large local model doesn't trip the client before the model replies.
pub const DEFAULT_LLM_REQUEST_TIMEOUT_SECS: u64 = 600;
