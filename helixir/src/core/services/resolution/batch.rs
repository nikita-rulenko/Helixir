

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use uuid::Uuid;
use tracing::{debug, info, warn};

use super::error::{BatchResolutionError, BatchResult, ResolutionError};
use crate::db::HelixClient;


pub struct BatchIDResolver {
    
    client: Arc<HelixClient>,
    
    semaphore: Arc<Semaphore>,
    
    retry_attempts: u32,
    
    retry_delay: Duration,
}

impl BatchIDResolver {
    
    
    pub fn new(client: Arc<HelixClient>, max_parallel: usize, retry_attempts: u32) -> Self {
        info!(
            "BatchIDResolver initialized: max_parallel={}, retries={}",
            max_parallel, retry_attempts
        );

        Self {
            client,
            semaphore: Arc::new(Semaphore::new(max_parallel)),
            retry_attempts,
            retry_delay: Duration::from_millis(100),
        }
    }

    
    pub async fn resolve_batch(
        &self,
        memory_ids: &[String],
        fail_fast: bool,
    ) -> Result<BatchResult, BatchResolutionError> {
        debug!(
            "Batch resolve started: {} IDs, fail_fast={}",
            memory_ids.len(),
            fail_fast
        );

        
        let unique_ids: Vec<String> = memory_ids
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if unique_ids.len() < memory_ids.len() {
            debug!(
                "Deduplicated: {} -> {} IDs",
                memory_ids.len(),
                unique_ids.len()
            );
        }

        
        let mut handles = Vec::with_capacity(unique_ids.len());

        for memory_id in unique_ids.iter() {
            let client = self.client.clone();
            let semaphore = self.semaphore.clone();
            let memory_id = memory_id.clone();
            let retry_attempts = self.retry_attempts;
            let retry_delay = self.retry_delay;

            handles.push(tokio::spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();
                Self::resolve_with_retry(&client, &memory_id, retry_attempts, retry_delay).await
            }));
        }

        
        let mut resolved = HashMap::new();
        let mut failed = Vec::new();

        for (idx, handle) in handles.into_iter().enumerate() {
            let memory_id = &unique_ids[idx];

            match handle.await {
                Ok(Ok(uuid)) => {
                    resolved.insert(memory_id.clone(), uuid);
                }
                Ok(Err(e)) => {
                    if fail_fast {
                        return Err(BatchResolutionError::SingleFailure {
                            memory_id: memory_id.clone(),
                            error: e.to_string(),
                        });
                    }
                    failed.push((memory_id.clone(), e.to_string()));
                }
                Err(e) => {
                    
                    if fail_fast {
                        return Err(BatchResolutionError::SingleFailure {
                            memory_id: memory_id.clone(),
                            error: format!("Task panic: {}", e),
                        });
                    }
                    failed.push((memory_id.clone(), format!("Task panic: {}", e)));
                }
            }
        }

        info!(
            "Batch resolve complete: {}/{} resolved, {} failed",
            resolved.len(),
            unique_ids.len(),
            failed.len()
        );

        Ok(BatchResult { resolved, failed })
    }

    
    async fn resolve_with_retry(
        client: &HelixClient,
        memory_id: &str,
        max_attempts: u32,
        base_delay: Duration,
    ) -> Result<Uuid, ResolutionError> {
        let mut last_error = None;

        for attempt in 0..max_attempts {
            match Self::query_db(client, memory_id).await {
                Ok(uuid) => {
                    if attempt > 0 {
                        debug!("Retry succeeded for {} on attempt {}", memory_id, attempt + 1);
                    }
                    return Ok(uuid);
                }
                Err(e) => {
                    last_error = Some(e);

                    if attempt < max_attempts - 1 {
                        
                        let delay = base_delay * (2_u32.pow(attempt));
                        debug!(
                            "Retry {} for {} after {:?}",
                            attempt + 1,
                            memory_id,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        warn!(
            "All {} retries exhausted for {}",
            max_attempts, memory_id
        );

        Err(last_error.unwrap_or(ResolutionError::Database("Unknown error".to_string())))
    }

    
    async fn query_db(client: &HelixClient, memory_id: &str) -> Result<Uuid, ResolutionError> {
        #[derive(serde::Serialize)]
        struct Input<'a> {
            memory_id: &'a str,
        }

        #[derive(serde::Deserialize)]
        struct Output {
            id: Option<String>,
        }

        let result: Output = client
            .execute_query("getMemory", &Input { memory_id })
            .await
            .map_err(|e| ResolutionError::Database(e.to_string()))?;

        let id_str = result
            .id
            .ok_or_else(|| ResolutionError::NotFound(memory_id.to_string()))?;

        Uuid::parse_str(&id_str).map_err(|e| ResolutionError::InvalidUuid(e.to_string()))
    }
}