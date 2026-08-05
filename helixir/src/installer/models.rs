//! Model-runtime adapters used by the onboarding planner.
//!
//! The adapter emits argv vectors rather than shell snippets.  This keeps the
//! interactive CLI and a future native UI on the same safe command contract.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;

/// A command that can be displayed, tested, and executed without a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// Executable resolved from PATH or an absolute path.
    pub program: String,
    /// Arguments passed verbatim to the executable.
    pub args: Vec<String>,
}

impl CommandSpec {
    /// Construct a command specification.
    #[must_use]
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

/// Ollama command adapter.
pub struct OllamaAdapter;

impl OllamaAdapter {
    /// Probe command used to detect an installed Ollama binary.
    #[must_use]
    pub fn version() -> CommandSpec {
        CommandSpec::new("ollama", ["--version"])
    }

    /// List locally available models.
    #[must_use]
    pub fn list() -> CommandSpec {
        CommandSpec::new("ollama", ["list"])
    }

    /// Start the local API service.
    #[must_use]
    pub fn serve() -> CommandSpec {
        CommandSpec::new("ollama", ["serve"])
    }

    /// Pull one model by its exact user-selected name.
    #[must_use]
    pub fn pull(model: &str) -> CommandSpec {
        CommandSpec::new("ollama", ["pull", model])
    }

    /// Parse the tabular `ollama list` output without trusting model metadata.
    #[must_use]
    pub fn parse_models(output: &str) -> BTreeSet<String> {
        output
            .lines()
            .skip(1)
            .filter_map(|line| line.split_whitespace().next())
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Resolve the Ollama CLI from `PATH` or a standard native app location.
    #[must_use]
    pub fn resolve_binary(home: Option<&Path>) -> Option<PathBuf> {
        let executable = if cfg!(windows) {
            "ollama.exe"
        } else {
            "ollama"
        };
        if let Some(paths) = std::env::var_os("PATH") {
            for directory in std::env::split_paths(&paths) {
                let candidate = directory.join(executable);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }

        let mut candidates = vec![PathBuf::from(
            "/Applications/Ollama.app/Contents/Resources/ollama",
        )];
        if let Some(home) = home {
            candidates.push(home.join("Applications/Ollama.app/Contents/Resources/ollama"));
            #[cfg(windows)]
            candidates.push(home.join("AppData/Local/Programs/Ollama/ollama.exe"));
        }
        candidates.into_iter().find(|candidate| candidate.is_file())
    }

    /// Treat Ollama's implicit `:latest` tag as the same selected model.
    #[must_use]
    pub fn has_model(models: &BTreeSet<String>, requested: &str) -> bool {
        let requested = requested.trim();
        if requested.is_empty() {
            return false;
        }
        models.contains(requested)
            || (!requested.contains(':') && models.contains(&format!("{requested}:latest")))
    }

    /// Query the official local API for installed models.
    pub async fn list_api(base_url: &str) -> Result<BTreeSet<String>> {
        let response = api_client(Duration::from_secs(15))
            .get(format!("{}/api/tags", base_url.trim_end_matches('/')))
            .send()
            .await
            .context("query Ollama /api/tags")?
            .error_for_status()
            .context("Ollama /api/tags returned an error")?
            .json::<TagsResponse>()
            .await
            .context("decode Ollama /api/tags")?;
        Ok(response
            .models
            .into_iter()
            .filter_map(|model| model.model.or(model.name))
            .collect())
    }

    /// Return true only when the local Ollama API answers successfully.
    pub async fn api_ready(base_url: &str) -> bool {
        Self::list_api(base_url).await.is_ok()
    }

    /// Wait for a newly started Ollama service to expose its API.
    pub async fn wait_until_ready(base_url: &str, timeout: Duration) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;
        while tokio::time::Instant::now() < deadline {
            if Self::api_ready(base_url).await {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        anyhow::bail!("Ollama API did not become ready at {base_url} within {timeout:?}")
    }

    /// Pull one model through Ollama's official API, retry interrupted transfers,
    /// and verify the final model list before reporting success.
    pub async fn pull_and_verify(base_url: &str, model: &str, attempts: usize) -> Result<()> {
        let model = model.trim();
        anyhow::ensure!(!model.is_empty(), "Ollama model name cannot be empty");
        let attempts = attempts.max(1);
        let client = api_client(Duration::from_secs(60 * 60 * 6));
        let endpoint = format!("{}/api/pull", base_url.trim_end_matches('/'));
        let mut last_error = None;

        for attempt in 1..=attempts {
            if Self::list_api(base_url)
                .await
                .map(|models| Self::has_model(&models, model))
                .unwrap_or(false)
            {
                return Ok(());
            }

            let result = async {
                let response = client
                    .post(&endpoint)
                    .json(&serde_json::json!({"model": model, "stream": false}))
                    .send()
                    .await
                    .with_context(|| format!("pull Ollama model {model}"))?
                    .error_for_status()
                    .with_context(|| format!("Ollama rejected model pull {model}"))?
                    .json::<PullResponse>()
                    .await
                    .with_context(|| format!("decode pull result for {model}"))?;
                anyhow::ensure!(
                    response.error.is_none() && response.status.as_deref() == Some("success"),
                    "Ollama pull did not complete: {}",
                    response
                        .error
                        .or(response.status)
                        .unwrap_or_else(|| "missing status".to_string())
                );
                let models = Self::list_api(base_url).await?;
                anyhow::ensure!(
                    Self::has_model(&models, model),
                    "Ollama reported success but {model} is absent from /api/tags"
                );
                Ok::<(), anyhow::Error>(())
            }
            .await;

            match result {
                Ok(()) => return Ok(()),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < attempts {
                        tokio::time::sleep(Duration::from_secs(attempt as u64)).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Ollama pull failed")))
    }
}

fn api_client(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(timeout)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

#[derive(Debug, Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<ApiModel>,
}

#[derive(Debug, Deserialize)]
struct ApiModel {
    name: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PullResponse {
    status: Option<String>,
    error: Option<String>,
}

/// Hardware-aware local LLM recommendation shown by onboarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalModelRecommendation {
    /// Exact Ollama model name.
    pub model: &'static str,
    /// Approximate model download size displayed before mutation.
    pub download: &'static str,
    /// Why this model was selected.
    pub rationale: &'static str,
}

/// Recommend a useful local model without selecting Gemma implicitly.
#[must_use]
pub fn recommend_local_llm(total_memory_bytes: Option<u64>) -> LocalModelRecommendation {
    const GIB: u64 = 1024 * 1024 * 1024;
    match total_memory_bytes {
        Some(bytes) if bytes >= 32 * GIB => LocalModelRecommendation {
            model: "gpt-oss:20b",
            download: "~13 GB",
            rationale: "32+ GB RAM: stronger local reasoning",
        },
        Some(bytes) if bytes >= 16 * GIB => LocalModelRecommendation {
            model: "qwen2.5:7b",
            download: "~4.7 GB",
            rationale: "16-31 GB RAM: balanced multilingual model",
        },
        _ => LocalModelRecommendation {
            model: crate::DEFAULT_LLM_FALLBACK_MODEL,
            download: "~2.0 GB",
            rationale: "compact default for machines below 16 GB or unknown RAM",
        },
    }
}

/// Best-effort download estimate for models offered by onboarding.
#[must_use]
pub fn download_estimate(model: &str) -> Option<&'static str> {
    match model.trim().trim_end_matches(":latest") {
        "gpt-oss:20b" => Some("~13 GB"),
        "qwen2.5:7b" => Some("~4.7 GB"),
        "llama3.2:3b" => Some("~2.0 GB"),
        "nomic-embed-text" => Some("~274 MB"),
        _ => None,
    }
}

/// NLI download locations are kept in one value so the executor can report a
/// precise rollback target and a UI can show the planned files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NliInstallTarget {
    /// Directory containing model.onnx, tokenizer.json and config.json.
    pub directory: PathBuf,
    /// Immutable HuggingFace revision used for downloads.
    pub revision: String,
}

impl NliInstallTarget {
    /// Construct a target for the default model directory.
    #[must_use]
    pub fn default_target(directory: PathBuf, revision: impl Into<String>) -> Self {
        Self {
            directory,
            revision: revision.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn ollama_commands_are_shell_free_and_deterministic() {
        assert_eq!(
            OllamaAdapter::version(),
            CommandSpec::new("ollama", ["--version"])
        );
        assert_eq!(
            OllamaAdapter::pull("nomic-embed-text:latest"),
            CommandSpec::new("ollama", ["pull", "nomic-embed-text:latest"])
        );
        assert!(!OllamaAdapter::pull("x; rm -rf /").args.is_empty());
    }

    #[test]
    fn model_parser_skips_header_and_empty_rows() {
        let models = OllamaAdapter::parse_models(
            "NAME ID SIZE MODIFIED\nllama3.2:3b abc 2GB now\n\n n\tdef 1GB now\n",
        );
        assert!(models.contains("llama3.2:3b"));
        assert!(models.contains("n"));
        assert_eq!(models.len(), 2);
    }

    #[test]
    fn implicit_latest_tag_is_idempotent() {
        let models = ["nomic-embed-text:latest".to_string()]
            .into_iter()
            .collect();
        assert!(OllamaAdapter::has_model(&models, "nomic-embed-text"));
        assert!(OllamaAdapter::has_model(&models, "nomic-embed-text:latest"));
        assert!(!OllamaAdapter::has_model(&models, "llama3.2:3b"));
    }

    #[test]
    fn recommendations_are_hardware_aware_and_never_gemma() {
        assert_eq!(recommend_local_llm(Some(8 << 30)).model, "llama3.2:3b");
        assert_eq!(recommend_local_llm(Some(16 << 30)).model, "qwen2.5:7b");
        assert_eq!(recommend_local_llm(Some(32 << 30)).model, "gpt-oss:20b");
        for memory in [None, Some(8 << 30), Some(16 << 30), Some(32 << 30)] {
            assert!(!recommend_local_llm(memory).model.contains("gemma"));
        }
    }

    #[tokio::test]
    async fn interrupted_api_pull_retries_and_verifies_inventory() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for request_index in 0..5 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = vec![0; 4096];
                let bytes = socket.read(&mut request).await.unwrap();
                let request = String::from_utf8_lossy(&request[..bytes]);
                let (status, body) = match request_index {
                    0 | 2 => {
                        assert!(request.starts_with("GET /api/tags "));
                        ("200 OK", r#"{"models":[]}"#)
                    }
                    1 => {
                        assert!(request.starts_with("POST /api/pull "));
                        ("500 Internal Server Error", r#"{"error":"interrupted"}"#)
                    }
                    3 => {
                        assert!(request.starts_with("POST /api/pull "));
                        ("200 OK", r#"{"status":"success"}"#)
                    }
                    _ => {
                        assert!(request.starts_with("GET /api/tags "));
                        (
                            "200 OK",
                            r#"{"models":[{"model":"nomic-embed-text:latest"}]}"#,
                        )
                    }
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });

        OllamaAdapter::pull_and_verify(&format!("http://{address}"), "nomic-embed-text", 2)
            .await
            .unwrap();
        server.await.unwrap();
    }
}
