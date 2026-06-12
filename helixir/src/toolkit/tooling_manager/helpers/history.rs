//! `HistoryEvent` write helper — used by the audit/versioning surface.

use tracing::debug;

use super::super::{ToolingError, ToolingManager};

impl ToolingManager {
    pub(crate) async fn add_memory_history_event(
        &self,
        memory_id: &str,
        action: &str,
        old_value: &str,
        new_value: &str,
        actor: &str,
    ) -> Result<(), ToolingError> {
        let event_id = uuid::Uuid::new_v4().to_string();
        let timestamp = chrono::Utc::now().to_rfc3339();
        let params = serde_json::json!({
            "memory_id": memory_id,
            "event_id": event_id,
            "action": action,
            "old_value": old_value,
            "new_value": new_value,
            "timestamp": timestamp,
            "actor": actor
        });
        self.db
            .execute_query::<serde_json::Value, _>("addMemoryHistoryEvent", &params)
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        debug!(
            "History event for memory {}: {} by {}",
            memory_id, action, actor
        );
        Ok(())
    }
}
