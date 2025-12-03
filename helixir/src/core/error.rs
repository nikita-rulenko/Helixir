

use thiserror::Error;


#[derive(Error, Debug)]
pub enum HelixirError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("HelixDB connection error: {0}")]
    Connection(String),

    #[error("Query execution error: {0}")]
    Query(String),

    #[error("LLM provider error: {0}")]
    LlmProvider(String),

    #[error("Embedding generation error: {0}")]
    Embedding(String),

    #[error("Memory not found: {0}")]
    MemoryNotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}


pub type Result<T> = std::result::Result<T, HelixirError>;

