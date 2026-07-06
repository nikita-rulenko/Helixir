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

    /// Build the request body. Split out from `generate` so the wire shape —
    /// especially the `thinking: {type: disabled}` knob that differs per
    /// provider — is unit-testable without an HTTP round-trip.
    fn build_request(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        response_format: Option<&str>,
    ) -> ChatRequest {
        ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_prompt.to_string(),
                },
            ],
            temperature: self.temperature,
            response_format: response_format.map(|f| ResponseFormat {
                r#type: f.to_string(),
            }),
            thinking: self.disable_thinking.then(|| ThinkingOptions {
                r#type: "disabled".to_string(),
            }),
        }
    }

    /// Extract content + token metadata from a decoded response. Split out so
    /// the empty-`choices` error path is unit-testable without a live server.
    fn parse_response(
        &self,
        response: ChatResponse,
    ) -> Result<(String, LlmMetadata), LlmProviderError> {
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
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    async fn generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        response_format: Option<&str>,
    ) -> Result<(String, LlmMetadata), LlmProviderError> {
        let request = self.build_request(system_prompt, user_prompt, response_format);

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

        let (content, metadata) = self.parse_response(response)?;
        // #96 cost instrument: exactly one line per REAL LLM call, with token
        // counts, so the write-path LLM cost is measurable before/after the
        // batching optimization. Filter with RUST_LOG=helixir::llm::cost=info.
        tracing::info!(
            target: "helixir::llm::cost",
            "llm_call provider={} model={} ptok={} ctok={}",
            self.name,
            self.model,
            metadata.tokens_prompt.unwrap_or(0),
            metadata.tokens_completion.unwrap_or(0)
        );
        Ok((content, metadata))
    }

    fn provider_name(&self) -> &str {
        &self.name
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deepseek() -> OpenAiCompatProvider {
        OpenAiCompatProvider::new(
            "deepseek",
            "https://api.deepseek.com/chat/completions",
            "k",
            "deepseek-v4-flash",
            0.1,
            60,
            true, // disable_thinking
        )
    }
    fn cerebras() -> OpenAiCompatProvider {
        OpenAiCompatProvider::new(
            "cerebras",
            "https://api.cerebras.ai/v1/chat/completions",
            "k",
            "gpt-oss-120b",
            0.3,
            60,
            false,
        )
    }

    #[test]
    fn deepseek_request_disables_thinking() {
        let req = deepseek().build_request("sys", "usr", Some("json_object"));
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "deepseek-v4-flash");
        assert_eq!(v["thinking"]["type"], "disabled");
        assert_eq!(v["response_format"]["type"], "json_object");
        assert_eq!(v["messages"][0]["role"], "system");
        assert_eq!(v["messages"][1]["content"], "usr");
    }

    #[test]
    fn cerebras_request_omits_thinking_and_format() {
        // Cerebras must NOT receive the DeepSeek-specific `thinking` field, and
        // `response_format` is omitted when not requested.
        let req = cerebras().build_request("sys", "usr", None);
        let v = serde_json::to_value(&req).unwrap();
        assert!(
            v.get("thinking").is_none(),
            "cerebras must not send thinking"
        );
        assert!(v.get("response_format").is_none());
        assert_eq!(v["temperature"], 0.3);
    }

    #[test]
    fn parse_response_extracts_content_and_usage() {
        let resp: ChatResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"content":"hello"}}],
                "usage":{"prompt_tokens":3,"completion_tokens":5,"total_tokens":8}}"#,
        )
        .unwrap();
        let (content, meta) = cerebras().parse_response(resp).unwrap();
        assert_eq!(content, "hello");
        assert_eq!(meta.provider, "cerebras");
        assert_eq!(meta.model, "gpt-oss-120b");
        assert_eq!(meta.tokens_total, Some(8));
    }

    #[test]
    fn parse_response_empty_choices_is_clean_error() {
        let resp: ChatResponse = serde_json::from_str(r#"{"choices":[]}"#).unwrap();
        let err = cerebras().parse_response(resp).unwrap_err();
        // must be a structured provider error, never a panic
        assert!(matches!(err, LlmProviderError::Provider(_)));
    }
}
