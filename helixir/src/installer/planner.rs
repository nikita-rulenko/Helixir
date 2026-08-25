//! Deterministic construction of an installation plan.

use super::*;

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

        if !state.rbac.satisfies(&options.rbac) {
            steps.push(InstallStep::required(
                InstallAction::BootstrapRbac {
                    operator_id: options.rbac.operator_id.clone(),
                    principals: options.rbac.principals.iter().cloned().collect(),
                },
                "converge default/onboarding/Moirai workspaces and permanently enable enforcement",
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
        if !options.clients.is_empty() {
            steps.push(InstallStep::required(
                InstallAction::InstallAgentSkill(options.clients.iter().copied().collect()),
                "install one canonical Helixir memory and RBAC skill for every selected client",
            ));
        }

        steps.push(InstallStep::required(
            InstallAction::RunDoctor,
            "prove readiness and recover broken embeddings without writing a test memory",
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
                    InstallAction::DeploySchema,
                    "prepare the release-pinned managed HelixDB image",
                ));
                steps.push(InstallStep::required(
                    InstallAction::ProvisionBackend,
                    "create a managed HelixDB service with persistent storage",
                ));
            }
            (
                BackendChoice::ProvisionLocal | BackendChoice::ReuseDetected,
                BackendState::ManagedLocal {
                    healthy,
                    schema_compatible,
                    ..
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
                        "deploy the release-pinned backend on the preserved volume",
                    ));
                }
            }
            (BackendChoice::JoinRemote { .. }, _) => {}
            (
                BackendChoice::ReuseDetected,
                BackendState::ExistingLocal {
                    schema_compatible, ..
                }
                | BackendState::Remote {
                    schema_compatible, ..
                },
            ) => {
                if !schema_compatible {
                    return Err(PlanError::IncompatibleExternalBackend);
                }
            }
            (BackendChoice::ProvisionLocal, BackendState::ExistingLocal { .. }) => {
                return Err(PlanError::ExistingLocalConflict);
            }
            (BackendChoice::ProvisionLocal, BackendState::Remote { .. }) => {
                steps.push(InstallStep::required(
                    InstallAction::DeploySchema,
                    "prepare the release-pinned managed HelixDB image",
                ));
                steps.push(InstallStep::required(
                    InstallAction::ProvisionBackend,
                    "create the explicitly selected managed local backend",
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
        if matches!(options.embeddings, EmbeddingChoice::LocalOllamaNomic) {
            models.insert(crate::DEFAULT_EMBEDDING_MODEL.to_string());
        }
        if models.is_empty() {
            return Ok(());
        }

        if !state.ollama.installed {
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
            if !models::OllamaAdapter::has_model(&state.ollama.models, &model) {
                let estimate = models::download_estimate(&model)
                    .map(|size| format!("; estimated download {size}"))
                    .unwrap_or_default();
                steps.push(InstallStep::required(
                    InstallAction::PullOllamaModel(model.clone()),
                    format!("make local model {model} available{estimate}"),
                ));
            }
        }
        Ok(())
    }
}
