

use std::collections::HashMap;
use std::sync::Arc;
use chrono::{DateTime, Utc, Duration};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::db::HelixClient;


#[derive(Error, Debug)]
pub enum AnalyticsError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Collection failed: {0}")]
    Collection(String),
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    pub total_size_bytes: usize,
    pub total_size_mb: f64,
    pub total_size_gb: f64,
    pub total_memories: usize,
    pub size_by_type: HashMap<String, usize>,
    pub avg_memory_size: f64,
    pub largest_memories: Vec<(String, usize)>,
    pub vector_count: usize,
    pub vector_storage_mb: f64,
    pub chunks_count: usize,
    pub chunks_storage_mb: f64,
    pub collected_at: DateTime<Utc>,
}

impl Default for StorageStats {
    fn default() -> Self {
        Self {
            total_size_bytes: 0,
            total_size_mb: 0.0,
            total_size_gb: 0.0,
            total_memories: 0,
            size_by_type: HashMap::new(),
            avg_memory_size: 0.0,
            largest_memories: Vec::new(),
            vector_count: 0,
            vector_storage_mb: 0.0,
            chunks_count: 0,
            chunks_storage_mb: 0.0,
            collected_at: Utc::now(),
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    pub node_counts: HashMap<String, usize>,
    pub total_nodes: usize,
    pub edge_counts: HashMap<String, usize>,
    pub total_edges: usize,
    pub orphaned_entities: usize,
    pub deleted_memories: usize,
    pub graph_density: f64,
    pub avg_degree: f64,
    pub collected_at: DateTime<Utc>,
}

impl Default for GraphStats {
    fn default() -> Self {
        Self {
            node_counts: HashMap::new(),
            total_nodes: 0,
            edge_counts: HashMap::new(),
            total_edges: 0,
            orphaned_entities: 0,
            deleted_memories: 0,
            graph_density: 0.0,
            avg_degree: 0.0,
            collected_at: Utc::now(),
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceStats {
    pub cache_hit_rate: f64,
    pub total_queries: usize,
    pub avg_query_latency_ms: f64,
    pub error_count: usize,
    pub collected_at: DateTime<Utc>,
}

impl Default for PerformanceStats {
    fn default() -> Self {
        Self {
            cache_hit_rate: 0.0,
            total_queries: 0,
            avg_query_latency_ms: 0.0,
            error_count: 0,
            collected_at: Utc::now(),
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrowthStats {
    pub memories_per_day: f64,
    pub growth_rate_percent: f64,
    pub trend: String,
    pub analysis_period_days: i64,
    pub collected_at: DateTime<Utc>,
}

impl Default for GrowthStats {
    fn default() -> Self {
        Self {
            memories_per_day: 0.0,
            growth_rate_percent: 0.0,
            trend: "unknown".to_string(),
            analysis_period_days: 7,
            collected_at: Utc::now(),
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsSummary {
    pub storage: StorageStats,
    pub graph: GraphStats,
    pub performance: PerformanceStats,
    pub growth: GrowthStats,
    pub collected_at: DateTime<Utc>,
}

impl Default for AnalyticsSummary {
    fn default() -> Self {
        Self {
            storage: StorageStats::default(),
            graph: GraphStats::default(),
            performance: PerformanceStats::default(),
            growth: GrowthStats::default(),
            collected_at: Utc::now(),
        }
    }
}


pub struct AnalyticsManager {
    client: Arc<HelixClient>,
}

impl AnalyticsManager {
    
    pub fn new(client: Arc<HelixClient>) -> Self {
        info!("AnalyticsManager initialized");
        Self { client }
    }

    
    pub async fn collect_all(&self) -> Result<AnalyticsSummary, AnalyticsError> {
        info!("Collecting all analytics...");

        let storage = self.collect_storage_stats().await?;
        let graph = self.collect_graph_stats().await?;
        let performance = self.collect_performance_stats().await;
        let growth = self.collect_growth_stats().await?;

        let summary = AnalyticsSummary {
            storage,
            graph,
            performance,
            growth,
            collected_at: Utc::now(),
        };

        info!(
            "✅ Analytics collected: {} memories, {} nodes, {:.2} MB",
            summary.storage.total_memories,
            summary.graph.total_nodes,
            summary.storage.total_size_mb
        );

        Ok(summary)
    }

    
    pub async fn collect_storage_stats(&self) -> Result<StorageStats, AnalyticsError> {
        debug!("Collecting storage stats...");

        #[derive(Deserialize)]
        struct MemoryData {
            memory_id: String,
            content: String,
            memory_type: Option<String>,
        }

        
        let memories: Vec<MemoryData> = self.client
            .execute_query("getAllMemories", &serde_json::json!({}))
            .await
            .map_err(|e| AnalyticsError::Database(e.to_string()))?;

        
        let total_memories = memories.len();
        let total_size_bytes: usize = memories.iter().map(|m| m.content.len()).sum();
        let total_size_mb = total_size_bytes as f64 / (1024.0 * 1024.0);
        let total_size_gb = total_size_mb / 1024.0;

        
        let mut size_by_type: HashMap<String, usize> = HashMap::new();
        for m in &memories {
            let mem_type = m.memory_type.clone().unwrap_or_else(|| "unknown".to_string());
            *size_by_type.entry(mem_type).or_insert(0) += m.content.len();
        }

        
        let avg_memory_size = if total_memories > 0 {
            total_size_bytes as f64 / total_memories as f64
        } else {
            0.0
        };

        
        let mut memories_with_sizes: Vec<(String, usize)> = memories
            .iter()
            .map(|m| (m.memory_id.clone(), m.content.len()))
            .collect();
        memories_with_sizes.sort_by_key(|x| std::cmp::Reverse(x.1));
        let largest_memories: Vec<(String, usize)> = memories_with_sizes.into_iter().take(10).collect();

        
        let vector_count = total_memories;
        let vector_storage_mb = (vector_count * 768 * 4) as f64 / (1024.0 * 1024.0);

        debug!(
            "Storage stats: {} memories, {:.2} MB, {} vectors",
            total_memories, total_size_mb, vector_count
        );

        Ok(StorageStats {
            total_size_bytes,
            total_size_mb,
            total_size_gb,
            total_memories,
            size_by_type,
            avg_memory_size,
            largest_memories,
            vector_count,
            vector_storage_mb,
            chunks_count: 0, 
            chunks_storage_mb: 0.0,
            collected_at: Utc::now(),
        })
    }

    
    pub async fn collect_graph_stats(&self) -> Result<GraphStats, AnalyticsError> {
        debug!("Collecting graph stats...");

        
        let memory_count: usize = self.client
            .execute_query::<usize, _>("countAllMemories", &serde_json::json!({}))
            .await
            .unwrap_or(0);

        let entity_count: usize = self.client
            .execute_query::<usize, _>("countAllEntities", &serde_json::json!({}))
            .await
            .unwrap_or(0);

        let concept_count: usize = self.client
            .execute_query::<usize, _>("countAllConcepts", &serde_json::json!({}))
            .await
            .unwrap_or(0);

        let mut node_counts = HashMap::new();
        node_counts.insert("Memory".to_string(), memory_count);
        node_counts.insert("Entity".to_string(), entity_count);
        node_counts.insert("Concept".to_string(), concept_count);

        let total_nodes = memory_count + entity_count + concept_count;

        
        let edge_counts = HashMap::new();
        let total_edges = 0;

        
        let max_edges = if total_nodes > 1 {
            total_nodes * (total_nodes - 1) / 2
        } else {
            0
        };
        let graph_density = if max_edges > 0 {
            total_edges as f64 / max_edges as f64
        } else {
            0.0
        };
        let avg_degree = if total_nodes > 0 {
            (2 * total_edges) as f64 / total_nodes as f64
        } else {
            0.0
        };

        debug!(
            "Graph stats: {} nodes ({} memories, {} entities, {} concepts)",
            total_nodes, memory_count, entity_count, concept_count
        );

        Ok(GraphStats {
            node_counts,
            total_nodes,
            edge_counts,
            total_edges,
            orphaned_entities: 0,
            deleted_memories: 0,
            graph_density,
            avg_degree,
            collected_at: Utc::now(),
        })
    }

    
    pub async fn collect_performance_stats(&self) -> PerformanceStats {
        debug!("Collecting performance stats...");

        
        PerformanceStats {
            cache_hit_rate: 0.0,
            total_queries: 0,
            avg_query_latency_ms: 0.0,
            error_count: 0,
            collected_at: Utc::now(),
        }
    }

    
    pub async fn collect_growth_stats(&self) -> Result<GrowthStats, AnalyticsError> {
        debug!("Collecting growth stats...");

        let analysis_period_days: i64 = 7;
        let cutoff_date = Utc::now() - Duration::days(analysis_period_days);

        #[derive(Deserialize)]
        struct MemoryWithDate {
            memory_id: String,
            created_at: String,
        }

        
        let memories: Vec<MemoryWithDate> = self.client
            .execute_query("getAllMemories", &serde_json::json!({}))
            .await
            .unwrap_or_default();

        
        let recent_count = memories.iter().filter(|m| {
            DateTime::parse_from_rfc3339(&m.created_at)
                .map(|dt| dt.with_timezone(&Utc) >= cutoff_date)
                .unwrap_or(false)
        }).count();

        let total_count = memories.len();
        let old_count = total_count.saturating_sub(recent_count);

        
        let memories_per_day = recent_count as f64 / analysis_period_days as f64;
        
        let growth_rate_percent = if old_count > 0 {
            (recent_count as f64 / old_count as f64) * 100.0
        } else if recent_count > 0 {
            100.0
        } else {
            0.0
        };

        let trend = match memories_per_day {
            x if x < 1.0 => "slow",
            x if x < 10.0 => "stable",
            x if x < 100.0 => "growing",
            _ => "rapid",
        }.to_string();

        debug!(
            "Growth stats: {:.1} memories/day, {:.1}% growth, trend={}",
            memories_per_day, growth_rate_percent, trend
        );

        Ok(GrowthStats {
            memories_per_day,
            growth_rate_percent,
            trend,
            analysis_period_days,
            collected_at: Utc::now(),
        })
    }

    
    pub async fn get_category_breakdown(&self) -> Result<HashMap<String, usize>, AnalyticsError> {
        debug!("Getting category breakdown...");

        #[derive(Deserialize)]
        struct MemoryType {
            memory_type: Option<String>,
        }

        let memories: Vec<MemoryType> = self.client
            .execute_query("getAllMemories", &serde_json::json!({}))
            .await
            .unwrap_or_default();

        let mut breakdown: HashMap<String, usize> = HashMap::new();
        for m in memories {
            let mem_type = m.memory_type.unwrap_or_else(|| "unknown".to_string());
            *breakdown.entry(mem_type).or_insert(0) += 1;
        }

        Ok(breakdown)
    }

    
    pub fn export_to_json(&self, summary: &AnalyticsSummary) -> String {
        serde_json::to_string_pretty(summary).unwrap_or_else(|_| "{}".to_string())
    }
}

impl std::fmt::Debug for AnalyticsManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnalyticsManager").finish()
    }
}

