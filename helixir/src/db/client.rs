

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use helix_rs::{HelixDB, HelixDBClient, HelixError};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use tracing::{debug, error, info, warn};


const MAX_RETRIES: u32 = 3;

const INITIAL_RETRY_DELAY_MS: u64 = 100;

const MAX_RETRY_DELAY_MS: u64 = 10000;


#[derive(Debug, Error)]
pub enum HelixClientError {
    #[error("Connection failed: {0}")]
    Connection(String),
    #[error("Query failed: {0}")]
    Query(String),
    #[error("Helix error: {0}")]
    Helix(#[from] HelixError),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Client not connected")]
    NotConnected,
    #[error("Retry exhausted after {0} attempts: {1}")]
    RetryExhausted(u32, String),
}


pub struct HelixClient {
    
    inner: HelixDB,
    
    is_connected: AtomicBool,
    
    base_url: String,
}

impl HelixClient {
    
    pub fn new(host: &str, port: u16) -> Result<Self, HelixClientError> {
        
        let endpoint = format!("http://{}", host);
        let base_url = format!("http://{}:{}", host, port);
        
        
        let inner = <HelixDB as HelixDBClient>::new(
            Some(&endpoint),
            Some(port),
            None, 
        );

        info!("HelixClient created for {}", base_url);

        Ok(Self {
            inner,
            is_connected: AtomicBool::new(false),
            base_url,
        })
    }

    
    pub fn from_env() -> Result<Self, HelixClientError> {
        let host = std::env::var("HELIX_HOST").unwrap_or_else(|_| "localhost".to_string());
        let port: u16 = std::env::var("HELIX_PORT")
            .unwrap_or_else(|_| "6969".to_string())
            .parse()
            .unwrap_or(6969);

        Self::new(&host, port)
    }

    
    pub async fn connect(&self) -> Result<(), HelixClientError> {
        if self.is_connected.load(Ordering::Relaxed) {
            return Ok(());
        }

        
                self.is_connected.store(true, Ordering::Relaxed);
        info!("HelixClient ready for {}", self.base_url);
                    Ok(())
    }

    
    pub async fn execute_query<T, P>(&self, query_name: &str, params: &P) -> Result<T, HelixClientError>
    where
        T: DeserializeOwned,
        P: Serialize + Sync,
    {
        let mut last_error = None;
        let mut delay = Duration::from_millis(INITIAL_RETRY_DELAY_MS);

        for attempt in 1..=MAX_RETRIES {
            debug!("Executing query: {} (attempt {})", query_name, attempt);

            match self.inner.query::<P, T>(query_name, params).await {
                Ok(result) => {
                    if !self.is_connected.load(Ordering::Relaxed) {
                        self.is_connected.store(true, Ordering::Relaxed);
                    }
                    debug!("Query {} succeeded", query_name);
                    return Ok(result);
                }
                Err(e) => {
                    let err_str = e.to_string();
                    
                    
                    if err_str.contains("not found") || err_str.contains("No value") {
                        debug!("Query {} returned not found (expected)", query_name);
                        return Err(HelixClientError::Query(err_str));
                    }

                    
                    if attempt == 1 {
                    debug!("Query {} failed (attempt {}), retrying: {}", query_name, attempt, e);
                } else {
                    debug!("Query {} failed (final attempt {}): {}", query_name, attempt, e);
                }
                last_error = Some(err_str);

                    if attempt < MAX_RETRIES {
                        tokio::time::sleep(delay).await;
                        
                        delay = (delay * 2).min(Duration::from_millis(MAX_RETRY_DELAY_MS));
                    }
                }
            }
        }

        Err(HelixClientError::RetryExhausted(
            MAX_RETRIES,
            last_error.unwrap_or_else(|| "Unknown error".to_string()),
        ))
    }

    
    pub async fn execute_query_no_retry<T, P>(&self, query_name: &str, params: &P) -> Result<T, HelixClientError>
    where
        T: DeserializeOwned,
        P: Serialize + Sync,
    {
        self.inner
            .query::<P, T>(query_name, params)
            .await
            .map_err(|e| HelixClientError::Query(e.to_string()))
    }

    
    pub async fn health_check(&self) -> Result<(), HelixClientError> {
        
        
        match self.execute_query_no_retry::<serde_json::Value, _>("health", &serde_json::json!({})).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let err_str = e.to_string().to_lowercase();
                
                if err_str.contains("404") 
                    || err_str.contains("not found") 
                    || err_str.contains("couldn't find")
                {
                    info!("Health check passed (server alive, no health query)");
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    
    pub fn inner(&self) -> &HelixDB {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = HelixClient::new("localhost", 6969);
        assert!(client.is_ok());
    }

    #[test]
    fn test_client_from_env() {
        std::env::set_var("HELIX_HOST", "localhost");
        std::env::set_var("HELIX_PORT", "6969");
        
        let client = HelixClient::from_env();
        assert!(client.is_ok());
    }
}
