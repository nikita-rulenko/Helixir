//! Installation and onboarding orchestration.
//!
//! This module owns the machine-facing workflow that takes Helixir from a set
//! of user choices to a verified installation. Frontends (the CLI today and a
//! native UI later) gather choices and render progress; platform adapters
//! detect and apply system changes. The plan in between is deterministic and
//! testable without touching Docker, Ollama, model registries, or MCP clients.

pub mod backend;
pub mod client_config;
pub mod clients;
pub mod config;
pub mod doctor;
pub mod manifest;
pub mod models;
mod planner;
pub mod rbac;
pub mod skills;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use async_trait::async_trait;
use thiserror::Error;

use crate::core::config::MemoryMode;

/// MCP clients the onboarding flow can register automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClientKind {
    /// Anthropic Claude Code CLI.
    ClaudeCode,
    /// OpenAI Codex CLI or desktop application.
    Codex,
    /// Cursor editor.
    Cursor,
}

impl ClientKind {
    /// Stable human-readable client name used by all frontends.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Cursor => "Cursor",
        }
    }
}

/// What kind of backend is already present on the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendState {
    /// No known local or remote backend is configured.
    Missing,
    /// A Helixir-managed local backend was detected with its exact endpoint
    /// and Docker ownership metadata.
    ManagedLocal {
        host: String,
        port: u16,
        container: String,
        volume: String,
        image: String,
        /// Whether the service currently answers its health endpoint.
        healthy: bool,
        /// Whether its compiled schema matches this Helixir build.
        schema_compatible: bool,
    },
    /// A reachable local database not owned by Helixir.
    ExistingLocal {
        host: String,
        port: u16,
        healthy: bool,
        schema_compatible: bool,
    },
    /// A separately managed remote backend was detected or configured.
    Remote {
        /// Hostname or address selected by the operator.
        host: String,
        /// HelixDB HTTP port.
        port: u16,
        /// Whether the backend currently answers its health endpoint.
        healthy: bool,
        schema_compatible: bool,
    },
}

/// Ollama state relevant to onboarding.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OllamaState {
    /// Whether an Ollama installation was detected.
    pub installed: bool,
    /// Whether the local Ollama API is responding.
    pub running: bool,
    /// Model names already available to the local Ollama service.
    pub models: BTreeSet<String>,
}

/// Read-only snapshot produced by platform detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemState {
    /// Current backend state.
    pub backend: BackendState,
    /// Current Ollama installation, service and models.
    pub ollama: OllamaState,
    /// Whether the NLI weights and tokenizer are installed and loadable.
    pub nli_installed: bool,
    /// Whether the resolved central config already matches the requested plan.
    pub central_config_matches: bool,
    /// Existing `helixir-local` registration state by client.
    pub client_registered: BTreeMap<ClientKind, bool>,
    /// Current graph-backed authorization state.
    pub rbac: rbac::RbacInstallState,
}

impl Default for SystemState {
    fn default() -> Self {
        Self {
            backend: BackendState::Missing,
            ollama: OllamaState::default(),
            nli_installed: false,
            central_config_matches: false,
            client_registered: BTreeMap::new(),
            rbac: rbac::RbacInstallState::default(),
        }
    }
}

/// Operator choice for the HelixDB backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendChoice {
    /// Install or reuse the managed local persistent backend.
    ProvisionLocal,
    /// Reuse the backend found during detection without changing ownership.
    ReuseDetected,
    /// Join a separately managed backend.
    JoinRemote { host: String, port: u16 },
}

/// Explicit configuration for an OpenAI-compatible remote embedding service.
#[derive(Clone, PartialEq, Eq)]
pub struct RemoteEmbeddingConfig {
    /// Provider adapter name. The current remote adapter is `openai`.
    pub provider: String,
    /// Provider-specific embedding model name.
    pub model: String,
    /// OpenAI-compatible API root, without the trailing `/embeddings` path.
    pub url: String,
    /// Secret used only in the protected central config and health probe.
    pub api_key: String,
}

impl fmt::Debug for RemoteEmbeddingConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteEmbeddingConfig")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("url", &self.url)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

/// Exactly one embedding strategy selected during onboarding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingChoice {
    /// Recommended local path: Ollama plus `nomic-embed-text`.
    LocalOllamaNomic,
    /// Explicit OpenAI-compatible remote embedding service.
    Remote(RemoteEmbeddingConfig),
}

/// User selections from an interactive UI or non-interactive flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOptions {
    /// Memory privilege tier to write to the central config.
    pub mode: MemoryMode,
    /// Backend ownership/connection choice.
    pub backend: BackendChoice,
    /// Optional local Ollama fallback LLM to ensure is available.
    pub local_llm_model: Option<String>,
    /// Required, fully specified embedding strategy.
    pub embeddings: EmbeddingChoice,
    /// MCP clients selected for automatic registration.
    pub clients: BTreeSet<ClientKind>,
    /// Graph-backed authorization profile.
    pub rbac: rbac::RbacInstallOptions,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            mode: MemoryMode::Collective,
            backend: BackendChoice::ProvisionLocal,
            local_llm_model: Some(crate::DEFAULT_LLM_FALLBACK_MODEL.to_string()),
            embeddings: EmbeddingChoice::LocalOllamaNomic,
            clients: BTreeSet::new(),
            rbac: rbac::RbacInstallOptions::default(),
        }
    }
}

/// One idempotent system change or verification in an installation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallAction {
    /// Create the managed persistent HelixDB service and data volume.
    ProvisionBackend,
    /// Start a detected but stopped managed backend.
    StartBackend,
    /// Back up backend data before a schema-affecting transition.
    BackupBackend,
    /// Deploy the schema bundled with this Helixir version.
    DeploySchema,
    /// Verify health, schema compatibility and persistence.
    VerifyBackend,
    /// Install Ollama using the platform adapter.
    InstallOllama,
    /// Start the local Ollama service.
    StartOllama,
    /// Pull a named Ollama model if it is absent.
    PullOllamaModel(String),
    /// Download and verify the host-compatible local NLI model.
    DownloadNli,
    /// Atomically write the protected central `helixir.toml`.
    WriteCentralConfig,
    /// Bootstrap the default, onboarding, and Moirai workspaces and attach legacy state.
    BootstrapRbac {
        operator_id: String,
        principals: Vec<String>,
    },
    /// Register the stable MCP entry in one client.
    RegisterClient(ClientKind),
    /// Install the canonical Helixir skill for all selected clients.
    InstallAgentSkill(Vec<ClientKind>),
    /// Run the final doctor, including local embedding recovery when needed.
    RunDoctor,
}

/// Planned action together with its user-facing rationale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallStep {
    /// Concrete adapter action.
    pub action: InstallAction,
    /// Whether failure aborts the remaining plan and triggers rollback.
    pub required: bool,
    /// Short explanation shown in CLI/UI plan previews.
    pub reason: String,
}

impl InstallStep {
    fn required(action: InstallAction, reason: impl Into<String>) -> Self {
        Self {
            action,
            required: true,
            reason: reason.into(),
        }
    }
}

/// Complete ordered, previewable onboarding plan.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InstallPlan {
    /// Steps in execution order.
    pub steps: Vec<InstallStep>,
}

/// Reasons a requested plan cannot safely be constructed.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlanError {
    /// `ReuseDetected` was selected but discovery found nothing to reuse.
    #[error("no backend was detected; provision a local backend or join a remote one")]
    MissingDetectedBackend,
    /// A separately managed backend cannot be mutated by local Docker actions.
    #[error(
        "the selected existing backend has an incompatible schema; upgrade it with its owner before onboarding"
    )]
    IncompatibleExternalBackend,
    #[error(
        "a separately managed local database already occupies the selected endpoint; reuse it or choose another port"
    )]
    ExistingLocalConflict,
}

/// Build a deterministic minimal plan from detected state and user choices.
pub struct Planner;

/// Read-only platform detection boundary.
#[async_trait]
pub trait SystemDetector: Send + Sync {
    /// Inspect backend, model runtime, model files and client registrations.
    async fn detect(&self) -> std::result::Result<SystemState, String>;
}

/// Mutation and verification boundary implemented by each supported platform.
#[async_trait]
pub trait PlanExecutor: Send + Sync {
    /// Apply one idempotent action.
    async fn apply(&self, action: &InstallAction) -> std::result::Result<(), String>;

    /// Roll back mutations already completed after a required step fails.
    async fn rollback(&self, completed: &[InstallAction]) -> std::result::Result<(), String>;
}

/// Result of one attempted plan step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepReport {
    /// Attempted action.
    pub action: InstallAction,
    /// Whether the adapter completed the action successfully.
    pub succeeded: bool,
    /// Optional adapter detail or error message.
    pub detail: Option<String>,
}

/// Machine-readable result of applying an installation plan.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InstallReport {
    /// Per-step execution results in plan order.
    pub steps: Vec<StepReport>,
    /// Whether all required steps succeeded.
    pub ready: bool,
    /// Whether rollback was attempted after a required failure.
    pub rollback_attempted: bool,
    /// Rollback error, if rollback itself failed.
    pub rollback_error: Option<String>,
}

/// Apply a plan sequentially and roll back after the first required failure.
pub async fn apply_plan(executor: &dyn PlanExecutor, plan: &InstallPlan) -> InstallReport {
    let mut report = InstallReport::default();
    let mut completed = Vec::new();

    for step in &plan.steps {
        match executor.apply(&step.action).await {
            Ok(()) => {
                completed.push(step.action.clone());
                report.steps.push(StepReport {
                    action: step.action.clone(),
                    succeeded: true,
                    detail: None,
                });
            }
            Err(error) => {
                report.steps.push(StepReport {
                    action: step.action.clone(),
                    succeeded: false,
                    detail: Some(error),
                });
                if step.required {
                    report.rollback_attempted = true;
                    if let Err(rollback_error) = executor.rollback(&completed).await {
                        report.rollback_error = Some(rollback_error);
                    }
                    return report;
                }
            }
        }
    }
    report.ready = true;
    report
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
