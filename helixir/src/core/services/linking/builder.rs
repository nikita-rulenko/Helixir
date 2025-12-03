

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::events::{LinkCreatedEvent, LinkingCompleteEvent};
use crate::core::services::chunking::ChunkCreatedEvent;
use crate::db::HelixClient;


#[derive(Debug, Clone)]
struct TrackedChunk {
    chunk_id: String,
    chunk_internal_id: Option<Uuid>,
    position: usize,
    correlation_id: Option<String>,
}


pub struct LinkBuilder {
    
    client: Arc<HelixClient>,
    
    chunks_by_memory: RwLock<HashMap<String, Vec<TrackedChunk>>>,
    
    expected_chunks: RwLock<HashMap<String, usize>>,
    
    event_tx: Option<tokio::sync::mpsc::Sender<LinkBuilderEvent>>,
}


#[derive(Debug, Clone)]
pub enum LinkBuilderEvent {
    LinkCreated(LinkCreatedEvent),
    Complete(LinkingCompleteEvent),
}

impl LinkBuilder {
    
    pub fn new(client: Arc<HelixClient>) -> Self {
        info!("LinkBuilder initialized");

        Self {
            client,
            chunks_by_memory: RwLock::new(HashMap::new()),
            expected_chunks: RwLock::new(HashMap::new()),
            event_tx: None,
        }
    }

    
    pub fn with_event_sender(mut self, tx: tokio::sync::mpsc::Sender<LinkBuilderEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    
    pub async fn handle_chunk_created(&self, event: ChunkCreatedEvent) {
        let memory_id = event.parent_memory_id.clone();

        debug!(
            "Tracking chunk: {} (position={}/{})",
            event.chunk_id, event.position, event.total_chunks
        );

        
        {
            let mut chunks = self.chunks_by_memory.write().await;
            let memory_chunks = chunks.entry(memory_id.clone()).or_insert_with(Vec::new);

            memory_chunks.push(TrackedChunk {
                chunk_id: event.chunk_id.clone(),
                chunk_internal_id: event.chunk_internal_id,
                position: event.position,
                correlation_id: event.correlation_id.clone(),
            });

            let mut expected = self.expected_chunks.write().await;
            expected.insert(memory_id.clone(), event.total_chunks);
        }

        
        let (collected, expected) = {
            let chunks = self.chunks_by_memory.read().await;
            let expected = self.expected_chunks.read().await;

            let collected = chunks.get(&memory_id).map(|c| c.len()).unwrap_or(0);
            let expected = expected.get(&memory_id).copied().unwrap_or(0);

            (collected, expected)
        };

        if collected == expected && expected > 0 {
            debug!("All {} chunks collected for {}", expected, memory_id);

            
            self.create_chunk_chain(&memory_id, event.correlation_id.clone())
                .await;

            
            {
                let mut chunks = self.chunks_by_memory.write().await;
                let mut expected = self.expected_chunks.write().await;

                chunks.remove(&memory_id);
                expected.remove(&memory_id);
            }
        }
    }

    
    async fn create_chunk_chain(&self, memory_id: &str, correlation_id: Option<String>) {
        let start_time = Instant::now();

        let chunks = {
            let chunks = self.chunks_by_memory.read().await;
            chunks.get(memory_id).cloned().unwrap_or_default()
        };

        
        let mut sorted_chunks = chunks;
        sorted_chunks.sort_by_key(|c| c.position);

        
        if sorted_chunks.len() <= 1 {
            debug!("Single chunk for {} - no chain needed", memory_id);

            self.emit_event(LinkBuilderEvent::Complete(LinkingCompleteEvent {
                memory_id: memory_id.to_string(),
                edges_created: 0,
                errors: 0,
                duration_ms: start_time.elapsed().as_secs_f64() * 1000.0,
                correlation_id,
            }))
            .await;

            return;
        }

        
        let mut edges_created = 0;
        let mut errors = 0;

        for i in 0..sorted_chunks.len() - 1 {
            let from_chunk = &sorted_chunks[i];
            let to_chunk = &sorted_chunks[i + 1];

            match self
                .create_next_chunk_edge(from_chunk, to_chunk, correlation_id.clone())
                .await
            {
                Ok(event) => {
                    self.emit_event(LinkBuilderEvent::LinkCreated(event)).await;
                    edges_created += 1;
                }
                Err(e) => {
                    warn!(
                        "Failed to create edge {} -> {}: {}",
                        from_chunk.chunk_id, to_chunk.chunk_id, e
                    );
                    errors += 1;
                }
            }
        }

        info!(
            "Chain complete for {}: {} edges, {} errors",
            memory_id, edges_created, errors
        );

        self.emit_event(LinkBuilderEvent::Complete(LinkingCompleteEvent {
            memory_id: memory_id.to_string(),
            edges_created,
            errors,
            duration_ms: start_time.elapsed().as_secs_f64() * 1000.0,
            correlation_id,
        }))
        .await;
    }

    
    async fn create_next_chunk_edge(
        &self,
        from_chunk: &TrackedChunk,
        to_chunk: &TrackedChunk,
        correlation_id: Option<String>,
    ) -> Result<LinkCreatedEvent, String> {
        let from_id = from_chunk
            .chunk_internal_id
            .ok_or("Missing from_chunk internal ID")?;
        let to_id = to_chunk
            .chunk_internal_id
            .ok_or("Missing to_chunk internal ID")?;

        #[derive(serde::Serialize)]
        struct Input {
            from_chunk_id: String,
            to_chunk_id: String,
        }

        #[derive(serde::Deserialize)]
        struct Output {
            id: Option<String>,
        }

        let result: Output = self
            .client
            .execute_query(
                "linkChunks",
                &Input {
                    from_chunk_id: from_id.to_string(),
                    to_chunk_id: to_id.to_string(),
                },
            )
            .await
            .map_err(|e| e.to_string())?;

        let edge_id = result.id.and_then(|s| Uuid::parse_str(&s).ok());

        Ok(LinkCreatedEvent {
            from_chunk_id: from_chunk.chunk_id.clone(),
            to_chunk_id: to_chunk.chunk_id.clone(),
            edge_type: "NEXT_CHUNK".to_string(),
            edge_id,
            correlation_id,
        })
    }

    
    async fn emit_event(&self, event: LinkBuilderEvent) {
        if let Some(ref tx) = self.event_tx {
            if let Err(e) = tx.send(event).await {
                warn!("Failed to emit link builder event: {}", e);
            }
        }
    }

    
    pub async fn get_stats(&self) -> LinkBuilderStats {
        let chunks = self.chunks_by_memory.read().await;

        LinkBuilderStats {
            pending_memories: chunks.len(),
            total_chunks_tracked: chunks.values().map(|c| c.len()).sum(),
        }
    }
}


#[derive(Debug, Clone)]
pub struct LinkBuilderStats {
    pub pending_memories: usize,
    pub total_chunks_tracked: usize,
}