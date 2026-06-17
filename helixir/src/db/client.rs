use helix_rs::{HelixDB, HelixDBClient, HelixError};
use serde::{Serialize, de::DeserializeOwned};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info};

use crate::core::config::RetryConfig;

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

    retry: RetryConfig,
}

impl HelixClient {
    pub fn new(host: &str, port: u16) -> Result<Self, HelixClientError> {
        let endpoint = format!("http://{}", host);
        let base_url = format!("http://{}:{}", host, port);

        let inner = <HelixDB as HelixDBClient>::new(Some(&endpoint), Some(port), None);

        info!("HelixClient created for {}", base_url);

        Ok(Self {
            inner,
            is_connected: AtomicBool::new(false),
            base_url,
            retry: RetryConfig::default(),
        })
    }

    /// Override the query retry policy (defaults to [`RetryConfig::default`]).
    #[must_use]
    pub fn with_retry(mut self, retry: RetryConfig) -> Self {
        self.retry = retry;
        self
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

    pub async fn execute_query<T, P>(
        &self,
        query_name: &str,
        params: &P,
    ) -> Result<T, HelixClientError>
    where
        T: DeserializeOwned,
        P: Serialize + Sync,
    {
        let mut last_error = None;
        let mut delay = Duration::from_millis(self.retry.initial_delay_ms);
        let max_retries = self.retry.max;

        for attempt in 1..=max_retries {
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
                        debug!(
                            "Query {} failed (attempt {}), retrying: {}",
                            query_name, attempt, e
                        );
                    } else {
                        debug!(
                            "Query {} failed (final attempt {}): {}",
                            query_name, attempt, e
                        );
                    }
                    last_error = Some(err_str);

                    if attempt < max_retries {
                        tokio::time::sleep(delay).await;

                        delay = (delay * self.retry.backoff_factor as u32)
                            .min(Duration::from_millis(self.retry.max_delay_ms));
                    }
                }
            }
        }

        Err(HelixClientError::RetryExhausted(
            max_retries,
            last_error.unwrap_or_else(|| "Unknown error".to_string()),
        ))
    }

    pub async fn execute_query_no_retry<T, P>(
        &self,
        query_name: &str,
        params: &P,
    ) -> Result<T, HelixClientError>
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
        match self
            .execute_query_no_retry::<serde_json::Value, _>("health", &serde_json::json!({}))
            .await
        {
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
        unsafe {
            std::env::set_var("HELIX_HOST", "localhost");
        }
        unsafe {
            std::env::set_var("HELIX_PORT", "6969");
        }

        let client = HelixClient::from_env();
        assert!(client.is_ok());
    }
}
