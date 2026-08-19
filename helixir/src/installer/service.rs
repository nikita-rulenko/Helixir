//! One application service for native installer detection, planning and execution.

use std::path::{Path, PathBuf};

use thiserror::Error;

use super::executor::NativeInstallExecutor;
use super::native::NativeSystemDetector;
use super::{
    InstallAction, InstallObserver, InstallOptions, InstallPlan, InstallReport, InstallStep,
    PlanError, Planner, SystemDetector, SystemState,
};

/// Stable application-level installer errors shared by CLI and HTTP adapters.
#[derive(Debug, Error)]
pub enum InstallerServiceError {
    #[error("system detection failed")]
    Detection,
    #[error("cannot build a safe installation plan: {0}")]
    Plan(#[from] PlanError),
    #[error("cannot persist the installation manifest")]
    Manifest(#[source] anyhow::Error),
}

impl InstallerServiceError {
    /// Machine-readable code safe to expose through frontend adapters.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Detection => "install_detection_failed",
            Self::Plan(_) => "unsafe_install_plan",
            Self::Manifest(_) => "install_manifest_failed",
        }
    }
}

/// Read-only result used by plan previews before any mutation occurs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PreparedInstallation {
    pub state: SystemState,
    pub plan: InstallPlan,
}

/// Shared installer facade. Frontends own prompts/rendering, never policy.
pub struct InstallerService<D = NativeSystemDetector> {
    detector: D,
}

impl Default for InstallerService<NativeSystemDetector> {
    fn default() -> Self {
        Self::new(NativeSystemDetector::new())
    }
}

impl<D> InstallerService<D>
where
    D: SystemDetector,
{
    #[must_use]
    pub const fn new(detector: D) -> Self {
        Self { detector }
    }

    /// Detect the host through the configured read-only adapter.
    pub async fn detect(&self) -> Result<SystemState, InstallerServiceError> {
        self.detector
            .detect()
            .await
            .map_err(|_error| InstallerServiceError::Detection)
    }

    /// Detect and build the deterministic minimal plan.
    pub async fn prepare(
        &self,
        options: &InstallOptions,
    ) -> Result<PreparedInstallation, InstallerServiceError> {
        let state = self.detect().await?;
        let plan = Planner::build(&state, options)?;
        Ok(PreparedInstallation { state, plan })
    }
}

impl InstallerService<NativeSystemDetector> {
    /// Apply a freshly rebuilt plan using the one concrete native executor.
    pub async fn apply(
        &self,
        options: &InstallOptions,
    ) -> Result<InstallReport, InstallerServiceError> {
        let prepared = self.prepare(options).await?;
        let executor = NativeInstallExecutor::new(options, &prepared.state);
        let report = super::apply_plan(&executor, &prepared.plan).await;
        self.finish(options, &executor, &report)?;
        Ok(report)
    }

    /// Apply while forwarding typed events to a CLI or durable web journal.
    pub async fn apply_observed(
        &self,
        options: &InstallOptions,
        observer: &dyn InstallObserver,
    ) -> Result<InstallReport, InstallerServiceError> {
        let prepared = self.prepare(options).await?;
        let executor = NativeInstallExecutor::new(options, &prepared.state);
        let report = super::apply_plan_observed(&executor, &prepared.plan, observer).await;
        self.finish(options, &executor, &report)?;
        Ok(report)
    }

    /// Re-run only the shared backend and doctor verification actions.
    pub async fn verify(
        &self,
        options: &InstallOptions,
    ) -> Result<InstallReport, InstallerServiceError> {
        let state = self.detect().await?;
        let executor = NativeInstallExecutor::new(options, &state);
        let plan = InstallPlan {
            steps: vec![
                InstallStep {
                    action: InstallAction::VerifyBackend,
                    required: true,
                    reason: "verify backend health, schema and persistence".to_string(),
                },
                InstallStep {
                    action: InstallAction::RunDoctor,
                    required: true,
                    reason: "verify models, configuration and permanent RBAC".to_string(),
                },
            ],
        };
        Ok(super::apply_plan(&executor, &plan).await)
    }

    fn finish(
        &self,
        requested: &InstallOptions,
        executor: &NativeInstallExecutor,
        report: &InstallReport,
    ) -> Result<(), InstallerServiceError> {
        if !report.ready {
            return Ok(());
        }
        let effective = executor.effective_options();
        debug_assert_eq!(effective.rbac, requested.rbac);
        let backend = executor
            .backend_manifest()
            .map_err(InstallerServiceError::Manifest)?;
        write_install_manifest(&effective, backend).map_err(InstallerServiceError::Manifest)
    }
}

fn write_install_manifest(
    options: &InstallOptions,
    backend: super::manifest::BackendManifest,
) -> anyhow::Result<()> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
    let install_dir = std::env::current_exe()?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let models = options
        .local_llm_model
        .iter()
        .cloned()
        .chain(
            matches!(options.embeddings, super::EmbeddingChoice::LocalOllamaNomic)
                .then(|| crate::DEFAULT_EMBEDDING_MODEL.to_string()),
        )
        .collect();
    let clients = options
        .clients
        .iter()
        .map(|client| client.label().to_string())
        .collect();
    let manifest = super::manifest::InstallManifest {
        version: env!("CARGO_PKG_VERSION").to_string(),
        install_dir,
        backend_volume: backend.volume.clone(),
        backend,
        models,
        clients,
        rbac: Some(super::rbac::RbacManifest {
            enabled: true,
            operator_id: options.rbac.operator_id.clone(),
            group_id: crate::core::DEFAULT_GROUP_ID.to_string(),
            principals: options.rbac.principals.iter().cloned().collect(),
        }),
        last_backup: None,
    };
    super::manifest::write(&home.join(".helixir/install.json"), &manifest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_installation_round_trips_without_embedding_secrets() {
        let secret = "must-not-escape-the-transport";
        let options = InstallOptions {
            embeddings: super::super::EmbeddingChoice::Remote(
                super::super::RemoteEmbeddingConfig {
                    provider: "openai".to_string(),
                    model: "text-embedding-3-small".to_string(),
                    url: "https://example.invalid/v1".to_string(),
                    api_key: secret.to_string(),
                },
            ),
            local_llm_model: None,
            ..InstallOptions::default()
        };
        let prepared = PreparedInstallation {
            state: SystemState::default(),
            plan: Planner::build(&SystemState::default(), &options).unwrap(),
        };
        let encoded = serde_json::to_string(&prepared).unwrap();
        assert!(!encoded.contains(secret));
        let decoded: PreparedInstallation = serde_json::from_str(&encoded).unwrap();
        assert!(!decoded.plan.steps.is_empty());
        assert!(!format!("{options:?}").contains(secret));
        assert!(serde_json::to_string(&options).unwrap().contains(secret));
    }
}
