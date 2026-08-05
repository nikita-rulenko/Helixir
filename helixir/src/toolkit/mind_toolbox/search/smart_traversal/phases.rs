use super::models::{GraphScores, ScoreWeights, SearchConfig, SearchResult};
use super::rrf;
use super::scoring::{calculate_graph_score, calculate_temporal_freshness};
use crate::core::{RetrievalProfile, TimeWindow};
use crate::db::HelixClient;
use crate::utils::nullable_string;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{debug, info, warn};

#[derive(Debug, thiserror::Error)]
pub enum TraversalError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Invalid query: {0}")]
    InvalidQuery(String),
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)] // `chunks` mirrors the HelixDB response shape; kept for parity / future use.
struct VectorSearchResponse {
    #[serde(default)]
    memories: Vec<VectorMemory>,
    #[serde(default)]
    chunks: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, Clone)]
struct VectorMemory {
    #[serde(default, deserialize_with = "nullable_string")]
    id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    memory_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    content: String,
    #[serde(default, deserialize_with = "nullable_string")]
    created_at: String,
    #[serde(default, deserialize_with = "nullable_string")]
    valid_from: String,
    #[serde(default, deserialize_with = "nullable_string")]
    memory_type: String,
    #[serde(default, deserialize_with = "nullable_string")]
    content_key: String,
    #[serde(default, deserialize_with = "nullable_string")]
    user_id: String,
}

#[derive(Debug, Deserialize, Default)]
struct GraphConnectionsResponse {
    #[serde(default)]
    implies_out: Vec<ConnectedMemory>,
    #[serde(default)]
    implies_in: Vec<ConnectedMemory>,
    #[serde(default)]
    because_out: Vec<ConnectedMemory>,
    #[serde(default)]
    because_in: Vec<ConnectedMemory>,
    #[serde(default)]
    contradicts_out: Vec<ConnectedMemory>,
    #[serde(default)]
    contradicts_in: Vec<ConnectedMemory>,
    #[serde(default)]
    relation_out: Vec<ConnectedMemory>,
    #[serde(default)]
    relation_in: Vec<ConnectedMemory>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // `memory_type` reflected from HelixDB; reserved for upcoming filters.
struct ConnectedMemory {
    #[serde(default, deserialize_with = "nullable_string")]
    memory_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    content: String,
    #[serde(default, deserialize_with = "nullable_string")]
    created_at: String,
    #[serde(default, deserialize_with = "nullable_string")]
    valid_from: String,
    #[serde(default, deserialize_with = "nullable_string")]
    memory_type: String,
}

mod bm25;
mod graph;
mod ranking;
mod vector;

pub use graph::graph_expansion_phase;
pub use ranking::rank_and_filter;
pub use vector::vector_search_phase;
