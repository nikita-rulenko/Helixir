use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SearchMethod {
    Vector,
    Bm25,
    Hybrid,
    SmartGraphV2,
    OntoSearch,
}

impl fmt::Display for SearchMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchMethod::Vector => write!(f, "Vector"),
            SearchMethod::Bm25 => write!(f, "BM25"),
            SearchMethod::Hybrid => write!(f, "Hybrid"),
            SearchMethod::SmartGraphV2 => write!(f, "SmartGraphV2"),
            SearchMethod::OntoSearch => write!(f, "OntoSearch"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub memory_id: String,
    pub content: String,
    pub score: f64,
    pub method: SearchMethod,
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: String,
}

impl fmt::Display for SearchResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let short_id = crate::safe_truncate(&self.memory_id, 8);
        write!(f, "{} [{:.3}] {}", short_id, self.score, self.method)
    }
}