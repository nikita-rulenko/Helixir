

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, info, warn, error};
use uuid::Uuid;

use super::config::{ChunkingConfig, ChunkingStrategy};
use super::events::{
    ChunkCreatedEvent, ChunkingCompleteEvent, ChunkingFailedEvent,
    ChunkingStartedEvent, MemoryCreatedEvent,
};
use super::splitter::{ContentSplitter, SentenceSplitter, SemanticSplitter, TextChunk};
use crate::core::services::resolution::IDResolutionService;
use crate::db::HelixClient;


pub struct ChunkingService {
    
    client: Arc<HelixClient>,
    
    id_resolver: Arc<IDResolutionService>,
    
    splitter: Arc<dyn ContentSplitter>,
    
    config: ChunkingConfig,
    
    event_tx: Option<tokio::sync::mpsc::Sender<ChunkingEvent>>,
}


#[derive(Debug, Clone)]
pub enum ChunkingEvent {
    Started(ChunkingStartedEvent),
    ChunkCreated(ChunkCreatedEvent),
    Complete(ChunkingCompleteEvent),
    Failed(ChunkingFailedEvent),
}

impl ChunkingService {
    
    pub fn new(
        client: Arc<HelixClient>,
        id_resolver: Arc<IDResolutionService>,
        config: ChunkingConfig,
    ) -> Self {
        let splitter: Arc<dyn ContentSplitter> = match config.strategy {
            ChunkingStrategy::Semantic => Arc::new(SemanticSplitter::new(
                config.chunk_size,
                config.similarity_threshold,
            )),
            ChunkingStrategy::Sentence => Arc::new(SentenceSplitter::new(
                config.chunk_size,
                config.chunk_overlap,
                config.min_sentences_per_chunk,
            )),
        };

        info!(
            "ChunkingService initialized: strategy={:?}, chunk_size={}",
            config.strategy, config.chunk_size
        );

        Self {
            client,
            id_resolver,
            splitter,
            config,
            event_tx: None,
        }
    }

    
    pub fn with_event_sender(mut self, tx: tokio::sync::mpsc::Sender<ChunkingEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    
    pub async fn handle_memory_created(
        &self,
        event: MemoryCreatedEvent,
    ) -> Result<ChunkingCompleteEvent, ChunkingFailedEvent> {
        let start_time = Instant::now();
        let memory_id = event.memory_id.clone();

        debug!(
            "Processing memory: {} (length={})",
            memory_id,
            event.content.len()
        );

        
        if !event.needs_chunking || !self.config.needs_chunking(event.content.len()) {
            debug!("Skipping chunking for {}: content too short", memory_id);
            return Ok(ChunkingCompleteEvent {
                memory_id,
                chunks_created: 0,
                links_created: 0,
                chains_created: 0,
                duration_ms: start_time.elapsed().as_secs_f64() * 1000.0,
                success: true,
                correlation_id: event.correlation_id,
            });
        }

        
        match self.process_chunking(&event).await {
            Ok(complete_event) => Ok(complete_event),
            Err(e) => {
                let failed = ChunkingFailedEvent {
                    memory_id: memory_id.clone(),
                    stage: "chunking_pipeline".to_string(),
                    error: e.to_string(),
                    correlation_id: event.correlation_id,
                };

                self.emit_event(ChunkingEvent::Failed(failed.clone())).await;

                Err(failed)
            }
        }
    }

    
    async fn process_chunking(
        &self,
        event: &MemoryCreatedEvent,
    ) -> Result<ChunkingCompleteEvent, Box<dyn std::error::Error + Send + Sync>> {
        let start_time = Instant::now();
        let memory_id = &event.memory_id;

        
        let internal_id = match &event.internal_id {
            Some(id) => *id,
            None => self.id_resolver.resolve(memory_id).await?,
        };

        debug!("Resolved {} -> {}", memory_id, internal_id);

        
        let chunks = self.splitter.split(&event.content).await?;
        let chunk_count = chunks.len();

        debug!("Split into {} chunks", chunk_count);

        
        self.emit_event(ChunkingEvent::Started(ChunkingStartedEvent {
            memory_id: memory_id.clone(),
            internal_id,
            content_length: event.content.len(),
            estimated_chunks: chunk_count,
            chunking_strategy: self.splitter.name().to_string(),
            correlation_id: event.correlation_id.clone(),
        }))
        .await;

        
        let mut handles = Vec::with_capacity(chunk_count);

        for (position, chunk) in chunks.into_iter().enumerate() {
            let client = self.client.clone();
            let memory_id = memory_id.clone();
            let correlation_id = event.correlation_id.clone();

            handles.push(tokio::spawn(async move {
                Self::create_chunk(
                    &client,
                    &memory_id,
                    internal_id,
                    chunk,
                    position,
                    chunk_count,
                    correlation_id,
                )
                .await
            }));
        }

        
        let mut successful = Vec::new();
        let mut errors = Vec::new();

        for handle in handles {
            match handle.await {
                Ok(Ok(event)) => {
                    self.emit_event(ChunkingEvent::ChunkCreated(event.clone())).await;
                    successful.push(event);
                }
                Ok(Err(e)) => errors.push(e),
                Err(e) => errors.push(format!("Task panic: {}", e)),
            }
        }

        if !errors.is_empty() {
            warn!(
                "Chunking had {} errors out of {} chunks",
                errors.len(),
                chunk_count
            );
        }

        
        let complete = ChunkingCompleteEvent {
            memory_id: memory_id.clone(),
            chunks_created: successful.len(),
            links_created: successful.len(), 
            chains_created: 0,               
            duration_ms: start_time.elapsed().as_secs_f64() * 1000.0,
            success: errors.is_empty(),
            correlation_id: event.correlation_id.clone(),
        };

        self.emit_event(ChunkingEvent::Complete(complete.clone())).await;

        Ok(complete)
    }

    
    async fn create_chunk(
        client: &HelixClient,
        parent_memory_id: &str,
        parent_internal_id: Uuid,
        chunk: TextChunk,
        position: usize,
        total_chunks: usize,
        correlation_id: Option<String>,
    ) -> Result<ChunkCreatedEvent, String> {
        let chunk_id = format!("{}_chunk_{}", parent_memory_id, position);

        #[derive(serde::Serialize)]
        struct Input {
            chunk_id: String,
            parent_id: String,
            position: usize,
            content: String,
            token_count: usize,
            created_at: String,
        }

        #[derive(serde::Deserialize)]
        struct Output {
            id: Option<String>,
        }

        let result: Output = client
            .execute_query(
                "addMemoryChunk",
                &Input {
                    chunk_id: chunk_id.clone(),
                    parent_id: parent_internal_id.to_string(),
                    position,
                    content: chunk.text.clone(),
                    token_count: chunk.token_count,
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
            )
            .await
            .map_err(|e| e.to_string())?;

        let chunk_internal_id = result
            .id
            .and_then(|s| Uuid::parse_str(&s).ok());

        Ok(ChunkCreatedEvent {
            chunk_id,
            chunk_internal_id,
            parent_memory_id: parent_memory_id.to_string(),
            parent_internal_id,
            position,
            content: chunk.text,
            token_count: chunk.token_count,
            total_chunks,
            correlation_id,
        })
    }

    
    async fn emit_event(&self, event: ChunkingEvent) {
        if let Some(ref tx) = self.event_tx {
            if let Err(e) = tx.send(event).await {
                warn!("Failed to emit chunking event: {}", e);
            }
        }
    }
}