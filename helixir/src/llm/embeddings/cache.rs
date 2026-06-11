//! Process-local LRU+TTL cache used to short-circuit repeat embedding requests.
//!
//! Crude eviction: when full, the entry with the oldest `created_at` is dropped.
//! Replacement is out of scope here — `moka` is on the wishlist for the next
//! pass; the current implementation has been stable across releases.
//!
//! Optional disk persistence (algo-opt R2): when constructed with a path, the
//! cache loads previously computed embeddings at startup and appends every new
//! one to the same JSONL file. text → embedding is a pure function of the
//! model, so persisted entries never expire; lines recorded under a different
//! model are skipped on load. This removes the cold-start re-embedding burst
//! (hundreds of ms via ollama) from the first search of a process.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

pub(super) struct CacheEntry {
    pub(super) embedding: Vec<f32>,
    pub(super) created_at: Instant,
    /// Loaded from disk — exempt from TTL expiry (embeddings are pure).
    pub(super) persistent: bool,
}

#[derive(Serialize, Deserialize)]
struct DiskLine {
    m: String,
    t: String,
    e: Vec<f32>,
}

pub(super) struct EmbeddingCache {
    cache: RwLock<HashMap<String, CacheEntry>>,
    max_size: usize,
    ttl: Duration,
    /// `(model, append handle)` when disk persistence is enabled.
    disk: Option<(String, Mutex<File>)>,
}

impl EmbeddingCache {
    pub(super) fn new(max_size: usize, ttl_secs: u64) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            max_size,
            ttl: Duration::from_secs(ttl_secs),
            disk: None,
        }
    }

    /// Cache with JSONL persistence at `path`, scoped to `model`.
    pub(super) fn with_persistence(
        max_size: usize,
        ttl_secs: u64,
        path: &Path,
        model: &str,
    ) -> Self {
        let mut map = HashMap::new();
        if let Ok(file) = File::open(path) {
            let mut loaded = 0usize;
            for line in BufReader::new(file).lines().map_while(Result::ok) {
                if map.len() >= max_size {
                    break;
                }
                let Ok(parsed) = serde_json::from_str::<DiskLine>(&line) else {
                    continue;
                };
                if parsed.m != model {
                    continue;
                }
                map.insert(
                    parsed.t,
                    CacheEntry {
                        embedding: parsed.e,
                        created_at: Instant::now(),
                        persistent: true,
                    },
                );
                loaded += 1;
            }
            info!(
                "Embedding cache: loaded {} persisted entries from {}",
                loaded,
                path.display()
            );
        }

        let handle = if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
            OpenOptions::new().create(true).append(true).open(path)
        } else {
            OpenOptions::new().create(true).append(true).open(path)
        };

        let disk = match handle {
            Ok(f) => Some((model.to_string(), Mutex::new(f))),
            Err(e) => {
                warn!(
                    "Embedding cache: cannot open {} for append ({}); persistence disabled",
                    path.display(),
                    e
                );
                None
            }
        };

        Self {
            cache: RwLock::new(map),
            max_size,
            ttl: Duration::from_secs(ttl_secs),
            disk,
        }
    }

    pub(super) fn get(&self, text: &str) -> Option<Vec<f32>> {
        let cache = self.cache.read().unwrap();
        if let Some(entry) = cache.get(text) {
            if entry.persistent || entry.created_at.elapsed() < self.ttl {
                return Some(entry.embedding.clone());
            }
        }
        None
    }

    pub(super) fn set(&self, text: &str, embedding: Vec<f32>) {
        if let Some((model, file)) = &self.disk {
            let already_persisted = {
                let cache = self.cache.read().unwrap();
                cache.get(text).is_some_and(|e| e.persistent)
            };
            if !already_persisted
                && let Ok(line) = serde_json::to_string(&DiskLine {
                    m: model.clone(),
                    t: text.to_string(),
                    e: embedding.clone(),
                })
                && let Ok(mut f) = file.lock()
            {
                let _ = writeln!(f, "{line}");
            }
        }

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
                persistent: self.disk.is_some(),
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
