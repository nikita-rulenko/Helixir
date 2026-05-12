//! Top-level client error. One variant per failure boundary.

#[derive(Debug, thiserror::Error)]
pub enum HelixirClientError {
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Database error: {0}")]
    Database(String),
    #[error("LLM error: {0}")]
    Llm(String),
    #[error("Embedding error: {0}")]
    Embedding(String),
    #[error("Tooling error: {0}")]
    Tooling(String),
    #[error("Client not initialized")]
    NotInitialized,
    #[error("Operation failed: {0}")]
    Operation(String),
}
