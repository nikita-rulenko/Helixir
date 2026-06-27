use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tracing::{info, warn};

use super::base::{LlmMetadata, LlmProvider, LlmProviderError};

/// Wraps a primary [`LlmProvider`] with a secondary one used only when the
/// primary errors. The fallback is injected (any `LlmProvider`), not hardwired
/// to Ollama — the factory passes a local Ollama instance in production, tests
/// pass mocks. This keeps the failover *decision* (the part that matters)
/// verifiable without a live HTTP endpoint.
pub struct LlmProviderWithFallback {
    primary: Arc<dyn LlmProvider>,
    fallback: Arc<dyn LlmProvider>,
    fallback_enabled: bool,
    /// `"<provider> (fallback)"` — precomputed because `provider_name` returns
    /// `&str` and can't format on the fly.
    fallback_label: String,
    using_fallback: AtomicBool,
    fallback_count: AtomicUsize,
    primary_failures: AtomicUsize,
}

impl LlmProviderWithFallback {
    pub fn new(
        primary: Arc<dyn LlmProvider>,
        fallback_enabled: bool,
        fallback: Arc<dyn LlmProvider>,
    ) -> Self {
        let fallback_label = format!("{} (fallback)", fallback.provider_name());
        info!(
            "LlmProviderWithFallback initialized: primary={}, fallback={}/{}",
            primary.provider_name(),
            fallback.provider_name(),
            fallback.model_name()
        );

        Self {
            primary,
            fallback,
            fallback_enabled,
            fallback_label,
            using_fallback: AtomicBool::new(false),
            fallback_count: AtomicUsize::new(0),
            primary_failures: AtomicUsize::new(0),
        }
    }

    async fn fallback_generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        response_format: Option<&str>,
        original_error: &LlmProviderError,
    ) -> Result<(String, LlmMetadata), LlmProviderError> {
        warn!(
            "Falling back to {} ({}) due to: {}",
            self.fallback.provider_name(),
            self.fallback.model_name(),
            original_error
        );

        let (content, mut metadata) = self
            .fallback
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
        match self
            .primary
            .generate(system_prompt, user_prompt, response_format)
            .await
        {
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
                    self.fallback_generate(system_prompt, user_prompt, response_format, &e)
                        .await
                } else {
                    Err(e)
                }
            }
        }
    }

    fn provider_name(&self) -> &str {
        if self.using_fallback.load(Ordering::SeqCst) {
            &self.fallback_label
        } else {
            self.primary.provider_name()
        }
    }

    fn model_name(&self) -> &str {
        if self.using_fallback.load(Ordering::SeqCst) {
            self.fallback.model_name()
        } else {
            self.primary.model_name()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scriptable provider: succeeds with `reply` after `fail_first` initial
    /// failures. `fail_first = usize::MAX` ⇒ always fails.
    struct StubProvider {
        name: String,
        model: String,
        reply: String,
        fail_first: AtomicUsize,
    }

    impl StubProvider {
        fn always_ok(name: &str, model: &str, reply: &str) -> Arc<dyn LlmProvider> {
            Arc::new(Self {
                name: name.into(),
                model: model.into(),
                reply: reply.into(),
                fail_first: AtomicUsize::new(0),
            })
        }
        fn always_err(name: &str, model: &str) -> Arc<dyn LlmProvider> {
            Arc::new(Self {
                name: name.into(),
                model: model.into(),
                reply: String::new(),
                fail_first: AtomicUsize::new(usize::MAX),
            })
        }
        fn fail_then_ok(name: &str, model: &str, reply: &str, n: usize) -> Arc<dyn LlmProvider> {
            Arc::new(Self {
                name: name.into(),
                model: model.into(),
                reply: reply.into(),
                fail_first: AtomicUsize::new(n),
            })
        }
    }

    #[async_trait]
    impl LlmProvider for StubProvider {
        async fn generate(
            &self,
            _s: &str,
            _u: &str,
            _f: Option<&str>,
        ) -> Result<(String, LlmMetadata), LlmProviderError> {
            let remaining = self.fail_first.load(Ordering::SeqCst);
            if remaining > 0 {
                if remaining != usize::MAX {
                    self.fail_first.fetch_sub(1, Ordering::SeqCst);
                }
                return Err(LlmProviderError::Provider(format!("{} down", self.name)));
            }
            Ok((
                self.reply.clone(),
                LlmMetadata {
                    provider: self.name.clone(),
                    model: self.model.clone(),
                    ..Default::default()
                },
            ))
        }
        fn provider_name(&self) -> &str {
            &self.name
        }
        fn model_name(&self) -> &str {
            &self.model
        }
    }

    #[tokio::test]
    async fn primary_success_never_touches_fallback() {
        let w = LlmProviderWithFallback::new(
            StubProvider::always_ok("cerebras", "gpt-oss-120b", "PRIMARY"),
            true,
            StubProvider::always_err("ollama", "qwen2.5:7b"), // would error if called
        );
        let (content, meta) = w.generate("s", "u", None).await.unwrap();
        assert_eq!(content, "PRIMARY");
        assert!(!w.is_using_fallback());
        assert_eq!(w.fallback_count(), 0);
        assert!(!meta.fallback_used);
        assert_eq!(w.provider_name(), "cerebras");
        assert_eq!(w.model_name(), "gpt-oss-120b");
    }

    #[tokio::test]
    async fn primary_error_fails_over_to_fallback() {
        let w = LlmProviderWithFallback::new(
            StubProvider::always_err("cerebras", "gpt-oss-120b"),
            true,
            StubProvider::always_ok("ollama", "qwen2.5:7b", "FALLBACK"),
        );
        let (content, meta) = w.generate("s", "u", None).await.unwrap();
        assert_eq!(content, "FALLBACK", "must return the fallback's answer");
        assert!(w.is_using_fallback());
        assert_eq!(w.fallback_count(), 1);
        assert!(meta.fallback_used);
        assert_eq!(meta.original_provider.as_deref(), Some("cerebras"));
        assert!(meta.original_error.is_some());
        // identity reflects the fallback while it is active
        assert_eq!(w.provider_name(), "ollama (fallback)");
        assert_eq!(w.model_name(), "qwen2.5:7b");
    }

    #[tokio::test]
    async fn fallback_disabled_propagates_primary_error() {
        let w = LlmProviderWithFallback::new(
            StubProvider::always_err("cerebras", "gpt-oss-120b"),
            false,
            StubProvider::always_ok("ollama", "qwen2.5:7b", "FALLBACK"),
        );
        let r = w.generate("s", "u", None).await;
        assert!(r.is_err(), "disabled fallback must surface the primary error");
        assert!(!w.is_using_fallback());
        assert_eq!(w.fallback_count(), 0);
        assert_eq!(w.primary_failures(), 1);
    }

    #[tokio::test]
    async fn both_down_returns_error() {
        let w = LlmProviderWithFallback::new(
            StubProvider::always_err("cerebras", "gpt-oss-120b"),
            true,
            StubProvider::always_err("ollama", "qwen2.5:7b"),
        );
        assert!(w.generate("s", "u", None).await.is_err());
    }

    #[tokio::test]
    async fn primary_recovery_flips_identity_back() {
        // primary fails once (→ fallback), then recovers on the next call.
        let w = LlmProviderWithFallback::new(
            StubProvider::fail_then_ok("cerebras", "gpt-oss-120b", "PRIMARY", 1),
            true,
            StubProvider::always_ok("ollama", "qwen2.5:7b", "FALLBACK"),
        );
        let (c1, _) = w.generate("s", "u", None).await.unwrap();
        assert_eq!(c1, "FALLBACK");
        assert!(w.is_using_fallback());

        let (c2, m2) = w.generate("s", "u", None).await.unwrap();
        assert_eq!(c2, "PRIMARY", "primary recovered → its answer is used");
        assert!(!w.is_using_fallback(), "identity flips back to primary");
        assert!(!m2.fallback_used);
        assert_eq!(w.provider_name(), "cerebras");
    }

    #[tokio::test]
    async fn reset_clears_state() {
        let w = LlmProviderWithFallback::new(
            StubProvider::always_err("cerebras", "gpt-oss-120b"),
            true,
            StubProvider::always_ok("ollama", "qwen2.5:7b", "FALLBACK"),
        );
        w.generate("s", "u", None).await.unwrap();
        assert!(w.is_using_fallback());
        w.reset_fallback_state();
        assert!(!w.is_using_fallback());
        assert_eq!(w.primary_failures(), 0);
    }
}
