

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

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


pub const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";


pub const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";


pub const DEFAULT_LLM_MODEL: &str = "llama3.1:8b";


pub const DEFAULT_HELIX_PORT: u16 = 6969;


pub const DEFAULT_CACHE_SIZE: usize = 1000;


pub const DEFAULT_CACHE_TTL: u64 = 300;
