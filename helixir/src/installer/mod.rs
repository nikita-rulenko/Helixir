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

use std::collections::{BTreeMap, BTreeSet};

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
    /// A Helixir-managed local backend was detected.
    Local {
        /// Whether the service currently answers its health endpoint.
        healthy: bool,
        /// Whether its compiled schema matches this Helixir build.
        schema_compatible: bool,
    },
    /// A separately managed backend was detected or configured.
    Remote {
        /// Hostname or address selected by the operator.
        host: String,
        /// HelixDB HTTP port.
        port: u16,
        /// Whether the backend currently answers its health endpoint.
        healthy: bool,
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
}

impl Default for SystemState {
    fn default() -> Self {
        Self {
            backend: BackendState::Missing,
            ollama: OllamaState::default(),
            nli_installed: false,
            central_config_matches: false,
            client_registered: BTreeMap::new(),
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

/// User selections from an interactive UI or non-interactive flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOptions {
    /// Memory privilege tier to write to the central config.
    pub mode: MemoryMode,
    /// Backend ownership/connection choice.
    pub backend: BackendChoice,
    /// Allow the installer to install Ollama when it is absent.
    pub install_ollama: bool,
    /// Local Ollama LLM to ensure is available, or `None` for remote-only LLM.
    pub local_llm_model: Option<String>,
    /// Ensure the canonical Nomic embedding model is available through Ollama.
    pub install_nomic: bool,
    /// MCP clients selected for automatic registration.
    pub clients: BTreeSet<ClientKind>,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            mode: MemoryMode::Collective,
            backend: BackendChoice::ProvisionLocal,
            install_ollama: true,
            local_llm_model: Some(crate::DEFAULT_LLM_FALLBACK_MODEL.to_string()),
            install_nomic: true,
            clients: BTreeSet::new(),
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
    /// Register the stable MCP entry in one client.
    RegisterClient(ClientKind),
    /// Run the final read-only installation doctor.
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
    /// Local inference was requested without an existing/installable Ollama.
    #[error("local models require Ollama, but it is absent and installation was declined")]
    OllamaRequired,
}

/// Build a deterministic minimal plan from detected state and user choices.
pub struct Planner;

impl Planner {
    /// Construct an ordered idempotent plan without mutating the system.
    pub fn build(state: &SystemState, options: &InstallOptions) -> Result<InstallPlan, PlanError> {
        let mut steps = Vec::new();
        Self::plan_backend(state, options, &mut steps)?;
        Self::plan_local_models(state, options, &mut steps)?;

        if !state.nli_installed {
            steps.push(InstallStep::required(
                InstallAction::DownloadNli,
                "install the required contradiction-safe local NLI judge",
            ));
        }

        if !state.central_config_matches {
            steps.push(InstallStep::required(
                InstallAction::WriteCentralConfig,
                "store backend, provider, model and mode choices in one protected config",
            ));
        }

        for client in &options.clients {
            if !state
                .client_registered
                .get(client)
                .copied()
                .unwrap_or(false)
            {
                steps.push(InstallStep::required(
                    InstallAction::RegisterClient(*client),
                    format!("register helixir-local in {}", client.label()),
                ));
            }
        }

        steps.push(InstallStep::required(
            InstallAction::RunDoctor,
            "prove the selected installation is ready without writing a test memory",
        ));
        Ok(InstallPlan { steps })
    }

    fn plan_backend(
        state: &SystemState,
        options: &InstallOptions,
        steps: &mut Vec<InstallStep>,
    ) -> Result<(), PlanError> {
        match (&options.backend, &state.backend) {
            (BackendChoice::ReuseDetected, BackendState::Missing) => {
                return Err(PlanError::MissingDetectedBackend);
            }
            (BackendChoice::ProvisionLocal, BackendState::Missing) => {
                steps.push(InstallStep::required(
                    InstallAction::ProvisionBackend,
                    "create a managed HelixDB service with persistent storage",
                ));
                steps.push(InstallStep::required(
                    InstallAction::DeploySchema,
                    "deploy the schema bundled with this Helixir version",
                ));
            }
            (
                BackendChoice::ProvisionLocal | BackendChoice::ReuseDetected,
                BackendState::Local {
                    healthy,
                    schema_compatible,
                },
            ) => {
                if !healthy {
                    steps.push(InstallStep::required(
                        InstallAction::StartBackend,
                        "start the detected managed HelixDB service",
                    ));
                }
                if !schema_compatible {
                    steps.push(InstallStep::required(
                        InstallAction::BackupBackend,
                        "protect existing memory before changing the compiled schema",
                    ));
                    steps.push(InstallStep::required(
                        InstallAction::DeploySchema,
                        "bring the backend schema in sync with this Helixir version",
                    ));
                }
            }
            (BackendChoice::JoinRemote { .. }, _) => {}
            (BackendChoice::ReuseDetected, BackendState::Remote { .. }) => {}
            (BackendChoice::ProvisionLocal, BackendState::Remote { .. }) => {
                steps.push(InstallStep::required(
                    InstallAction::ProvisionBackend,
                    "create the explicitly selected managed local backend",
                ));
                steps.push(InstallStep::required(
                    InstallAction::DeploySchema,
                    "deploy the schema bundled with this Helixir version",
                ));
            }
        }
        steps.push(InstallStep::required(
            InstallAction::VerifyBackend,
            "verify backend health, schema compatibility and persistence",
        ));
        Ok(())
    }

    fn plan_local_models(
        state: &SystemState,
        options: &InstallOptions,
        steps: &mut Vec<InstallStep>,
    ) -> Result<(), PlanError> {
        let mut models = BTreeSet::new();
        if let Some(model) = options
            .local_llm_model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty())
        {
            models.insert(model.to_string());
        }
        if options.install_nomic {
            models.insert(crate::DEFAULT_EMBEDDING_MODEL.to_string());
        }
        if models.is_empty() {
            return Ok(());
        }

        if !state.ollama.installed {
            if !options.install_ollama {
                return Err(PlanError::OllamaRequired);
            }
            steps.push(InstallStep::required(
                InstallAction::InstallOllama,
                "install the selected local model runtime",
            ));
        }
        if !state.ollama.running {
            steps.push(InstallStep::required(
                InstallAction::StartOllama,
                "start Ollama before pulling or verifying local models",
            ));
        }
        for model in models {
            if !state.ollama.models.contains(&model) {
                steps.push(InstallStep::required(
                    InstallAction::PullOllamaModel(model.clone()),
                    format!("make local model {model} available"),
                ));
            }
        }
        Ok(())
    }
}

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
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn selected_clients() -> BTreeSet<ClientKind> {
        [
            ClientKind::ClaudeCode,
            ClientKind::Codex,
            ClientKind::Cursor,
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn fresh_local_plan_orders_backend_models_config_clients_then_doctor() {
        let state = SystemState::default();
        let options = InstallOptions {
            clients: selected_clients(),
            ..InstallOptions::default()
        };

        let actions: Vec<_> = Planner::build(&state, &options)
            .unwrap()
            .steps
            .into_iter()
            .map(|step| step.action)
            .collect();

        assert_eq!(actions[0], InstallAction::ProvisionBackend);
        assert_eq!(actions[1], InstallAction::DeploySchema);
        assert_eq!(actions[2], InstallAction::VerifyBackend);
        assert_eq!(actions[3], InstallAction::InstallOllama);
        assert_eq!(actions[4], InstallAction::StartOllama);
        assert!(actions.contains(&InstallAction::PullOllamaModel(
            crate::DEFAULT_LLM_FALLBACK_MODEL.to_string()
        )));
        assert!(actions.contains(&InstallAction::PullOllamaModel(
            crate::DEFAULT_EMBEDDING_MODEL.to_string()
        )));
        assert!(actions.contains(&InstallAction::DownloadNli));
        assert!(actions.contains(&InstallAction::RegisterClient(ClientKind::Codex)));
        assert_eq!(actions.last(), Some(&InstallAction::RunDoctor));
    }

    #[test]
    fn satisfied_install_is_idempotent_except_for_verification() {
        let models = [
            crate::DEFAULT_LLM_FALLBACK_MODEL.to_string(),
            crate::DEFAULT_EMBEDDING_MODEL.to_string(),
        ]
        .into_iter()
        .collect();
        let clients = selected_clients();
        let state = SystemState {
            backend: BackendState::Local {
                healthy: true,
                schema_compatible: true,
            },
            ollama: OllamaState {
                installed: true,
                running: true,
                models,
            },
            nli_installed: true,
            central_config_matches: true,
            client_registered: clients.iter().copied().map(|c| (c, true)).collect(),
        };
        let options = InstallOptions {
            clients,
            ..InstallOptions::default()
        };

        let actions: Vec<_> = Planner::build(&state, &options)
            .unwrap()
            .steps
            .into_iter()
            .map(|step| step.action)
            .collect();

        assert_eq!(
            actions,
            vec![InstallAction::VerifyBackend, InstallAction::RunDoctor]
        );
    }

    #[test]
    fn schema_change_is_backed_up_before_deploy() {
        let state = SystemState {
            backend: BackendState::Local {
                healthy: true,
                schema_compatible: false,
            },
            ..SystemState::default()
        };
        let options = InstallOptions {
            local_llm_model: None,
            install_nomic: false,
            ..InstallOptions::default()
        };

        let actions: Vec<_> = Planner::build(&state, &options)
            .unwrap()
            .steps
            .into_iter()
            .map(|step| step.action)
            .collect();

        let backup = actions
            .iter()
            .position(|a| a == &InstallAction::BackupBackend)
            .unwrap();
        let deploy = actions
            .iter()
            .position(|a| a == &InstallAction::DeploySchema)
            .unwrap();
        assert!(backup < deploy);
    }

    #[test]
    fn local_models_require_existing_or_installable_ollama() {
        let state = SystemState::default();
        let options = InstallOptions {
            install_ollama: false,
            ..InstallOptions::default()
        };

        assert_eq!(
            Planner::build(&state, &options),
            Err(PlanError::OllamaRequired)
        );
    }

    #[derive(Default)]
    struct FakeExecutor {
        applied: Mutex<Vec<InstallAction>>,
        fail_on: Option<InstallAction>,
        rolled_back: Mutex<Vec<InstallAction>>,
    }

    #[async_trait]
    impl PlanExecutor for FakeExecutor {
        async fn apply(&self, action: &InstallAction) -> std::result::Result<(), String> {
            if self.fail_on.as_ref() == Some(action) {
                return Err("injected failure".to_string());
            }
            self.applied.lock().unwrap().push(action.clone());
            Ok(())
        }

        async fn rollback(&self, completed: &[InstallAction]) -> std::result::Result<(), String> {
            self.rolled_back
                .lock()
                .unwrap()
                .extend_from_slice(completed);
            Ok(())
        }
    }

    #[tokio::test]
    async fn required_failure_stops_and_rolls_back_completed_steps() {
        let plan = InstallPlan {
            steps: vec![
                InstallStep::required(InstallAction::ProvisionBackend, "provision"),
                InstallStep::required(InstallAction::DeploySchema, "deploy"),
                InstallStep::required(InstallAction::RunDoctor, "verify"),
            ],
        };
        let executor = FakeExecutor {
            fail_on: Some(InstallAction::DeploySchema),
            ..FakeExecutor::default()
        };

        let report = apply_plan(&executor, &plan).await;

        assert!(!report.ready);
        assert!(report.rollback_attempted);
        assert_eq!(report.steps.len(), 2);
        assert_eq!(
            *executor.rolled_back.lock().unwrap(),
            vec![InstallAction::ProvisionBackend]
        );
    }
}
