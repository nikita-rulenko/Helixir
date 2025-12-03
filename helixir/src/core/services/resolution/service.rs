

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;
use tracing::{debug, info, warn};

use super::error::ResolutionError;
use crate::db::HelixClient;


struct CacheEntry {
    uuid: Uuid,
    inserted_at: std::time::Instant,
}


pub struct IDResolutionService {
    
    client: Arc<HelixClient>,
    
    cache: RwLock<lru::LruCache<String, CacheEntry>>,
    
    ttl: Duration,
    
    stats: RwLock<ResolutionStats>,
}


#[derive(Debug, Default, Clone)]
pub struct ResolutionStats {
    pub hits: u64,
    pub misses: u64,
    pub invalidations: u64,
    pub evictions: u64,
}

impl IDResolutionService {
    
    
    pub fn new(client: Arc<HelixClient>, max_size: usize, ttl_secs: u64) -> Self {
        info!(
            "IDResolutionService initialized: max_size={}, ttl={}s",
            max_size, ttl_secs
        );

        Self {
            client,
            cache: RwLock::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(max_size).unwrap_or(std::num::NonZeroUsize::new(10000).unwrap())
            )),
            ttl: Duration::from_secs(ttl_secs),
            stats: RwLock::new(ResolutionStats::default()),
        }
    }

    
    pub async fn resolve(&self, memory_id: &str) -> Result<Uuid, ResolutionError> {
        debug!("Resolving ID: {}", memory_id);

        
        {
            let mut cache = self.cache.write().await;
            if let Some(entry) = cache.get(memory_id) {
                
                if entry.inserted_at.elapsed() < self.ttl {
                    let mut stats = self.stats.write().await;
                    stats.hits += 1;
                    debug!("Cache HIT for {}", memory_id);
                    return Ok(entry.uuid);
                } else {
                    
                    cache.pop(memory_id);
                    debug!("Cache entry expired for {}", memory_id);
                }
            }
        }

        
        {
            let mut stats = self.stats.write().await;
            stats.misses += 1;
        }
        debug!("Cache MISS for {}", memory_id);

        let uuid = self.query_db(memory_id).await?;

        
        {
            let mut cache = self.cache.write().await;
            cache.put(
                memory_id.to_string(),
                CacheEntry {
                    uuid,
                    inserted_at: std::time::Instant::now(),
                },
            );
        }

        debug!("Cached {} -> {}", memory_id, uuid);
        Ok(uuid)
    }

    
    pub async fn resolve_many(
        &self,
        memory_ids: &[String],
    ) -> std::collections::HashMap<String, Uuid> {
        use futures::future::join_all;

        debug!("Batch resolving {} IDs", memory_ids.len());

        let futures: Vec<_> = memory_ids
            .iter()
            .map(|id| async move { (id.clone(), self.resolve(id).await) })
            .collect();

        let results = join_all(futures).await;

        let mut resolved = std::collections::HashMap::new();
        let mut errors = 0;

        for (id, result) in results {
            match result {
                Ok(uuid) => {
                    resolved.insert(id, uuid);
                }
                Err(e) => {
                    warn!("Failed to resolve {}: {}", id, e);
                    errors += 1;
                }
            }
        }

        info!(
            "Batch resolve complete: {}/{} resolved, {} errors",
            resolved.len(),
            memory_ids.len(),
            errors
        );

        resolved
    }

    
    async fn query_db(&self, memory_id: &str) -> Result<Uuid, ResolutionError> {
        debug!("Querying DB for {}", memory_id);

        #[derive(serde::Serialize)]
        struct Input<'a> {
            memory_id: &'a str,
        }

        #[derive(serde::Deserialize)]
        struct Output {
            id: Option<String>,
        }

        let result: Output = self
            .client
            .execute_query("getMemory", &Input { memory_id })
            .await
            .map_err(|e| ResolutionError::Database(e.to_string()))?;

        let id_str = result
            .id
            .ok_or_else(|| ResolutionError::NotFound(memory_id.to_string()))?;

        Uuid::parse_str(&id_str).map_err(|e| ResolutionError::InvalidUuid(e.to_string()))
    }

    
    pub async fn invalidate(&self, memory_id: &str) {
        let mut cache = self.cache.write().await;
        if cache.pop(memory_id).is_some() {
            let mut stats = self.stats.write().await;
            stats.invalidations += 1;
            debug!("Invalidated cache for {}", memory_id);
        }
    }

    
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
        info!("Cache cleared");
    }

    
    pub async fn get_stats(&self) -> ResolutionStats {
        self.stats.read().await.clone()
    }
}