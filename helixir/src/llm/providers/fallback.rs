use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{info, warn};

use super::base::{LlmMetadata, LlmProvider, LlmProviderError};

/// Wraps a primary [`LlmProvider`] with an ordered chain of fallbacks, each
/// tried in turn only when everything before it errored. Tiers are injected
/// (any `LlmProvider`), not hardwired — the factory passes real providers in
/// production (e.g. cerebras → deepseek → ollama), tests pass mocks. This
/// keeps the failover *decision* (the part that matters) verifiable without
/// live HTTP endpoints.
///
/// Every call retries from the primary, so a recovered tier is readopted
/// automatically — the chain degrades under outage and heals on its own.
pub struct LlmProviderWithFallback {
    primary: Arc<dyn LlmProvider>,
    fallbacks: Vec<Arc<dyn LlmProvider>>,
    fallback_enabled: bool,
    /// `"<provider> (fallback)"` per tier — precomputed because
    /// `provider_name` returns `&str` and can't format on the fly.
    fallback_labels: Vec<String>,
    /// 0 = primary answered last; i ≥ 1 = `fallbacks[i-1]` answered last.
    active_tier: AtomicUsize,
    fallback_count: AtomicUsize,
    primary_failures: AtomicUsize,
}

impl LlmProviderWithFallback {
    pub fn new(
        primary: Arc<dyn LlmProvider>,
        fallback_enabled: bool,
        fallback: Arc<dyn LlmProvider>,
    ) -> Self {
        Self::new_chain(primary, fallback_enabled, vec![fallback])
    }

    pub fn new_chain(
        primary: Arc<dyn LlmProvider>,
        fallback_enabled: bool,
        fallbacks: Vec<Arc<dyn LlmProvider>>,
    ) -> Self {
        let fallback_labels = fallbacks
            .iter()
            .map(|f| format!("{} (fallback)", f.provider_name()))
            .collect();
        info!(
            "LlmProviderWithFallback initialized: primary={}, chain=[{}]",
            primary.provider_name(),
            fallbacks
                .iter()
                .map(|f| format!("{}/{}", f.provider_name(), f.model_name()))
                .collect::<Vec<_>>()
                .join(" → ")
        );

        Self {
            primary,
            fallbacks,
            fallback_enabled,
            fallback_labels,
            active_tier: AtomicUsize::new(0),
            fallback_count: AtomicUsize::new(0),
            primary_failures: AtomicUsize::new(0),
        }
    }

    /// Walk the fallback chain in order after the primary failed. Success on
    /// tier N annotates the metadata with the full trail of errors that led
    /// there, so the write path can surface *why* a weaker model answered.
    async fn fallback_generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        response_format: Option<&str>,
        primary_error: LlmProviderError,
    ) -> Result<(String, LlmMetadata), LlmProviderError> {
        let mut trail = vec![format!(
            "{}: {}",
            self.primary.provider_name(),
            primary_error
        )];
        let mut last_error = primary_error;

        for (i, tier) in self.fallbacks.iter().enumerate() {
            warn!(
                "Falling back to {} ({}) due to: {}",
                tier.provider_name(),
                tier.model_name(),
                last_error
            );

            match tier
                .generate(system_prompt, user_prompt, response_format)
                .await
            {
                Ok((content, mut metadata)) => {
                    metadata.fallback_used = true;
                    metadata.original_provider = Some(self.primary.provider_name().to_string());
                    metadata.original_error = Some(trail.join("; "));

                    self.active_tier.store(i + 1, Ordering::SeqCst);
                    self.fallback_count.fetch_add(1, Ordering::SeqCst);

                    info!(
                        "Fallback successful on {}! total_fallbacks={}",
                        tier.provider_name(),
                        self.fallback_count.load(Ordering::SeqCst)
                    );
                    return Ok((content, metadata));
                }
                Err(e) => {
                    trail.push(format!("{}: {}", tier.provider_name(), e));
                    last_error = e;
                }
            }
        }

        warn!(
            "All {} fallback tier(s) failed after the primary: {}",
            self.fallbacks.len(),
            trail.join("; ")
        );
        Err(last_error)
    }

    pub fn is_using_fallback(&self) -> bool {
        self.active_tier.load(Ordering::SeqCst) > 0
    }

    pub fn fallback_count(&self) -> usize {
        self.fallback_count.load(Ordering::SeqCst)
    }

    pub fn primary_failures(&self) -> usize {
        self.primary_failures.load(Ordering::SeqCst)
    }

    pub fn reset_fallback_state(&self) {
        self.active_tier.store(0, Ordering::SeqCst);
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
                self.active_tier.store(0, Ordering::SeqCst);
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
                    self.fallback_generate(system_prompt, user_prompt, response_format, e)
                        .await
                } else {
                    Err(e)
                }
            }
        }
    }

    fn provider_name(&self) -> &str {
        match self.active_tier.load(Ordering::SeqCst) {
            0 => self.primary.provider_name(),
            i => &self.fallback_labels[i - 1],
        }
    }

    fn model_name(&self) -> &str {
        match self.active_tier.load(Ordering::SeqCst) {
            0 => self.primary.model_name(),
            i => self.fallbacks[i - 1].model_name(),
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
        assert!(
            r.is_err(),
            "disabled fallback must surface the primary error"
        );
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

    // ---- 3-tier chain (cerebras → deepseek → ollama) ----

    #[tokio::test]
    async fn chain_prefers_earlier_tier() {
        // primary down, first fallback up: the second fallback is never asked.
        let w = LlmProviderWithFallback::new_chain(
            StubProvider::always_err("cerebras", "gpt-oss-120b"),
            true,
            vec![
                StubProvider::always_ok("deepseek", "deepseek-v4-flash", "MID"),
                StubProvider::always_err("ollama", "qwen2.5:7b"), // would error if called
            ],
        );
        let (content, meta) = w.generate("s", "u", None).await.unwrap();
        assert_eq!(content, "MID");
        assert_eq!(w.provider_name(), "deepseek (fallback)");
        assert_eq!(w.model_name(), "deepseek-v4-flash");
        assert!(meta.fallback_used);
        assert_eq!(meta.original_provider.as_deref(), Some("cerebras"));
    }

    #[tokio::test]
    async fn chain_cascades_past_dead_middle_tier() {
        // primary AND first fallback down: the last tier answers, and the
        // metadata carries the full error trail that led there.
        let w = LlmProviderWithFallback::new_chain(
            StubProvider::always_err("cerebras", "gpt-oss-120b"),
            true,
            vec![
                StubProvider::always_err("deepseek", "deepseek-v4-flash"),
                StubProvider::always_ok("ollama", "qwen2.5:7b", "LAST"),
            ],
        );
        let (content, meta) = w.generate("s", "u", None).await.unwrap();
        assert_eq!(content, "LAST");
        assert_eq!(w.provider_name(), "ollama (fallback)");
        assert_eq!(w.model_name(), "qwen2.5:7b");
        let trail = meta.original_error.unwrap();
        assert!(
            trail.contains("cerebras"),
            "trail must name tier 1: {trail}"
        );
        assert!(
            trail.contains("deepseek"),
            "trail must name tier 2: {trail}"
        );
    }

    #[tokio::test]
    async fn chain_all_down_returns_last_error() {
        let w = LlmProviderWithFallback::new_chain(
            StubProvider::always_err("cerebras", "gpt-oss-120b"),
            true,
            vec![
                StubProvider::always_err("deepseek", "deepseek-v4-flash"),
                StubProvider::always_err("ollama", "qwen2.5:7b"),
            ],
        );
        let e = w.generate("s", "u", None).await.unwrap_err();
        assert!(e.to_string().contains("ollama"), "last tier's error: {e}");
    }

    #[tokio::test]
    async fn chain_recovery_readopts_primary() {
        // full outage window: chain bottoms out at ollama; next call the
        // primary is back and the identity flips all the way home.
        let w = LlmProviderWithFallback::new_chain(
            StubProvider::fail_then_ok("cerebras", "gpt-oss-120b", "PRIMARY", 1),
            true,
            vec![
                StubProvider::always_err("deepseek", "deepseek-v4-flash"),
                StubProvider::always_ok("ollama", "qwen2.5:7b", "LAST"),
            ],
        );
        let (c1, _) = w.generate("s", "u", None).await.unwrap();
        assert_eq!(c1, "LAST");
        assert_eq!(w.provider_name(), "ollama (fallback)");

        let (c2, _) = w.generate("s", "u", None).await.unwrap();
        assert_eq!(c2, "PRIMARY");
        assert_eq!(w.provider_name(), "cerebras");
        assert!(!w.is_using_fallback());
    }
}
