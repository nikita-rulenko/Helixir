use lru::LruCache;
use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicU64, Ordering};
use sha2::{Sha256, Digest};
use parking_lot::Mutex;

pub struct SearchCache<T> {
    cache: Mutex<LruCache<String, (T, Instant)>>,
    ttl: Duration,
    hits: AtomicU64,
    misses: AtomicU64,
}

#[derive(Debug, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub size: usize,
    pub hit_rate: f64,
}

impl<T> SearchCache<T> {
    pub fn new(capacity: usize, ttl_secs: u64) -> Self {
        Self {
            cache: Mutex::new(LruCache::new(capacity.try_into().unwrap())),
            ttl: Duration::from_secs(ttl_secs),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    pub fn get(&self, key: &str) -> Option<T>
    where
        T: Clone,
    {
        let mut cache = self.cache.lock();
        if let Some((value, timestamp)) = cache.get(key) {
            if timestamp.elapsed() < self.ttl {
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(value.clone())
            } else {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    pub fn set(&self, key: &str, value: T) {
        let mut cache = self.cache.lock();
        cache.put(key.to_string(), (value, Instant::now()));
    }

    pub fn make_key(query: &str, user_id: Option<&str>, limit: usize, min_score: f64) -> String {
        let mut hasher = Sha256::new();
        hasher.update(query.as_bytes());
        if let Some(uid) = user_id {
            hasher.update(uid.as_bytes());
        }
        hasher.update(limit.to_string().as_bytes());
        hasher.update(min_score.to_string().as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub fn stats(&self) -> CacheStats {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        let hit_rate = if total > 0 { hits as f64 / total as f64 } else { 0.0 };
        let cache = self.cache.lock();
        
        CacheStats {
            hits,
            misses,
            size: cache.len(),
            hit_rate,
        }
    }

    pub fn clear(&self) {
        let mut cache = self.cache.lock();
        cache.clear();
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
    }
}