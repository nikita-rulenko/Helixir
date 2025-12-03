

use std::sync::Arc;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::db::HelixClient;
use crate::toolkit::mind_toolbox::reasoning::{ReasoningEngine, ReasoningType, ReasoningError};


#[derive(Error, Debug)]
pub enum EvolutionError {
    #[error("Memory not found: {0}")]
    MemoryNotFound(String),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Reasoning error: {0}")]
    Reasoning(#[from] ReasoningError),
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionResult {
    pub success: bool,
    pub old_memory_id: String,
    pub new_memory_id: Option<String>,
    pub operation: String,
    pub edge_created: bool,
    pub timestamp: DateTime<Utc>,
}


pub struct MemoryEvolution {
    client: Arc<HelixClient>,
    reasoning_engine: Arc<ReasoningEngine>,
}

impl MemoryEvolution {
    
    pub fn new(client: Arc<HelixClient>, reasoning_engine: Arc<ReasoningEngine>) -> Self {
        info!("MemoryEvolution initialized");
        Self {
            client,
            reasoning_engine,
        }
    }

    
    pub async fn handle_supersession(
        &self,
        old_memory_id: &str,
        new_memory_id: &str,
        reason: Option<&str>,
        _changed_by: Option<&str>,
    ) -> Result<EvolutionResult, EvolutionError> {
        info!(
            "Handling supersession: {} supersedes {}",
            crate::safe_truncate(new_memory_id, 12),
            crate::safe_truncate(old_memory_id, 12)
        );

        
        debug!("Setting temporal boundary on old memory: {}", old_memory_id);
        
        let now = Utc::now();
        
        #[derive(Serialize)]
        struct UpdateValidUntil {
            memory_id: String,
            valid_until: String,
        }

        self.client
            .execute_query::<(), _>(
                "updateMemoryValidUntil",
                &UpdateValidUntil {
                    memory_id: old_memory_id.to_string(),
                    valid_until: now.to_rfc3339(),
                },
            )
            .await
            .map_err(|e| EvolutionError::Database(e.to_string()))?;

        
        debug!(
            "Creating SUPERSEDES edge: {} → {}",
            new_memory_id, old_memory_id
        );

        let edge_created = match self
            .reasoning_engine
            .add_relation(
                new_memory_id,
                old_memory_id,
                ReasoningType::Supports, 
                95, 
                None,
            )
            .await
        {
            Ok(_) => {
                debug!("SUPERSEDES edge created successfully");
                true
            }
            Err(e) => {
                warn!("Failed to create SUPERSEDES edge: {}", e);
                false
            }
        };

        info!(
            "✅ Memory supersession complete: {} supersedes {}",
            crate::safe_truncate(new_memory_id, 12),
            crate::safe_truncate(old_memory_id, 12)
        );

        Ok(EvolutionResult {
            success: true,
            old_memory_id: old_memory_id.to_string(),
            new_memory_id: Some(new_memory_id.to_string()),
            operation: "supersession".to_string(),
            edge_created,
            timestamp: now,
        })
    }

    
    pub async fn handle_contradiction(
        &self,
        existing_memory_id: &str,
        new_memory_id: &str,
        _explanation: Option<&str>,
        confidence: i32,
    ) -> Result<EvolutionResult, EvolutionError> {
        info!(
            "Handling contradiction: {} ⇄ {}",
            crate::safe_truncate(new_memory_id, 12),
            crate::safe_truncate(existing_memory_id, 12)
        );

        let now = Utc::now();

        
        let edge1 = self
            .reasoning_engine
            .add_relation(
                new_memory_id,
                existing_memory_id,
                ReasoningType::Contradicts,
                confidence,
                None,
            )
            .await;

        
        let edge2 = self
            .reasoning_engine
            .add_relation(
                existing_memory_id,
                new_memory_id,
                ReasoningType::Contradicts,
                confidence,
                None,
            )
            .await;

        let edge_created = edge1.is_ok() && edge2.is_ok();

        if !edge_created {
            warn!(
                "⚠️ Some CONTRADICTS edges failed: edge1={:?}, edge2={:?}",
                edge1.is_ok(),
                edge2.is_ok()
            );
        }

        warn!(
            "⚠️ Memory contradiction detected and logged: {} ⇄ {}",
            crate::safe_truncate(new_memory_id, 12),
            crate::safe_truncate(existing_memory_id, 12)
        );

        Ok(EvolutionResult {
            success: true,
            old_memory_id: existing_memory_id.to_string(),
            new_memory_id: Some(new_memory_id.to_string()),
            operation: "contradiction".to_string(),
            edge_created,
            timestamp: now,
        })
    }

    
    pub async fn handle_enhancement(
        &self,
        memory_id: &str,
        enhanced_content: &str,
        _enhanced_by: Option<&str>,
    ) -> Result<EvolutionResult, EvolutionError> {
        info!(
            "Enhancing memory: {}",
            crate::safe_truncate(memory_id, 12)
        );

        let now = Utc::now();

        #[derive(Serialize)]
        struct UpdateContent {
            memory_id: String,
            content: String,
            updated_at: String,
        }

        self.client
            .execute_query::<(), _>(
                "updateMemoryContent",
                &UpdateContent {
                    memory_id: memory_id.to_string(),
                    content: enhanced_content.to_string(),
                    updated_at: now.to_rfc3339(),
                },
            )
            .await
            .map_err(|e| EvolutionError::Database(e.to_string()))?;

        info!(
            "✅ Memory enhanced: {}",
            crate::safe_truncate(memory_id, 12)
        );

        Ok(EvolutionResult {
            success: true,
            old_memory_id: memory_id.to_string(),
            new_memory_id: None, 
            operation: "enhancement".to_string(),
            edge_created: false, 
            timestamp: now,
        })
    }
}

impl std::fmt::Debug for MemoryEvolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryEvolution").finish()
    }
}

