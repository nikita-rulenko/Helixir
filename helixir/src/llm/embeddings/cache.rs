//! Process-local LRU+TTL cache used to short-circuit repeat embedding requests.
//!
//! Crude eviction: when full, the entry with the oldest `created_at` is dropped.
//! Replacement is out of scope here — `moka` is on the wishlist for the next
//! pass; the current implementation has been stable across releases.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

pub(super) struct CacheEntry {
    pub(super) embedding: Vec<f32>,
    pub(super) created_at: Instant,
}

pub(super) struct EmbeddingCache {
    cache: RwLock<HashMap<String, CacheEntry>>,
    max_size: usize,
    ttl: Duration,
}

impl EmbeddingCache {
    pub(super) fn new(max_size: usize, ttl_secs: u64) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            max_size,
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    pub(super) fn get(&self, text: &str) -> Option<Vec<f32>> {
        let cache = self.cache.read().unwrap();
        if let Some(entry) = cache.get(text) {
            if entry.created_at.elapsed() < self.ttl {
                return Some(entry.embedding.clone());
            }
        }
        None
    }

    pub(super) fn set(&self, text: &str, embedding: Vec<f32>) {
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
    }

    pub(super) fn clear(&self) {
        self.cache.write().unwrap().clear();
    }

    pub(super) fn len(&self) -> usize {
        self.cache.read().unwrap().len()
    }
}
