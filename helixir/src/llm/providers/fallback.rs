

use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::base::{LlmMetadata, LlmProvider, LlmProviderError};
use super::ollama::OllamaProvider;

const DEFAULT_FALLBACK_URL: &str = "http://localhost:11434";
const DEFAULT_FALLBACK_MODEL: &str = "llama3.2";


pub struct LlmProviderWithFallback {
    primary: Arc<dyn LlmProvider>,
    fallback_enabled: bool,
    fallback_url: String,
    fallback_model: String,
    temperature: f64,
    
    
    fallback_provider: RwLock<Option<OllamaProvider>>,
    using_fallback: AtomicBool,
    fallback_count: AtomicUsize,
    primary_failures: AtomicUsize,
}

impl LlmProviderWithFallback {
    
    pub fn new(
        primary: Arc<dyn LlmProvider>,
        fallback_enabled: bool,
        fallback_url: Option<String>,
        fallback_model: Option<String>,
        temperature: f64,
    ) -> Self {
        let fallback_url = fallback_url.unwrap_or_else(|| DEFAULT_FALLBACK_URL.to_string());
        let fallback_model = fallback_model.unwrap_or_else(|| DEFAULT_FALLBACK_MODEL.to_string());
        
        info!(
            "LlmProviderWithFallback initialized: primary={}, fallback={}/{}",
            primary.provider_name(),
            fallback_url,
            fallback_model
        );
        
        Self {
            primary,
            fallback_enabled,
            fallback_url,
            fallback_model,
            temperature,
            fallback_provider: RwLock::new(None),
            using_fallback: AtomicBool::new(false),
            fallback_count: AtomicUsize::new(0),
            primary_failures: AtomicUsize::new(0),
        }
    }

    
    async fn get_fallback_provider(&self) -> OllamaProvider {
        let guard = self.fallback_provider.read().await;
        if let Some(ref provider) = *guard {
            return OllamaProvider::new(
                self.fallback_url.clone(),
                self.fallback_model.clone(),
                self.temperature,
            );
        }
        drop(guard);

        let mut guard = self.fallback_provider.write().await;
        if guard.is_none() {
            *guard = Some(OllamaProvider::new(
                self.fallback_url.clone(),
                self.fallback_model.clone(),
                self.temperature,
            ));
            info!("Fallback provider initialized: {}/{}", self.fallback_url, self.fallback_model);
        }
        
        OllamaProvider::new(
            self.fallback_url.clone(),
            self.fallback_model.clone(),
            self.temperature,
        )
    }

    
    async fn fallback_generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        response_format: Option<&str>,
        original_error: &LlmProviderError,
    ) -> Result<(String, LlmMetadata), LlmProviderError> {
        warn!(
            "Falling back to Ollama ({}/{}) due to: {}",
            self.fallback_url, self.fallback_model, original_error
        );

        let fallback = self.get_fallback_provider().await;
        let (content, mut metadata) = fallback
            .generate(system_prompt, user_prompt, response_format)
            .await?;

        metadata.fallback_used = true;
        metadata.original_provider = Some(self.primary.provider_name().to_string());
        metadata.original_error = Some(original_error.to_string());

        self.using_fallback.store(true, Ordering::SeqCst);
        self.fallback_count.fetch_add(1, Ordering::SeqCst);

        info!(
            "Fallback successful! total_fallbacks={}",
            self.fallback_count.load(Ordering::SeqCst)
        );

        Ok((content, metadata))
    }

    
    pub fn is_using_fallback(&self) -> bool {
        self.using_fallback.load(Ordering::SeqCst)
    }

    
    pub fn fallback_count(&self) -> usize {
        self.fallback_count.load(Ordering::SeqCst)
    }

    
    pub fn primary_failures(&self) -> usize {
        self.primary_failures.load(Ordering::SeqCst)
    }

    
    pub fn reset_fallback_state(&self) {
        self.using_fallback.store(false, Ordering::SeqCst);
        self.primary_failures.store(0, Ordering::SeqCst);
        info!("Fallback state reset");
    }
}

#[async_trait]
impl LlmProvider for LlmProviderWithFallback {
    async fn generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        response_format: Option<&str>,
    ) -> Result<(String, LlmMetadata), LlmProviderError> {
        match self.primary.generate(system_prompt, user_prompt, response_format).await {
            Ok((content, metadata)) => {
                self.using_fallback.store(false, Ordering::SeqCst);
                self.primary_failures.store(0, Ordering::SeqCst);
                Ok((content, metadata))
            }
            Err(e) => {
                self.primary_failures.fetch_add(1, Ordering::SeqCst);
                warn!(
                    "Primary LLM provider failed ({}x): {}",
                    self.primary_failures.load(Ordering::SeqCst),
                    e
                );

                if self.fallback_enabled {
                    self.fallback_generate(system_prompt, user_prompt, response_format, &e).await
                } else {
                    Err(e)
                }
            }
        }
    }

    fn provider_name(&self) -> &str {
        if self.using_fallback.load(Ordering::SeqCst) {
            "ollama (fallback)"
        } else {
            self.primary.provider_name()
        }
    }

    fn model_name(&self) -> &str {
        if self.using_fallback.load(Ordering::SeqCst) {
            &self.fallback_model
        } else {
            self.primary.model_name()
        }
    }
}
