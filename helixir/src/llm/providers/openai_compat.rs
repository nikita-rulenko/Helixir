//! One provider for any OpenAI-compatible `/chat/completions` endpoint.
//!
//! Cerebras and DeepSeek (and most hosted LLMs) speak the identical wire
//! format — `{model, messages, temperature, response_format}` in, `{choices,
//! usage}` out. Only the base URL, auth, model, and a couple of
//! provider-specific knobs differ, so they share this single implementation
//! rather than a near-duplicate file per vendor.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::info;

use super::base::{LlmMetadata, LlmProvider, LlmProviderError};

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
    /// DeepSeek V4 defaults to *thinking* mode, which spends output tokens
    /// and latency on reasoning we never read (the answer is in `content`,
    /// the reasoning in a separate `reasoning_content` field). For
    /// extraction/decision we want clean JSON fast, so we send
    /// `thinking: {type: "disabled"}`. Omitted entirely for providers that
    /// don't understand the field (e.g. Cerebras), keeping their request
    /// byte-identical to before.
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingOptions>,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    r#type: String,
}

#[derive(Debug, Serialize)]
struct ThinkingOptions {
    r#type: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: RespMessage,
}

#[derive(Debug, Deserialize)]
struct RespMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

/// A provider for any OpenAI-compatible chat-completions endpoint.
pub struct OpenAiCompatProvider {
    name: String,
    endpoint: String,
    api_key: String,
    model: String,
    temperature: f64,
    disable_thinking: bool,
    client: Client,
}

impl OpenAiCompatProvider {
    pub fn new(
        name: impl Into<String>,
        endpoint: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        temperature: f64,
        timeout_secs: u64,
        disable_thinking: bool,
    ) -> Self {
        let name = name.into();
        let model = model.into();
        info!("{name} provider initialized (model={model})");
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .unwrap_or_default();
        Self {
            name,
            endpoint: endpoint.into(),
            api_key: api_key.into(),
            model,
            temperature,
            disable_thinking,
            client,
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    async fn generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        response_format: Option<&str>,
    ) -> Result<(String, LlmMetadata), LlmProviderError> {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_prompt.to_string(),
            },
        ];

        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            temperature: self.temperature,
            response_format: response_format.map(|f| ResponseFormat {
                r#type: f.to_string(),
            }),
            thinking: self.disable_thinking.then(|| ThinkingOptions {
                r#type: "disabled".to_string(),
            }),
        };

        let response = self
            .client
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await?
            .error_for_status()
            .map_err(LlmProviderError::Http)?
            .json::<ChatResponse>()
            .await?;

        let content = response
            .choices
            .first()
            .ok_or_else(|| LlmProviderError::Provider("No choices in response".to_string()))?
            .message
            .content
            .clone();

        let mut metadata = LlmMetadata {
            provider: self.name.clone(),
            model: self.model.clone(),
            base_url: Some(self.endpoint.clone()),
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
        &self.name
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
