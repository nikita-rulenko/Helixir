

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;


#[derive(Error, Debug)]
pub enum LlmProviderError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON parsing failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Provider error: {0}")]
    Provider(String),

    #[error("Internal error: {0}")]
    Internal(String),
}


#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmMetadata {
    pub provider: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_prompt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_completion: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_total: Option<u32>,
    #[serde(default)]
    pub fallback_used: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_error: Option<String>,
}


#[async_trait]
pub trait LlmProvider: Send + Sync {
    
    async fn generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        response_format: Option<&str>,
    ) -> Result<(String, LlmMetadata), LlmProviderError>;

    
    fn provider_name(&self) -> &str;

    
    fn model_name(&self) -> &str;
}


#[async_trait]
impl LlmProvider for Arc<dyn LlmProvider> {
    async fn generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        response_format: Option<&str>,
    ) -> Result<(String, LlmMetadata), LlmProviderError> {
        (**self).generate(system_prompt, user_prompt, response_format).await
    }

    fn provider_name(&self) -> &str {
        (**self).provider_name()
    }

    fn model_name(&self) -> &str {
        (**self).model_name()
    }
}
