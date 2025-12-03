

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::info;

use super::base::{LlmMetadata, LlmProvider, LlmProviderError};

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    options: OllamaOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    temperature: f64,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
    #[serde(default)]
    prompt_eval_count: u32,
    #[serde(default)]
    eval_count: u32,
    #[serde(default)]
    total_duration: u64,
}


pub struct OllamaProvider {
    base_url: String,
    model: String,
    temperature: f64,
    client: Client,
}

impl OllamaProvider {
    
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        temperature: f64,
    ) -> Self {
        let base_url = base_url.into();
        let model = model.into();
        info!("Ollama provider initialized (model={}, url={})", model, base_url);
        Self {
            base_url,
            model,
            temperature,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    
    pub fn localhost(model: impl Into<String>, temperature: f64) -> Self {
        Self::new("http://localhost:11434", model, temperature)
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        response_format: Option<&str>,
    ) -> Result<(String, LlmMetadata), LlmProviderError> {
        let messages = vec![
            OllamaMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            OllamaMessage {
                role: "user".to_string(),
                content: user_prompt.to_string(),
            },
        ];

        let format = if response_format == Some("json_object") {
            Some("json".to_string())
        } else {
            None
        };

        let request = OllamaRequest {
            model: self.model.clone(),
            messages,
            stream: false,
            options: OllamaOptions {
                temperature: self.temperature,
            },
            format,
        };

        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&request)
            .send()
            .await?
            .error_for_status()
            .map_err(LlmProviderError::Http)?
            .json::<OllamaResponse>()
            .await?;

        let content = response.message.content;

        let metadata = LlmMetadata {
            provider: "ollama".to_string(),
            model: self.model.clone(),
            base_url: Some(self.base_url.clone()),
            tokens_prompt: Some(response.prompt_eval_count),
            tokens_completion: Some(response.eval_count),
            tokens_total: Some(response.prompt_eval_count + response.eval_count),
            ..Default::default()
        };

        Ok((content, metadata))
    }

    fn provider_name(&self) -> &str {
        "ollama"
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
