

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::info;

use super::base::{LlmMetadata, LlmProvider, LlmProviderError};

#[derive(Debug, Serialize)]
struct CerebrasRequest {
    model: String,
    messages: Vec<CerebrasMessage>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CerebrasMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    r#type: String,
}

#[derive(Debug, Deserialize)]
struct CerebrasResponse {
    choices: Vec<CerebrasChoice>,
    usage: Option<CerebrasUsage>,
}

#[derive(Debug, Deserialize)]
struct CerebrasChoice {
    message: CerebrasMessage,
}

#[derive(Debug, Deserialize)]
struct CerebrasUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}


pub struct CerebrasProvider {
    api_key: String,
    model: String,
    temperature: f64,
    client: Client,
}

impl CerebrasProvider {
    
    pub fn new(api_key: impl Into<String>, model: impl Into<String>, temperature: f64) -> Self {
        let model = model.into();
        info!("Cerebras provider initialized (model={})", model);
        Self {
            api_key: api_key.into(),
            model,
            temperature,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for CerebrasProvider {
    async fn generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        response_format: Option<&str>,
    ) -> Result<(String, LlmMetadata), LlmProviderError> {
        let messages = vec![
            CerebrasMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            CerebrasMessage {
                role: "user".to_string(),
                content: user_prompt.to_string(),
            },
        ];

        let format = response_format.map(|f| ResponseFormat {
            r#type: f.to_string(),
        });

        let request = CerebrasRequest {
            model: self.model.clone(),
            messages,
            temperature: self.temperature,
            response_format: format,
        };

        let response = self
            .client
            .post("https://api.cerebras.ai/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await?
            .error_for_status()
            .map_err(LlmProviderError::Http)?
            .json::<CerebrasResponse>()
            .await?;

        let content = response
            .choices
            .first()
            .ok_or_else(|| LlmProviderError::Provider("No choices in response".to_string()))?
            .message
            .content
            .clone();

        let mut metadata = LlmMetadata {
            provider: "cerebras".to_string(),
            model: self.model.clone(),
            base_url: Some("https://api.cerebras.ai/v1".to_string()),
            ..Default::default()
        };

        if let Some(usage) = response.usage {
            metadata.tokens_prompt = Some(usage.prompt_tokens);
            metadata.tokens_completion = Some(usage.completion_tokens);
            metadata.tokens_total = Some(usage.total_tokens);
        }

        Ok((content, metadata))
    }

    fn provider_name(&self) -> &str {
        "cerebras"
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
