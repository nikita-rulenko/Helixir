

use std::collections::HashMap;
use std::sync::Arc;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::db::HelixClient;
use super::models::Memory;


#[derive(Error, Debug)]
pub enum ContextError {
    #[error("Context not found: {0}")]
    NotFound(String),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Database error: {0}")]
    Database(String),
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextDef {
    pub context_id: String,
    pub name: String,
    pub properties: HashMap<String, serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

impl ContextDef {
    
    pub fn new(name: String, properties: Option<HashMap<String, serde_json::Value>>) -> Self {
        Self {
            context_id: format!("ctx_{}", Uuid::new_v4().to_string().replace("-", "")[..12].to_string()),
            name,
            properties: properties.unwrap_or_default(),
            created_at: Utc::now(),
        }
    }
}


pub struct ContextManager {
    client: Arc<HelixClient>,
    
    context_cache: RwLock<HashMap<String, ContextDef>>,
    
    active_contexts: RwLock<HashMap<String, Vec<String>>>,
    cache_size: usize,
    is_warmed_up: bool,
}

impl ContextManager {
    
    pub fn new(client: Arc<HelixClient>, cache_size: usize) -> Self {
        info!("ContextManager initialized (cache_size={})", cache_size);
        Self {
            client,
            context_cache: RwLock::new(HashMap::new()),
            active_contexts: RwLock::new(HashMap::new()),
            cache_size,
            is_warmed_up: false,
        }
    }

    
    fn add_to_cache(&self, context: ContextDef) {
        let mut cache = self.context_cache.write();
        
        
        if cache.len() >= self.cache_size {
            if let Some(oldest_key) = cache.keys().next().cloned() {
                cache.remove(&oldest_key);
                debug!("Cache eviction: removed context {}", crate::safe_truncate(&oldest_key, 8));
            }
        }

        cache.insert(context.context_id.clone(), context);
    }

    
    pub async fn warm_up_cache(&mut self, user_id: Option<&str>, limit: usize) -> Result<usize, ContextError> {
        if self.is_warmed_up {
            info!("Context cache already warmed up, skipping");
            return Ok(self.context_cache.read().len());
        }

        info!("Warming up context cache (user={:?}, limit={})", user_id, limit);

        #[derive(Serialize)]
        struct WarmupParams {
            limit: usize,
            #[serde(skip_serializing_if = "Option::is_none")]
            user_id: Option<String>,
        }

        let params = WarmupParams {
            limit,
            user_id: user_id.map(String::from),
        };

        match self.client.execute_query::<Vec<ContextDef>, _>("getRecentContexts", &params).await {
            Ok(contexts) => {
                for context in contexts {
                    self.add_to_cache(context);
                }
                self.is_warmed_up = true;
                let count = self.context_cache.read().len();
                info!("Context cache warm-up complete: {} contexts loaded", count);
                Ok(count)
            }
            Err(e) => {
                warn!("Context cache warm-up failed: {}, continuing with empty cache", e);
                Ok(0)
            }
        }
    }

    
    pub async fn create_context(
        &self,
        name: &str,
        properties: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<ContextDef, ContextError> {
        if name.trim().is_empty() {
            return Err(ContextError::Validation("Context name cannot be empty".into()));
        }

        let context = ContextDef::new(name.to_string(), properties);

        #[derive(Serialize)]
        struct AddContextParams {
            context_id: String,
            name: String,
            properties: String,
            created_at: String,
        }

        let params = AddContextParams {
            context_id: context.context_id.clone(),
            name: context.name.clone(),
            properties: serde_json::to_string(&context.properties).unwrap_or_default(),
            created_at: context.created_at.to_rfc3339(),
        };

        match self.client.execute_query::<(), _>("addContext", &params).await {
            Ok(_) => {
                self.add_to_cache(context.clone());
                info!("Created context: {} ({})", context.name, crate::safe_truncate(&context.context_id, 8));
                Ok(context)
            }
            Err(e) => {
                
                warn!("Failed to persist context to HelixDB: {}, adding to cache only", e);
                self.add_to_cache(context.clone());
                Ok(context)
            }
        }
    }

    
    pub async fn get_context(&self, context_id: &str) -> Result<Option<ContextDef>, ContextError> {
        
        if let Some(context) = self.context_cache.read().get(context_id).cloned() {
            debug!("Cache HIT: {}", context_id);
            return Ok(Some(context));
        }

        
        debug!("Cache MISS: {}, querying HelixDB", context_id);

        #[derive(Serialize)]
        struct GetParams {
            context_id: String,
        }

        match self.client.execute_query::<Option<ContextDef>, _>(
            "getContext",
            &GetParams { context_id: context_id.to_string() },
        ).await {
            Ok(Some(context)) => {
                self.add_to_cache(context.clone());
                debug!("Loaded from DB and cached: {}", context_id);
                Ok(Some(context))
            }
            Ok(None) => Ok(None),
            Err(e) => {
                warn!("Failed to query HelixDB for context {}: {}", context_id, e);
                Ok(None)
            }
        }
    }

    
    pub async fn get_context_by_name(&self, name: &str) -> Option<ContextDef> {
        
        let cache = self.context_cache.read();
        for context in cache.values() {
            if context.name.eq_ignore_ascii_case(name) {
                return Some(context.clone());
            }
        }
        drop(cache);

        
        #[derive(Serialize)]
        struct GetByNameParams {
            name: String,
        }

        match self.client.execute_query::<Option<ContextDef>, _>(
            "getContextByName",
            &GetByNameParams { name: name.to_string() },
        ).await {
            Ok(Some(context)) => {
                self.add_to_cache(context.clone());
                Some(context)
            }
            _ => None,
        }
    }

    
    pub async fn link_memory_to_context(
        &self,
        memory_id: &str,
        context_id: &str,
        priority: i32,
    ) -> Result<bool, ContextError> {
        if !(0..=100).contains(&priority) {
            return Err(ContextError::Validation(format!(
                "Priority must be between 0 and 100, got {}",
                priority
            )));
        }

        #[derive(Serialize)]
        struct LinkParams {
            memory_id: String,
            context_id: String,
            priority: i32,
        }

        match self.client.execute_query::<(), _>(
            "linkMemoryToContext",
            &LinkParams {
                memory_id: memory_id.to_string(),
                context_id: context_id.to_string(),
                priority,
            },
        ).await {
            Ok(_) => {
                debug!("Linked memory {} to context {}", crate::safe_truncate(memory_id, 8), crate::safe_truncate(context_id, 8));
                Ok(true)
            }
            Err(e) => {
                warn!("Failed to link memory to context: {}", e);
                Ok(false)
            }
        }
    }

    
    pub fn activate_context(&self, user_id: &str, context_id: &str) -> bool {
        let mut active = self.active_contexts.write();
        let user_contexts = active.entry(user_id.to_string()).or_insert_with(Vec::new);
        
        if !user_contexts.contains(&context_id.to_string()) {
            user_contexts.push(context_id.to_string());
        }
        
        info!("Activated context {} for user {}", context_id, user_id);
        true
    }

    
    pub fn deactivate_context(&self, user_id: &str, context_id: &str) -> bool {
        let mut active = self.active_contexts.write();
        
        if let Some(user_contexts) = active.get_mut(user_id) {
            user_contexts.retain(|c| c != context_id);
            info!("Deactivated context {} for user {}", context_id, user_id);
            return true;
        }
        
        false
    }

    
    pub fn get_active_contexts(&self, user_id: &str) -> Vec<String> {
        self.active_contexts
            .read()
            .get(user_id)
            .cloned()
            .unwrap_or_default()
    }

    
    pub fn filter_by_context(
        &self,
        memories: Vec<Memory>,
        context_names: &[String],
        match_all: bool,
    ) -> Vec<Memory> {
        if context_names.is_empty() {
            return memories;
        }

        let context_names_lower: Vec<String> = context_names
            .iter()
            .map(|c| c.to_lowercase())
            .collect();

        memories
            .into_iter()
            .filter(|memory| {
                
                let memory_contexts: Vec<String> = if memory.context_tags.is_empty() {
                    Vec::new()
                } else {
                    
                    if let Ok(parsed) = serde_json::from_str::<HashMap<String, serde_json::Value>>(&memory.context_tags) {
                        parsed.keys().map(|k| k.to_lowercase()).collect()
                    } else {
                        vec![memory.context_tags.to_lowercase()]
                    }
                };

                if match_all {
                    
                    context_names_lower.iter().all(|ctx| memory_contexts.contains(ctx))
                } else {
                    
                    context_names_lower.iter().any(|ctx| memory_contexts.contains(ctx))
                }
            })
            .collect()
    }

    
    pub fn calculate_context_relevance(
        &self,
        memory: &Memory,
        active_contexts: &[String],
    ) -> f64 {
        if active_contexts.is_empty() {
            return 1.0; 
        }

        let memory_contexts: Vec<String> = if memory.context_tags.is_empty() {
            Vec::new()
        } else {
            if let Ok(parsed) = serde_json::from_str::<HashMap<String, serde_json::Value>>(&memory.context_tags) {
                parsed.keys().map(|k| k.to_lowercase()).collect()
            } else {
                vec![memory.context_tags.to_lowercase()]
            }
        };

        if memory_contexts.is_empty() {
            return 0.5; 
        }

        
        let matches = active_contexts
            .iter()
            .filter(|ctx| memory_contexts.contains(&ctx.to_lowercase()))
            .count();

        matches as f64 / active_contexts.len() as f64
    }

    
    pub fn cached_count(&self) -> usize {
        self.context_cache.read().len()
    }
}

impl std::fmt::Debug for ContextManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextManager")
            .field("cached_contexts", &self.context_cache.read().len())
            .field("active_users", &self.active_contexts.read().len())
            .finish()
    }
}

