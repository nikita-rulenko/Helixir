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
    assert!(actions.contains(&InstallAction::InstallAgentSkill(
        selected_clients().into_iter().collect()
    )));
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
        rbac: rbac::RbacInstallState {
            enabled: true,
            compatibility_group_exists: true,
            global_admins: BTreeSet::from(["helixir-operator".to_string()]),
            compatibility_group_admins: BTreeSet::from(["helixir-operator".to_string()]),
            all_users_enrolled: true,
            all_memories_covered: true,
        },
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
        vec![
            InstallAction::VerifyBackend,
            InstallAction::InstallAgentSkill(selected_clients().into_iter().collect()),
            InstallAction::RunDoctor,
        ]
    );
}

#[test]
fn explicit_legacy_mode_disables_existing_enforcement() {
    let state = SystemState {
        rbac: rbac::RbacInstallState {
            enabled: true,
            ..Default::default()
        },
        ..SystemState::default()
    };
    let options = InstallOptions {
        rbac: rbac::RbacInstallOptions {
            enabled: false,
            operator_id: "root".to_string(),
            principals: BTreeSet::new(),
        },
        ..InstallOptions::default()
    };

    let plan = Planner::build(&state, &options).unwrap();
    assert!(plan.steps.iter().any(|step| {
        step.action
            == (InstallAction::DisableRbac {
                operator_id: "root".to_string(),
            })
    }));
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
fn local_embeddings_require_ollama_nomic_and_nli_without_a_local_llm() {
    let state = SystemState::default();
    let options = InstallOptions {
        local_llm_model: None,
        ..InstallOptions::default()
    };

    let actions: Vec<_> = Planner::build(&state, &options)
        .unwrap()
        .steps
        .into_iter()
        .map(|step| step.action)
        .collect();
    assert!(actions.contains(&InstallAction::InstallOllama));
    assert!(actions.contains(&InstallAction::StartOllama));
    assert!(actions.contains(&InstallAction::PullOllamaModel(
        crate::DEFAULT_EMBEDDING_MODEL.to_string()
    )));
    assert!(actions.contains(&InstallAction::DownloadNli));
    assert!(!actions.contains(&InstallAction::PullOllamaModel(
        crate::DEFAULT_LLM_FALLBACK_MODEL.to_string()
    )));
}

#[test]
fn explicit_remote_embeddings_skip_ollama_but_keep_nli_required() {
    let state = SystemState::default();
    let options = InstallOptions {
        local_llm_model: None,
        embeddings: EmbeddingChoice::Remote(RemoteEmbeddingConfig {
            provider: "openai".to_string(),
            model: "text-embedding-3-small".to_string(),
            url: "https://example.invalid/v1".to_string(),
            api_key: "must-not-leak".to_string(),
        }),
        ..InstallOptions::default()
    };

    let debug = format!("{options:?}");
    assert!(!debug.contains("must-not-leak"));
    let actions: Vec<_> = Planner::build(&state, &options)
        .unwrap()
        .steps
        .into_iter()
        .map(|step| step.action)
        .collect();
    assert!(!actions.contains(&InstallAction::InstallOllama));
    assert!(!actions.contains(&InstallAction::StartOllama));
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, InstallAction::PullOllamaModel(_)))
    );
    assert!(actions.contains(&InstallAction::DownloadNli));
}

#[test]
fn healthy_remote_backend_and_embeddings_need_only_verification() {
    let state = SystemState {
        backend: BackendState::Remote {
            host: "helix.internal".to_string(),
            port: 6969,
            healthy: true,
        },
        nli_installed: true,
        central_config_matches: true,
        rbac: rbac::RbacInstallState {
            enabled: true,
            compatibility_group_exists: true,
            global_admins: BTreeSet::from(["helixir-operator".to_string()]),
            compatibility_group_admins: BTreeSet::from(["helixir-operator".to_string()]),
            all_users_enrolled: true,
            all_memories_covered: true,
        },
        ..SystemState::default()
    };
    let options = InstallOptions {
        backend: BackendChoice::ReuseDetected,
        local_llm_model: None,
        embeddings: EmbeddingChoice::Remote(RemoteEmbeddingConfig {
            provider: "openai".to_string(),
            model: "text-embedding-3-small".to_string(),
            url: "https://example.invalid/v1".to_string(),
            api_key: "redacted".to_string(),
        }),
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
fn implicit_latest_tag_does_not_schedule_duplicate_pull() {
    let state = SystemState {
        backend: BackendState::Local {
            healthy: true,
            schema_compatible: true,
        },
        ollama: OllamaState {
            installed: true,
            running: true,
            models: ["nomic-embed-text:latest".to_string()]
                .into_iter()
                .collect(),
        },
        nli_installed: true,
        central_config_matches: true,
        ..SystemState::default()
    };
    let options = InstallOptions {
        local_llm_model: None,
        ..InstallOptions::default()
    };

    let actions: Vec<_> = Planner::build(&state, &options)
        .unwrap()
        .steps
        .into_iter()
        .map(|step| step.action)
        .collect();
    assert!(!actions.iter().any(
            |action| matches!(action, InstallAction::PullOllamaModel(model) if model == crate::DEFAULT_EMBEDDING_MODEL)
        ));
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
