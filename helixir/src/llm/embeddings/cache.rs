//! Process-local LRU+TTL embedding cache with optional bounded persistence.
//!
//! The durable format is deliberately namespaced by the complete embedding
//! identity rather than by model name alone. A provider/endpoint/model change
//! therefore misses safely instead of returning a vector from another space.

mod disk;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tracing::{info, warn};

use self::disk::{DiskRecord, PersistentStore};

const CACHE_FORMAT_VERSION: u8 = 2;
const DEFAULT_MAX_BYTES: u64 = 128 * 1024 * 1024;

/// Bounded operational view of the embedding cache.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EmbeddingCacheDiagnostics {
    pub entries: usize,
    pub bytes: u64,
    pub namespaces: Vec<String>,
    pub hits: u64,
    pub misses: u64,
    pub compactions: u64,
    pub invalidations: u64,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct CacheNamespace {
    id: String,
    expected_dimension: Option<usize>,
}

impl CacheNamespace {
    pub(super) fn new(
        provider: &str,
        endpoint: &str,
        model: &str,
        revision: &str,
        expected_dimension: Option<usize>,
        epoch: &str,
    ) -> Self {
        let endpoint = normalize_endpoint(endpoint);
        let identity = format!(
            "v={CACHE_FORMAT_VERSION}\nprovider={}\nendpoint={endpoint}\nmodel={}\nrevision={}\ndimension={}\nepoch={}",
            provider.trim().to_ascii_lowercase(),
            model.trim(),
            revision.trim(),
            expected_dimension.map_or_else(|| "auto".to_string(), |value| value.to_string()),
            epoch.trim()
        );
        Self {
            id: format!("{:x}", Sha256::digest(identity.as_bytes())),
            expected_dimension,
        }
    }

    fn accepts_dimension(&self, dimension: usize) -> bool {
        dimension > 0
            && self
                .expected_dimension
                .is_none_or(|expected| expected == dimension)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CacheKey {
    namespace: String,
    text_hash: String,
}

impl CacheKey {
    fn new(namespace: &CacheNamespace, text: &str) -> Self {
        Self {
            namespace: namespace.id.clone(),
            text_hash: format!("{:x}", Sha256::digest(text.as_bytes())),
        }
    }
}

pub(super) struct CacheEntry {
    pub(super) embedding: Vec<f32>,
    pub(super) created_at: Instant,
    pub(super) persistent: bool,
}

pub(super) struct EmbeddingCache {
    cache: RwLock<HashMap<CacheKey, CacheEntry>>,
    max_size: usize,
    ttl: Duration,
    disk: Option<PersistentStore>,
    namespaces: Vec<String>,
    hits: AtomicU64,
    misses: AtomicU64,
    invalidations: AtomicU64,
}

impl EmbeddingCache {
    pub(super) fn new(max_size: usize, ttl_secs: u64) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            max_size: max_size.max(1),
            ttl: Duration::from_secs(ttl_secs),
            disk: None,
            namespaces: Vec::new(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            invalidations: AtomicU64::new(0),
        }
    }

    pub(super) fn with_persistence(
        max_size: usize,
        ttl_secs: u64,
        path: &Path,
        namespaces: &[CacheNamespace],
    ) -> Self {
        let max_size = max_size.max(1);
        let max_bytes = std::env::var("HELIXIR_EMBED_CACHE_MAX_BYTES")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_MAX_BYTES);
        Self::with_persistence_limits(max_size, ttl_secs, path, namespaces, max_bytes)
    }

    fn with_persistence_limits(
        max_size: usize,
        ttl_secs: u64,
        path: &Path,
        namespaces: &[CacheNamespace],
        max_bytes: u64,
    ) -> Self {
        match PersistentStore::open(path, namespaces, max_size, max_bytes) {
            Ok((disk, records, invalidations)) => {
                let mut cache = HashMap::with_capacity(records.len());
                for record in records {
                    cache.insert(
                        CacheKey {
                            namespace: record.namespace,
                            text_hash: record.text_hash,
                        },
                        CacheEntry {
                            embedding: record.embedding,
                            created_at: Instant::now(),
                            persistent: true,
                        },
                    );
                }
                info!(
                    "Embedding cache: loaded {} current entries from {}",
                    cache.len(),
                    path.display()
                );
                Self {
                    cache: RwLock::new(cache),
                    max_size,
                    ttl: Duration::from_secs(ttl_secs),
                    disk: Some(disk),
                    namespaces: namespaces
                        .iter()
                        .map(|namespace| namespace.id.clone())
                        .collect(),
                    hits: AtomicU64::new(0),
                    misses: AtomicU64::new(0),
                    invalidations: AtomicU64::new(invalidations),
                }
            }
            Err(error) => {
                warn!(
                    "Embedding cache: cannot initialize {} ({error}); persistence disabled",
                    path.display()
                );
                Self::new(max_size, ttl_secs)
            }
        }
    }

    pub(super) fn get(&self, namespace: &CacheNamespace, text: &str) -> Option<Vec<f32>> {
        let key = CacheKey::new(namespace, text);
        let cache = self
            .cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = cache.get(&key).and_then(|entry| {
            let current = entry.persistent || entry.created_at.elapsed() < self.ttl;
            (current && namespace.accepts_dimension(entry.embedding.len()))
                .then(|| entry.embedding.clone())
        });
        if result.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }
        result
    }

    pub(super) fn set(&self, namespace: &CacheNamespace, text: &str, embedding: Vec<f32>) {
        if !namespace.accepts_dimension(embedding.len()) {
            warn!(
                "Embedding cache: refused dimension {} for namespace {}",
                embedding.len(),
                &namespace.id[..12]
            );
            return;
        }
        let key = CacheKey::new(namespace, text);
        let record = DiskRecord::new(
            namespace.id.clone(),
            key.text_hash.clone(),
            embedding.clone(),
        );
        let persistent = self.disk.as_ref().is_some_and(|disk| {
            disk.append_and_compact(&record).unwrap_or_else(|error| {
                warn!("Embedding cache: persistence write failed ({error})");
                false
            })
        });

        let mut cache = self
            .cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if cache.len() >= self.max_size
            && !cache.contains_key(&key)
            && let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, entry)| entry.created_at)
                .map(|(key, _)| key.clone())
        {
            cache.remove(&oldest_key);
        }
        cache.insert(
            key,
            CacheEntry {
                embedding,
                created_at: Instant::now(),
                persistent,
            },
        );
    }

    pub(super) fn clear(&self) {
        let removed = self
            .cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .drain()
            .count();
        self.invalidations
            .fetch_add(removed as u64, Ordering::Relaxed);
        if let Some(disk) = &self.disk
            && let Err(error) = disk.clear()
        {
            warn!("Embedding cache: durable clear failed ({error})");
        }
    }

    pub(super) fn len(&self) -> usize {
        self.cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    pub(super) fn diagnostics(&self) -> EmbeddingCacheDiagnostics {
        EmbeddingCacheDiagnostics {
            entries: self.len(),
            bytes: self
                .disk
                .as_ref()
                .and_then(|disk| disk.bytes().ok())
                .unwrap_or(0),
            namespaces: self.namespaces.clone(),
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            compactions: self.disk.as_ref().map_or(0, PersistentStore::compactions),
            invalidations: self.invalidations.load(Ordering::Relaxed),
        }
    }
}

fn normalize_endpoint(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    url::Url::parse(trimmed)
        .map(|mut parsed| {
            parsed.set_fragment(None);
            parsed.set_query(None);
            parsed.set_username("").ok();
            parsed.set_password(None).ok();
            parsed.to_string().trim_end_matches('/').to_string()
        })
        .unwrap_or_else(|_| trimmed.to_ascii_lowercase())
}
