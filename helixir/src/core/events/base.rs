

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    
    pub correlation_id: Option<Uuid>,
    
    pub metadata: Value,
}

impl Default for EventMetadata {
    fn default() -> Self {
        Self {
            correlation_id: None,
            metadata: Value::Null,
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    
    pub event_id: Uuid,
    
    pub event_type: String,
    
    pub timestamp: DateTime<Utc>,
    
    pub metadata: EventMetadata,
    
    pub payload: Value,
}

impl Event {
    
    #[must_use]
    pub fn new(event_type: impl Into<String>, payload: Value) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            event_type: event_type.into(),
            timestamp: Utc::now(),
            metadata: EventMetadata::default(),
            payload,
        }
    }

    
    #[must_use]
    pub fn with_correlation(mut self, correlation_id: Uuid) -> Self {
        self.metadata.correlation_id = Some(correlation_id);
        self
    }
}
