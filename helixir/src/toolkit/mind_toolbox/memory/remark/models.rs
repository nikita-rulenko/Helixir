use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemarkResult {
    pub memory_id: String,
    pub entities_added: usize,
    pub concepts_added: usize,
    pub success: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
}

impl RemarkResult {
    pub fn success(memory_id: String, entities: usize, concepts: usize, duration_ms: u64) -> Self {
        Self {
            memory_id,
            entities_added: entities,
            concepts_added: concepts,
            success: true,
            error: None,
            duration_ms,
        }
    }

    pub fn failure(memory_id: String, error: String) -> Self {
        Self {
            memory_id,
            entities_added: 0,
            concepts_added: 0,
            success: false,
            error: Some(error),
            duration_ms: 0,
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemarkStats {
    pub total_processed: usize,
    pub total_entities: usize,
    pub total_concepts: usize,
    pub failures: usize,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl RemarkStats {
    pub fn new() -> Self {
        Self {
            total_processed: 0,
            total_entities: 0,
            total_concepts: 0,
            failures: 0,
            started_at: Some(Utc::now()),
            completed_at: None,
        }
    }

    pub fn add_result(&mut self, result: &RemarkResult) {
        self.total_processed += 1;
        if result.success {
            self.total_entities += result.entities_added;
            self.total_concepts += result.concepts_added;
        } else {
            self.failures += 1;
        }
    }

    pub fn duration_secs(&self) -> Option<f64> {
        match (self.started_at, self.completed_at) {
            (Some(start), Some(end)) => Some((end - start).num_milliseconds() as f64 / 1000.0),
            (Some(start), None) => Some((Utc::now() - start).num_milliseconds() as f64 / 1000.0),
            _ => None,
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_processed == 0 {
            0.0
        } else {
            (self.total_processed - self.failures) as f64 / self.total_processed as f64
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnmarkedMemory {
    pub memory_id: String,
    pub content: String,
    pub created_at: String,
    pub user_id: String,
}