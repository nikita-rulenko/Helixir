//! Embedding subsystem error type.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum EmbeddingError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON parsing failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Empty text")]
    EmptyText,

    #[error("Provider not implemented: {0}")]
    NotImplemented(String),

    #[error("Both primary and fallback failed: primary={0}, fallback={1}")]
    BothFailed(String, String),
}
