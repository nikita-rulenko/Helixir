

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};


pub struct EmbeddingCache {
    cache: RwLock<HashMap<String, CacheEntry>>,
    max_size: usize,
    ttl: Duration,
    stats: RwLock<CacheStats>,
}

struct CacheEntry {
    embedding: Vec<f32>,
    created_at: Instant,
}

#[derive(Debug, Default, Clone)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub size: usize,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

impl EmbeddingCache {
    
    pub fn new(max_size: usize, ttl_secs: u64) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            max_size,
            ttl: Duration::from_secs(ttl_secs),
            stats: RwLock::new(CacheStats::default()),
        }
    }

    
    pub fn get(&self, text: &str) -> Option<Vec<f32>> {
        let cache = self.cache.read().unwrap();
        
        if let Some(entry) = cache.get(text) {
            if entry.created_at.elapsed() < self.ttl {
                let mut stats = self.stats.write().unwrap();
                stats.hits += 1;
                return Some(entry.embedding.clone());
            }
        }
        
        let mut stats = self.stats.write().unwrap();
        stats.misses += 1;
        None
    }

    
    pub fn set(&self, text: &str, embedding: Vec<f32>) {
        let mut cache = self.cache.write().unwrap();
        
        
        if cache.len() >= self.max_size {
            
            if let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, v)| v.created_at)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&oldest_key);
            }
        }
        
        cache.insert(
            text.to_string(),
            CacheEntry {
                embedding,
                created_at: Instant::now(),
            },
        );
        
        let mut stats = self.stats.write().unwrap();
        stats.size = cache.len();
    }

    
    pub fn stats(&self) -> CacheStats {
        self.stats.read().unwrap().clone()
    }

    
    pub fn clear(&self) {
        let mut cache = self.cache.write().unwrap();
        cache.clear();
        
        let mut stats = self.stats.write().unwrap();
        stats.size = 0;
    }
}

