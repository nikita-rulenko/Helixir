

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug, error};

use super::base::Event;


pub type EventHandler = Arc<dyn Fn(Event) + Send + Sync>;


pub struct EventBus {
    handlers: Arc<RwLock<HashMap<String, Vec<EventHandler>>>>,
}

impl EventBus {
    
    #[must_use]
    pub fn new() -> Self {
        Self {
            handlers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    
    pub async fn register(&self, event_type: &str, handler: EventHandler) {
        let mut handlers = self.handlers.write().await;
        handlers
            .entry(event_type.to_string())
            .or_default()
            .push(handler);
        debug!("Registered handler for event type: {}", event_type);
    }

    
    pub async fn emit(&self, event: Event) {
        let handlers = self.handlers.read().await;

        if let Some(event_handlers) = handlers.get(&event.event_type) {
            for handler in event_handlers {
                let handler = Arc::clone(handler);
                let event = event.clone();

                tokio::spawn(async move {
                    handler(event);
                });
            }
        } else {
            debug!("No handlers for event type: {}", event.event_type);
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn test_event_bus() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        let handler: EventHandler = Arc::new(move |_event| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        bus.register("test.event", handler).await;

        let event = Event::new("test.event", json!({"test": true}));
        bus.emit(event).await;

        
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
