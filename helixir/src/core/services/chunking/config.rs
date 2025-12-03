

use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChunkingStrategy {
    
    #[default]
    Semantic,
    
    Sentence,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkingConfig {
    
    #[serde(default = "default_chunk_size")]
    pub chunk_size: usize,

    
    #[serde(default = "default_similarity_threshold")]
    pub similarity_threshold: f64,

    
    #[serde(default = "default_min_chunk_length")]
    pub min_chunk_length: usize,

    
    #[serde(default = "default_min_sentences")]
    pub min_sentences_per_chunk: usize,

    
    #[serde(default)]
    pub strategy: ChunkingStrategy,

    
    #[serde(default = "default_overlap")]
    pub chunk_overlap: usize,
}

fn default_chunk_size() -> usize { 1024 }
fn default_similarity_threshold() -> f64 { 0.7 }
fn default_min_chunk_length() -> usize { 1000 }
fn default_min_sentences() -> usize { 2 }
fn default_overlap() -> usize { 128 }

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            chunk_size: default_chunk_size(),
            similarity_threshold: default_similarity_threshold(),
            min_chunk_length: default_min_chunk_length(),
            min_sentences_per_chunk: default_min_sentences(),
            strategy: ChunkingStrategy::default(),
            chunk_overlap: default_overlap(),
        }
    }
}

impl ChunkingConfig {
    
    pub fn semantic(chunk_size: usize, threshold: f64) -> Self {
        Self {
            chunk_size,
            similarity_threshold: threshold,
            strategy: ChunkingStrategy::Semantic,
            ..Default::default()
        }
    }

    
    pub fn sentence(chunk_size: usize, overlap: usize) -> Self {
        Self {
            chunk_size,
            chunk_overlap: overlap,
            strategy: ChunkingStrategy::Sentence,
            ..Default::default()
        }
    }

    
    pub fn needs_chunking(&self, content_length: usize) -> bool {
        content_length >= self.min_chunk_length
    }
}

