

use thiserror::Error;

#[derive(Error, Debug)]
pub enum HelixirError {
    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Query error: {message}")]
    Query { message: String, query: Option<String> },

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Schema error: {0}")]
    Schema(String),

    #[error("Ontology error: {0}")]
    Ontology(String),

    #[error("Memory operation error: {0}")]
    MemoryOperation(String),

    #[error("Reasoning error: {0}")]
    Reasoning(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

impl HelixirError {
    pub fn query(message: impl Into<String>, query: Option<String>) -> Self {
        Self::Query {
            message: message.into(),
            query,
        }
    }
}

pub type Result<T> = std::result::Result<T, HelixirError>;
