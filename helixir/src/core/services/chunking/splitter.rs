

use async_trait::async_trait;
use thiserror::Error;


#[derive(Debug, Clone)]
pub struct TextChunk {
    
    pub text: String,
    
    pub token_count: usize,
    
    pub start_pos: usize,
    
    pub end_pos: usize,
}


#[derive(Error, Debug)]
pub enum SplitterError {
    #[error("Content too short to split")]
    ContentTooShort,
    #[error("Splitting failed: {0}")]
    SplitFailed(String),
}


#[async_trait]
pub trait ContentSplitter: Send + Sync {
    
    async fn split(&self, content: &str) -> Result<Vec<TextChunk>, SplitterError>;

    
    fn name(&self) -> &'static str;
}


pub struct SentenceSplitter {
    chunk_size: usize,
    overlap: usize,
    min_sentences: usize,
}

impl SentenceSplitter {
    pub fn new(chunk_size: usize, overlap: usize, min_sentences: usize) -> Self {
        Self {
            chunk_size,
            overlap,
            min_sentences,
        }
    }

    
    fn estimate_tokens(text: &str) -> usize {
        let words = text.split_whitespace().count();
        (words as f64 / 0.75) as usize
    }

    
    fn split_sentences(text: &str) -> Vec<&str> {
        
        let mut sentences = Vec::new();
        let mut start = 0;

        for (i, c) in text.char_indices() {
            if c == '.' || c == '!' || c == '?' {
                let end = i + c.len_utf8();
                let sentence = text[start..end].trim();
                if !sentence.is_empty() {
                    sentences.push(sentence);
                }
                start = end;
            }
        }

        
        let remaining = text[start..].trim();
        if !remaining.is_empty() {
            sentences.push(remaining);
        }

        sentences
    }
}

#[async_trait]
impl ContentSplitter for SentenceSplitter {
    async fn split(&self, content: &str) -> Result<Vec<TextChunk>, SplitterError> {
        let sentences = Self::split_sentences(content);

        if sentences.is_empty() {
            return Err(SplitterError::ContentTooShort);
        }

        let mut chunks = Vec::new();
        let mut current_chunk = String::new();
        let mut current_tokens = 0;
        let mut chunk_start = 0;
        let mut sentence_count = 0;

        for sentence in sentences {
            let sentence_tokens = Self::estimate_tokens(sentence);

            
            if current_tokens + sentence_tokens > self.chunk_size
                && sentence_count >= self.min_sentences
            {
                
                let chunk_end = chunk_start + current_chunk.len();
                chunks.push(TextChunk {
                    text: current_chunk.trim().to_string(),
                    token_count: current_tokens,
                    start_pos: chunk_start,
                    end_pos: chunk_end,
                });

                
                let overlap_start = current_chunk
                    .len()
                    .saturating_sub(self.overlap * 4); 
                current_chunk = current_chunk[overlap_start..].to_string();
                current_tokens = Self::estimate_tokens(&current_chunk);
                chunk_start = chunk_end - (current_chunk.len());
                sentence_count = 0;
            }

            
            if !current_chunk.is_empty() {
                current_chunk.push(' ');
            }
            current_chunk.push_str(sentence);
            current_tokens += sentence_tokens;
            sentence_count += 1;
        }

        
        if !current_chunk.is_empty() {
            let chunk_end = chunk_start + current_chunk.len();
            chunks.push(TextChunk {
                text: current_chunk.trim().to_string(),
                token_count: current_tokens,
                start_pos: chunk_start,
                end_pos: chunk_end,
            });
        }

        Ok(chunks)
    }

    fn name(&self) -> &'static str {
        "SentenceSplitter"
    }
}


pub struct SemanticSplitter {
    chunk_size: usize,
    similarity_threshold: f64,
}

impl SemanticSplitter {
    pub fn new(chunk_size: usize, similarity_threshold: f64) -> Self {
        Self {
            chunk_size,
            similarity_threshold,
        }
    }
}

#[async_trait]
impl ContentSplitter for SemanticSplitter {
    async fn split(&self, content: &str) -> Result<Vec<TextChunk>, SplitterError> {
        
        
        let sentence_splitter = SentenceSplitter::new(self.chunk_size, 128, 2);
        sentence_splitter.split(content).await
    }

    fn name(&self) -> &'static str {
        "SemanticSplitter"
    }
}

