//! Redacted, allowlisted post-installation configuration administration.

use std::path::PathBuf;

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

use crate::core::config::{HelixirConfig, MemoryMode};

mod locks;
use locks::{locked_fields, reject_locked_fields};
/// Effective settings safe to return to the global-admin control plane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    pub config_path: String,
    pub locked_fields: Vec<String>,
    pub mode: MemoryMode,
    pub database: DatabaseSettings,
    pub reasoning: ReasoningSettings,
    pub embeddings: EmbeddingSettings,
    pub gateway: GatewaySettings,
    pub swarm: SwarmSettings,
    pub watchdog: WatchdogSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseSettings {
    pub host: String,
    pub port: u16,
    pub instance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningSettings {
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub temperature: f32,
    pub api_key_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingSettings {
    pub provider: String,
    pub model: String,
    pub url: String,
    pub api_key_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewaySettings {
    pub bind: String,
    pub public_url: String,
    pub auth_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmSettings {
    pub active_window_secs: u64,
    pub presence_ttl_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchdogSettings {
    pub enabled: bool,
    pub sample_interval_secs: u64,
    pub mem_alert_pct: f64,
    pub mem_restart_pct: f64,
    pub allow_container_restart: bool,
    pub allow_cache_reclaim: bool,
    pub backup_interval_hours: f64,
    pub backup_keep: usize,
}

/// Partial, allowlisted update. Secret fields are write-only replacements.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SettingsPatch {
    pub mode: Option<MemoryMode>,
    pub reasoning_provider: Option<String>,
    pub reasoning_model: Option<String>,
    pub reasoning_base_url: Option<String>,
    pub reasoning_temperature: Option<f32>,
    pub reasoning_api_key: Option<String>,
    pub embedding_provider: Option<String>,
    pub embedding_model: Option<String>,
    pub embedding_url: Option<String>,
    pub embedding_api_key: Option<String>,
    pub gateway_public_url: Option<String>,
    pub swarm_active_window_secs: Option<u64>,
    pub swarm_presence_ttl_secs: Option<u64>,
    pub watchdog_enabled: Option<bool>,
    pub watchdog_sample_interval_secs: Option<u64>,
    pub watchdog_mem_alert_pct: Option<f64>,
    pub watchdog_mem_restart_pct: Option<f64>,
    pub watchdog_allow_container_restart: Option<bool>,
    pub watchdog_allow_cache_reclaim: Option<bool>,
    pub backup_interval_hours: Option<f64>,
    pub backup_keep: Option<usize>,
}

impl std::fmt::Debug for SettingsPatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SettingsPatch")
            .field("mode", &self.mode)
            .field("reasoning_provider", &self.reasoning_provider)
            .field("reasoning_model", &self.reasoning_model)
            .field("reasoning_base_url", &self.reasoning_base_url)
            .field("reasoning_temperature", &self.reasoning_temperature)
            .field(
                "reasoning_api_key",
                &self.reasoning_api_key.as_ref().map(|_| "<redacted>"),
            )
            .field("embedding_provider", &self.embedding_provider)
            .field("embedding_model", &self.embedding_model)
            .field("embedding_url", &self.embedding_url)
            .field(
                "embedding_api_key",
                &self.embedding_api_key.as_ref().map(|_| "<redacted>"),
            )
            .finish_non_exhaustive()
    }
}
/// Result of an atomic settings write.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsApplyResult {
    pub changed: bool,
    pub config_backup: Option<String>,
    pub reload_required: bool,
    pub settings: SettingsSnapshot,
}
/// Load effective settings with secret values replaced by configured flags.
pub fn load() -> SettingsSnapshot {
    snapshot(&HelixirConfig::from_env())
}

/// Validate and atomically apply an allowlisted settings patch.
pub fn apply(patch: &SettingsPatch) -> anyhow::Result<SettingsApplyResult> {
    validate(patch)?;
    reject_locked_fields(patch)?;
    let path = target_path();
    let current = HelixirConfig::from_env();
    validate_effective(patch, &current)?;
    ensure_secret_continuity(patch, &current)?;
    let config_patch = build_config_patch(patch);
    ensure!(
        !config_patch.values.is_empty(),
        "settings patch contains no changes"
    );
    let existing = std::fs::read_to_string(&path).ok();
    let (candidate, _) = super::config::merge_patch(existing.as_deref(), &config_patch)?;
    toml::from_str::<HelixirConfig>(&candidate).context("validate resulting helixir.toml")?;
    let result = super::config::write_patch(&path, &config_patch)?;
    Ok(SettingsApplyResult {
        changed: result.changed,
        config_backup: result.backup.map(|value| value.display().to_string()),
        reload_required: result.changed,
        settings: load(),
    })
}

fn snapshot(config: &HelixirConfig) -> SettingsSnapshot {
    SettingsSnapshot {
        config_path: display_path(&target_path()),
        locked_fields: locked_fields(),
        mode: config.mode,
        database: DatabaseSettings {
            host: config.host.clone(),
            port: config.port,
            instance: config.instance.clone(),
        },
        reasoning: ReasoningSettings {
            provider: config.llm_provider.clone(),
            model: if config.llm_provider == "cerebras" {
                crate::DEFAULT_LLM_MODEL.to_string()
            } else {
                config.llm_model.clone()
            },
            base_url: config.llm_base_url.clone().unwrap_or_default(),
            temperature: config.llm_temperature,
            api_key_configured: config
                .llm_api_key
                .as_ref()
                .is_some_and(|key| !key.is_empty()),
        },
        embeddings: EmbeddingSettings {
            provider: config.embedding_provider.clone(),
            model: config.embedding_model.clone(),
            url: config.embedding_url.clone(),
            api_key_configured: config
                .embedding_api_key
                .as_ref()
                .is_some_and(|key| !key.is_empty()),
        },
        gateway: GatewaySettings {
            bind: config.gateway.default_bind.clone(),
            public_url: config.gateway.public_url.clone().unwrap_or_default(),
            auth_enabled: config
                .gateway
                .auth_token
                .as_ref()
                .is_some_and(|token| !token.is_empty()),
        },
        swarm: SwarmSettings {
            active_window_secs: config.swarm.active_window_secs,
            presence_ttl_secs: config.swarm.presence_ttl_secs,
        },
        watchdog: WatchdogSettings {
            enabled: config.watchdog.enabled,
            sample_interval_secs: config.watchdog.sample_interval_secs,
            mem_alert_pct: config.watchdog.mem_alert_pct,
            mem_restart_pct: config.watchdog.mem_restart_pct,
            allow_container_restart: config.watchdog.allow_container_restart,
            allow_cache_reclaim: config.watchdog.allow_cache_reclaim,
            backup_interval_hours: config.watchdog.backup_interval_hours,
            backup_keep: config.watchdog.backup_keep,
        },
    }
}

fn validate_effective(patch: &SettingsPatch, current: &HelixirConfig) -> anyhow::Result<()> {
    let active = patch
        .swarm_active_window_secs
        .unwrap_or(current.swarm.active_window_secs);
    let ttl = patch
        .swarm_presence_ttl_secs
        .unwrap_or(current.swarm.presence_ttl_secs);
    ensure!(
        ttl >= active,
        "presence TTL must not be shorter than the active window"
    );

    let alert = patch
        .watchdog_mem_alert_pct
        .unwrap_or(current.watchdog.mem_alert_pct);
    let restart = patch
        .watchdog_mem_restart_pct
        .unwrap_or(current.watchdog.mem_restart_pct);
    ensure!(
        restart == 0.0 || restart > alert,
        "restart threshold must be above alert threshold"
    );
    Ok(())
}

fn display_path(path: &std::path::Path) -> String {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Ok(relative) = path.strip_prefix(home)
    {
        return format!("~/{}", relative.display());
    }
    path.display().to_string()
}

fn validate(patch: &SettingsPatch) -> anyhow::Result<()> {
    if let Some(provider) = &patch.reasoning_provider {
        ensure!(
            ["cerebras", "deepseek", "ollama"].contains(&provider.as_str()),
            "unsupported reasoning provider"
        );
    }
    for value in [
        patch.reasoning_model.as_ref(),
        patch.embedding_model.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        ensure!(
            !value.trim().is_empty() && value.len() <= 128,
            "model name must be 1..128 characters"
        );
    }
    if let Some(provider) = &patch.embedding_provider {
        ensure!(
            ["ollama", "openai"].contains(&provider.as_str()),
            "unsupported embedding provider"
        );
    }
    for value in [
        patch.reasoning_base_url.as_ref(),
        patch.embedding_url.as_ref(),
        patch.gateway_public_url.as_ref(),
    ]
    .into_iter()
    .flatten()
    .filter(|value| !value.is_empty())
    {
        let parsed = reqwest::Url::parse(value).context("invalid provider URL")?;
        ensure!(
            matches!(parsed.scheme(), "http" | "https"),
            "provider URL must use http or https"
        );
    }
    if let Some(value) = patch.reasoning_temperature {
        ensure!((0.0..=2.0).contains(&value), "temperature must be 0..2");
    }
    if let Some(value) = patch.swarm_active_window_secs {
        ensure!(
            (15..=3600).contains(&value),
            "active window must be 15..3600 seconds"
        );
    }
    if let Some(value) = patch.swarm_presence_ttl_secs {
        ensure!(
            (30..=86_400).contains(&value),
            "presence TTL must be 30..86400 seconds"
        );
    }
    if let (Some(active), Some(ttl)) = (
        patch.swarm_active_window_secs,
        patch.swarm_presence_ttl_secs,
    ) {
        ensure!(
            ttl >= active,
            "presence TTL must not be shorter than the active window"
        );
    }
    if let Some(value) = patch.watchdog_sample_interval_secs {
        ensure!(
            (5..=86_400).contains(&value),
            "watchdog interval must be 5..86400 seconds"
        );
    }
    if let Some(value) = patch.watchdog_mem_alert_pct {
        ensure!(
            (25.0..=95.0).contains(&value),
            "memory alert must be 25..95 percent"
        );
    }
    if let Some(value) = patch.watchdog_mem_restart_pct {
        ensure!(
            value == 0.0 || (50.0..=99.0).contains(&value),
            "memory restart must be 0 or 50..99 percent"
        );
    }
    if let (Some(alert), Some(restart)) =
        (patch.watchdog_mem_alert_pct, patch.watchdog_mem_restart_pct)
    {
        ensure!(
            restart == 0.0 || restart > alert,
            "restart threshold must be above alert threshold"
        );
    }
    if let Some(value) = patch.backup_interval_hours {
        ensure!(
            (0.25..=8760.0).contains(&value),
            "backup interval must be 0.25..8760 hours"
        );
    }
    if let Some(value) = patch.backup_keep {
        ensure!(
            (1..=365).contains(&value),
            "backup retention must be 1..365 archives"
        );
    }
    for secret in [
        patch.reasoning_api_key.as_ref(),
        patch.embedding_api_key.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        ensure!(
            !secret.trim().is_empty() && secret.len() <= 16_384,
            "secret replacement must be non-empty and bounded"
        );
    }
    Ok(())
}

fn ensure_secret_continuity(patch: &SettingsPatch, current: &HelixirConfig) -> anyhow::Result<()> {
    let reasoning = patch
        .reasoning_provider
        .as_deref()
        .unwrap_or(&current.llm_provider);
    if reasoning != "ollama" {
        ensure!(
            patch.reasoning_api_key.is_some()
                || current
                    .llm_api_key
                    .as_ref()
                    .is_some_and(|key| !key.is_empty()),
            "selected reasoning provider needs an API key"
        );
    }
    let embeddings = patch
        .embedding_provider
        .as_deref()
        .unwrap_or(&current.embedding_provider);
    if embeddings != "ollama" {
        ensure!(
            patch.embedding_api_key.is_some()
                || current
                    .embedding_api_key
                    .as_ref()
                    .is_some_and(|key| !key.is_empty()),
            "selected embedding provider needs an API key"
        );
    }
    Ok(())
}

fn build_config_patch(patch: &SettingsPatch) -> super::config::ConfigPatch {
    let mut out = super::config::ConfigPatch::default();
    macro_rules! set {
        ($field:expr, $key:literal) => {
            if let Some(value) = $field {
                out = out.set($key, value.to_string());
            }
        };
    }
    if let Some(value) = patch.mode {
        out = out.set("mode", format!("{value:?}"));
    }
    if let Some(value) = &patch.reasoning_provider {
        out = out.set("llm_provider", value);
    }
    if let Some(value) = &patch.reasoning_model {
        out = out.set("llm_model", value);
    }
    if let Some(value) = &patch.reasoning_base_url {
        out = out.set("llm_base_url", value);
    }
    set!(patch.reasoning_temperature, "llm_temperature");
    if let Some(value) = &patch.reasoning_api_key {
        out = out.set("llm_api_key", value);
    }
    if let Some(value) = &patch.embedding_provider {
        out = out.set("embedding_provider", value);
    }
    if let Some(value) = &patch.embedding_model {
        out = out.set("embedding_model", value);
    }
    if let Some(value) = &patch.embedding_url {
        out = out.set("embedding_url", value);
    }
    if let Some(value) = &patch.embedding_api_key {
        out = out.set("embedding_api_key", value);
    }
    if let Some(value) = &patch.gateway_public_url {
        out = out.set("gateway.public_url", value);
    }
    set!(patch.swarm_active_window_secs, "swarm.active_window_secs");
    set!(patch.swarm_presence_ttl_secs, "swarm.presence_ttl_secs");
    set!(patch.watchdog_enabled, "watchdog.enabled");
    set!(
        patch.watchdog_sample_interval_secs,
        "watchdog.sample_interval_secs"
    );
    set!(patch.watchdog_mem_alert_pct, "watchdog.mem_alert_pct");
    set!(patch.watchdog_mem_restart_pct, "watchdog.mem_restart_pct");
    set!(
        patch.watchdog_allow_container_restart,
        "watchdog.allow_container_restart"
    );
    set!(
        patch.watchdog_allow_cache_reclaim,
        "watchdog.allow_cache_reclaim"
    );
    set!(
        patch.backup_interval_hours,
        "watchdog.backup_interval_hours"
    );
    set!(patch.backup_keep, "watchdog.backup_keep");
    out
}

fn target_path() -> PathBuf {
    HelixirConfig::config_file_path().unwrap_or_else(|| {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".helixir/helixir.toml")
    })
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
