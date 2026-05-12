//! HTTP wire DTOs for Ollama / OpenAI-compatible embedding endpoints.
//!
//! `pub(super)` so the generator can build/parse them; not part of the
//! crate's public API.

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub(super) struct OllamaEmbeddingRequest {
    pub(super) model: String,
    pub(super) prompt: String,
}

#[derive(Deserialize)]
pub(super) struct OllamaEmbeddingResponse {
    pub(super) embedding: Vec<f32>,
}

#[derive(Serialize)]
pub(super) struct OpenAIEmbeddingRequest {
    pub(super) model: String,
    pub(super) input: String,
}

#[derive(Serialize)]
pub(super) struct OpenAIBatchEmbeddingRequest {
    pub(super) model: String,
    pub(super) input: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct OpenAIEmbeddingResponse {
    pub(super) data: Vec<OpenAIEmbeddingData>,
}

#[derive(Deserialize)]
pub(super) struct OpenAIEmbeddingData {
    pub(super) embedding: Vec<f32>,
    #[serde(default)]
    pub(super) index: usize,
}
