

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use strum::{EnumString, IntoStaticStr};


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum IssueStatus {
    Created,
    InProgress,
    Blocked,
    Resolved,
    Deprecated,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum EventType {
    IssueCreated,
    IssueStatusChanged,
    IssueResolved,
    CommitMade,
    MemoryAdded,
    FeatureCompleted,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VelocityEvent {
    pub event_type: EventType,
    pub timestamp: DateTime<Utc>,
    pub entity_id: String,
    pub metadata: HashMap<String, serde_json::Value>,
    pub user_id: String,
}

impl VelocityEvent {
    
    pub fn new(
        event_type: EventType,
        entity_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Self {
        Self {
            event_type,
            timestamp: Utc::now(),
            entity_id: entity_id.into(),
            metadata: HashMap::new(),
            user_id: user_id.into(),
        }
    }

    
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        self.metadata.insert(
            key.into(),
            serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
        );
        self
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueState {
    pub status: IssueStatus,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub transitions: Vec<IssueTransition>,
    pub metadata: HashMap<String, serde_json::Value>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueTransition {
    pub from: Option<IssueStatus>,
    pub to: IssueStatus,
    pub at: DateTime<Utc>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VelocityMetrics {
    
    pub avg_bug_resolution_secs: f64,
    
    pub bugs_resolved_count: usize,
    
    pub bugs_open_count: usize,
    
    pub avg_feature_implementation_secs: f64,
    
    pub features_completed_count: usize,
    
    pub commits_per_day: f64,
    
    pub memories_per_session: f64,
    
    pub bug_reopen_rate: f64,
    
    pub memory_update_rate: f64,
    
    pub velocity_score: f64,
    
    pub period_start: DateTime<Utc>,
    
    pub period_end: DateTime<Utc>,
}

impl Default for VelocityMetrics {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            avg_bug_resolution_secs: 0.0,
            bugs_resolved_count: 0,
            bugs_open_count: 0,
            avg_feature_implementation_secs: 0.0,
            features_completed_count: 0,
            commits_per_day: 0.0,
            memories_per_session: 0.0,
            bug_reopen_rate: 0.0,
            memory_update_rate: 0.0,
            velocity_score: 0.0,
            period_start: now,
            period_end: now,
        }
    }
}

