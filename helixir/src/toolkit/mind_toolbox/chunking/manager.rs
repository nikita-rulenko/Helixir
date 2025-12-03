

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use text_splitter::TextSplitter;
use tracing::{debug, info, warn};

use crate::db::HelixClient;
use crate::llm::embeddings::EmbeddingGenerator;


pub const DEFAULT_THRESHOLD: usize = 500;


pub const DEFAULT_CHUNK_SIZE: usize = 512;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    
    pub chunk_id: String,
    
    pub content: String,
    
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub embedding: Vec<f32>,
    
    pub position: usize,
    
    pub memory_id: String,
    
    pub char_count: usize,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkingResult {
    
    pub memory_id: String,
    
    pub was_chunked: bool,
    
    pub chunk_count: usize,
    
    pub total_chars: usize,
    
    pub chunk_ids: Vec<String>,
}


#[derive(Debug, thiserror::Error)]
pub enum ChunkingError {
    
    #[error("Database error: {0}")]
    Database(String),

    
    #[error("Embedding error: {0}")]
    Embedding(String),

    
    #[error("Invalid configuration: {0}")]
    Config(String),
}


pub struct ChunkingManager {
    client: Arc<HelixClient>,
    embedder: Option<Arc<EmbeddingGenerator>>,
    splitter: TextSplitter<text_splitter::Characters>,
    threshold: usize,
    chunk_size: usize,
    enable_embeddings: bool,
}

impl ChunkingManager {
    
    pub fn new(
        client: Arc<HelixClient>,
        embedder: Option<Arc<EmbeddingGenerator>>,
    ) -> Self {
        Self::with_config(client, embedder, DEFAULT_THRESHOLD, DEFAULT_CHUNK_SIZE, true)
    }

    
    pub fn with_config(
        client: Arc<HelixClient>,
        embedder: Option<Arc<EmbeddingGenerator>>,
        threshold: usize,
        chunk_size: usize,
        enable_embeddings: bool,
    ) -> Self {
        
        
        let splitter = TextSplitter::new(chunk_size);

        info!(
            "ChunkingManager initialized (threshold={}, chunk_size={}, embeddings={})",
            threshold, chunk_size, enable_embeddings
        );

        Self {
            client,
            embedder,
            splitter,
            threshold,
            chunk_size,
            enable_embeddings,
        }
    }

    
    #[inline]
    pub fn should_chunk(&self, text: &str) -> bool {
        text.chars().count() > self.threshold
    }

    
    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    
    pub fn threshold(&self) -> usize {
        self.threshold
    }

    
    pub fn split_text(&self, text: &str) -> Vec<String> {
        if !self.should_chunk(text) {
            return vec![text.to_string()];
        }

        self.splitter
            .chunks(text)
            .map(|s| s.to_string())
            .collect()
    }

    
    pub async fn add_memory_with_chunking(
        &self,
        memory_id: &str,
        content: &str,
        user_id: &str,
        memory_type: &str,
        certainty: i64,
        importance: i64,
        source: &str,
        context_tags: &str,
        metadata: &str,
    ) -> Result<ChunkingResult, ChunkingError> {
        let char_count = content.chars().count();

        if !self.should_chunk(content) {
            debug!(
                "Content below threshold ({} chars), no chunking needed",
                char_count
            );
            return Ok(ChunkingResult {
                memory_id: memory_id.to_string(),
                was_chunked: false,
                chunk_count: 0,
                total_chars: char_count,
                chunk_ids: Vec::new(),
            });
        }

        info!(
            "Chunking content ({} chars) into ~{}-char chunks",
            char_count, self.chunk_size
        );

        
        let chunks_text: Vec<String> = self.splitter
            .chunks(content)
            .map(|s| s.to_string())
            .collect();

        info!("Created {} chunks", chunks_text.len());

        
        #[derive(Deserialize)]
        struct GetMemResult {
            #[serde(default)]
            memory: Option<MemNode>,
        }
        #[derive(Deserialize)]
        struct MemNode {
            #[serde(default)]
            id: String,
        }

        let mem_result: GetMemResult = self
            .client
            .execute_query("getMemory", &serde_json::json!({"memory_id": memory_id}))
            .await
            .map_err(|e| ChunkingError::Database(e.to_string()))?;

        let memory_internal_id = match mem_result.memory {
            Some(m) if !m.id.is_empty() => m.id,
            _ => {
                return Err(ChunkingError::Database(format!(
                    "Memory {} not found",
                    memory_id
                )))
            }
        };

        let now = Utc::now().to_rfc3339();
        let mut chunk_ids = Vec::with_capacity(chunks_text.len());

        
        for (position, chunk_text) in chunks_text.iter().enumerate() {
            let chunk_id = format!("{}_chunk_{}", memory_id, position);

            #[derive(Serialize)]
            struct AddChunkInput {
                chunk_id: String,
                memory_id: String,
                content: String,
                position: i64,
                token_count: i64,
                created_at: String,
            }

            #[derive(Deserialize)]
            struct AddChunkOutput {
                #[serde(default)]
                chunk: Option<ChunkNode>,
            }
            #[derive(Deserialize)]
            struct ChunkNode {
                #[serde(default)]
                id: String,
            }

            let input = AddChunkInput {
                chunk_id: chunk_id.clone(),
                memory_id: memory_internal_id.clone(),
                content: chunk_text.clone(),
                position: position as i64,
                token_count: chunk_text.chars().count() as i64,
                created_at: now.clone(),
            };

            let chunk_result: AddChunkOutput = self
                .client
                .execute_query("addChunk", &input)
                .await
                .map_err(|e| ChunkingError::Database(e.to_string()))?;

            let chunk_internal_id = match chunk_result.chunk {
                Some(c) if !c.id.is_empty() => c.id,
                _ => {
                    warn!("Failed to create chunk {}", position);
                    continue;
                }
            };

            chunk_ids.push(chunk_id.clone());

            
            if self.enable_embeddings {
                if let Some(ref embedder) = self.embedder {
                    match embedder.generate(chunk_text, true).await {
                        Ok(vector) => {
                            #[derive(Serialize)]
                            struct AddChunkEmbeddingInput {
                                chunk_id: String,
                                vector_data: Vec<f32>,
                            }

                            let embed_input = AddChunkEmbeddingInput {
                                chunk_id: chunk_internal_id,
                                vector_data: vector,
                            };

                            if let Err(e) = self
                                .client
                                .execute_query::<serde_json::Value, _>(
                                    "addChunkEmbedding",
                                    &embed_input,
                                )
                                .await
                            {
                                warn!("Failed to add chunk {} embedding: {}", position, e);
                            } else {
                                debug!("✅ Chunk {} embedding created", position);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to generate embedding for chunk {}: {}", position, e);
                        }
                    }
                }
            }
        }

        info!(
            "✅ Memory chunked: {} chunks created for {}",
            chunk_ids.len(),
            memory_id
        );

        Ok(ChunkingResult {
            memory_id: memory_id.to_string(),
            was_chunked: true,
            chunk_count: chunk_ids.len(),
            total_chars: char_count,
            chunk_ids,
        })
    }

    
    pub fn reconstruct_content(&self, chunks: &[Chunk]) -> String {
        let mut sorted: Vec<_> = chunks.iter().collect();
        sorted.sort_by_key(|c| c.position);
        sorted.iter().map(|c| c.content.as_str()).collect::<Vec<_>>().join("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_chunk() {
        
        let short_text = "Short text";
        let long_text = "A".repeat(600);

        assert!(short_text.chars().count() <= DEFAULT_THRESHOLD);
        assert!(long_text.chars().count() > DEFAULT_THRESHOLD);
    }

    #[test]
    fn test_split_text_semantic() {
        let splitter = TextSplitter::new(100);
        let text = "First sentence. Second sentence. Third sentence here. And fourth one too.";
        let chunks: Vec<_> = splitter.chunks(text).collect();
        
        
        assert!(!chunks.is_empty());
        for chunk in &chunks {
            assert!(chunk.len() <= 100);
        }
    }

    #[test]
    fn test_split_cyrillic() {
        let splitter = TextSplitter::new(50);
        let text = "Первое предложение. Второе предложение. Третье предложение здесь.";
        let chunks: Vec<_> = splitter.chunks(text).collect();
        
        
        assert!(!chunks.is_empty());
        for chunk in &chunks {
            
            let _ = chunk.chars().count();
        }
    }
}
