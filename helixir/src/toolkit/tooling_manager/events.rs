use crate::core::events::{Event, EventBus};
use serde_json::json;
use std::sync::Arc;

impl super::ToolingManager {
    pub(crate) async fn emit_memory_added(&self, memory_id: &str, user_id: &str, memory_type: &str) {
        let event = Event::new("memory.added", json!({
            "memory_id": memory_id,
            "user_id": user_id,
            "memory_type": memory_type
        }));
        self.event_bus.emit(event).await;
    }

    pub(crate) async fn emit_memory_updated(&self, memory_id: &str, user_id: &str) {
        let event = Event::new("memory.updated", json!({
            "memory_id": memory_id,
            "user_id": user_id
        }));
        self.event_bus.emit(event).await;
    }

    pub(crate) async fn emit_memory_superseded(&self, new_id: &str, old_id: &str, user_id: &str) {
        let event = Event::new("memory.superseded", json!({
            "new_memory_id": new_id,
            "old_memory_id": old_id,
            "user_id": user_id
        }));
        self.event_bus.emit(event).await;
    }

    pub(crate) async fn emit_memory_deduplicated(&self, memory_id: &str, user_id: &str) {
        let event = Event::new("memory.deduplicated", json!({
            "memory_id": memory_id,
            "user_id": user_id
        }));
        self.event_bus.emit(event).await;
    }

    pub(crate) async fn emit_search_executed(&self, user_id: &str, mode: &str, result_count: usize) {
        let event = Event::new("search.executed", json!({
            "user_id": user_id,
            "mode": mode,
            "result_count": result_count
        }));
        self.event_bus.emit(event).await;
    }

    pub(crate) async fn emit_llm_decision(&self, operation: &str, confidence: u32, user_id: &str) {
        let event = Event::new("llm.decision.made", json!({
            "operation": operation,
            "confidence": confidence,
            "user_id": user_id
        }));
        self.event_bus.emit(event).await;
    }

    pub fn event_bus(&self) -> &Arc<EventBus> {
        &self.event_bus
    }
}
