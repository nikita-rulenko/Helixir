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
/// The primary LLM model default — kept in sync with `HelixirConfig` (was
/// previously a stale "llama3.1:8b" that diverged from the cerebras default).
pub const DEFAULT_LLM_MODEL: &str = "llama-3.3-70b";
pub const DEFAULT_LLM_FALLBACK_MODEL: &str = "llama3.2";
pub const DEFAULT_HELIX_PORT: u16 = 6969;
pub const DEFAULT_CACHE_SIZE: usize = 1000;
pub const DEFAULT_CACHE_TTL: u64 = 300;
/// Ollama HTTP request timeout (seconds). Generous by default so weak hardware
/// running a large local model doesn't trip the client before the model replies.
pub const DEFAULT_LLM_REQUEST_TIMEOUT_SECS: u64 = 600;
