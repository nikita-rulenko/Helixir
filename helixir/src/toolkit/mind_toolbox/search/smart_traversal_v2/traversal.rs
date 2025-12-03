use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use lru::LruCache;
use chrono::{DateTime, Utc};
use sha2::{Sha256, Digest};
use tracing::{debug, info, warn};
use super::models::{SearchResult, SearchConfig, TraversalStats};
use super::phases::{vector_search_phase, graph_expansion_phase, rank_and_filter, TraversalError};
use crate::db::HelixClient;

pub struct SmartTraversalV2 {
    client: Arc<HelixClient>,
    cache: RwLock<LruCache<String, Vec<SearchResult>>>,
    cache_ttl: Duration,
    stats: RwLock<TraversalStats>,
}

impl SmartTraversalV2 {
    pub fn new(client: Arc<HelixClient>, cache_size: usize, cache_ttl_secs: u64) -> Self {
        Self {
            client,
            cache: RwLock::new(LruCache::new(
                std::num::NonZeroUsize::new(cache_size).unwrap()
            )),
            cache_ttl: Duration::from_secs(cache_ttl_secs),
            stats: RwLock::new(TraversalStats::default()),
        }
    }
    
    pub async fn search(
        &self,
        query: &str,
        query_embedding: &[f32],
        user_id: Option<&str>,
        config: SearchConfig,
        temporal_cutoff: Option<DateTime<Utc>>,
    ) -> Result<Vec<SearchResult>, TraversalError> {
        let cache_key = Self::make_cache_key(query_embedding, user_id, &config);
        
        
        {
            let mut cache = self.cache.write().await;
            if let Some(cached_results) = cache.get(&cache_key) {
                let mut stats = self.stats.write().await;
                stats.cache_hits += 1;
                stats.cache_hit_rate = stats.cache_hits as f64 / (stats.cache_hits + stats.cache_misses) as f64;
                debug!("Cache hit for query: {}", query);
                return Ok(cached_results.clone());
            }
        }
        
        let start_time = Instant::now();
        info!("Starting smart traversal search for query: {}", query);
        
        
        {
            let mut stats = self.stats.write().await;
            stats.cache_misses += 1;
            stats.cache_hit_rate = stats.cache_hits as f64 / (stats.cache_hits + stats.cache_misses) as f64;
        }
        
        
        let phase1_start = Instant::now();
        let vector_hits = vector_search_phase(
            Arc::clone(&self.client),
            query_embedding,
            user_id,
            config.vector_top_k,
            config.min_vector_score,
            temporal_cutoff,
        ).await?;
        let phase1_duration = phase1_start.elapsed();
        
        if vector_hits.is_empty() {
            info!("No vector hits found, returning empty results");
            let total_duration = start_time.elapsed();
            let mut stats = self.stats.write().await;
            stats.phase1_duration_ms = phase1_duration.as_millis() as f64;
            stats.total_duration_ms = total_duration.as_millis() as f64;
            return Ok(vec![]);
        }
        
        
        let phase2_start = Instant::now();
        let edge_types = config.edge_types.as_deref().unwrap_or(&[]);
        let graph_results = graph_expansion_phase(
            Arc::clone(&self.client),
            &vector_hits,
            query_embedding,
            config.graph_depth,
            edge_types,
        ).await?;
        let phase2_duration = phase2_start.elapsed();
        
        
        let mut all_results = vector_hits;
        all_results.extend(graph_results);
        
        
        let phase3_start = Instant::now();
        let final_results = rank_and_filter(all_results, config.min_combined_score);
        let phase3_duration = phase3_start.elapsed();
        
        let total_duration = start_time.elapsed();
        
        
        {
            let mut stats = self.stats.write().await;
            stats.phase1_duration_ms = phase1_duration.as_millis() as f64;
            stats.phase2_duration_ms = phase2_duration.as_millis() as f64;
            stats.phase3_duration_ms = phase3_duration.as_millis() as f64;
            stats.total_duration_ms = total_duration.as_millis() as f64;
            stats.cache_size = self.cache.read().await.len();
        }
        
        
        {
            let mut cache = self.cache.write().await;
            cache.put(cache_key, final_results.clone());
        }
        
        info!("Smart traversal search completed in {:.2}ms with {} results", 
              total_duration.as_millis(), final_results.len());
        
        Ok(final_results)
    }
    
    pub fn get_stats(&self) -> TraversalStats {
        
        
        TraversalStats::default()
    }
    
    fn make_cache_key(
        query_embedding: &[f32],
        user_id: Option<&str>,
        config: &SearchConfig,
    ) -> String {
        let mut hasher = Sha256::new();
        
        
        for value in query_embedding {
            hasher.update(value.to_le_bytes());
        }
        
        
        if let Some(uid) = user_id {
            hasher.update(uid.as_bytes());
        }
        
        
        hasher.update(config.vector_top_k.to_le_bytes());
        hasher.update(config.graph_depth.to_le_bytes());
        hasher.update(config.min_vector_score.to_le_bytes());
        hasher.update(config.min_combined_score.to_le_bytes());
        
        if let Some(edge_types) = &config.edge_types {
            for edge_type in edge_types {
                hasher.update(edge_type.as_bytes());
            }
        }
        
        format!("{:x}", hasher.finalize())
    }
}