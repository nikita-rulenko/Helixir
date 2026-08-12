use super::*;

pub(crate) fn gather_onboard_options(
    state: &helixir::installer::SystemState,
    interactive: bool,
    mode: Option<String>,
    model_args: &OnboardModelArgs,
    backend_args: &OnboardBackendArgs,
    security_args: &OnboardSecurityArgs,
) -> Result<helixir::installer::InstallOptions> {
    use helixir::installer::{BackendChoice, EmbeddingChoice, InstallOptions};
    use std::collections::BTreeSet;

    let env_mode = std::env::var("HELIXIR_MODE").unwrap_or_default();
    let effective_mode = match mode {
        Some(value) => MemoryMode::parse(&value),
        None if !env_mode.trim().is_empty() => MemoryMode::parse(&env_mode),
        None if interactive => prompt_mode_recommendation()?,
        None => MemoryMode::Collective,
    };

    let backend = if let Some(host) = backend_args.backend_host.as_deref() {
        BackendChoice::JoinRemote {
            host: host.trim().to_string(),
            port: backend_args.backend_port,
        }
    } else if backend_args.provision_local {
        BackendChoice::ProvisionLocal
    } else if backend_args.reuse_detected {
        BackendChoice::ReuseDetected
    } else if interactive {
        let detected = !matches!(state.backend, helixir::installer::BackendState::Missing);
        let options = if detected {
            vec![
                "reuse the detected HelixDB",
                "provision a Helixir-managed local HelixDB",
                "connect to a remote HelixDB",
            ]
        } else {
            vec![
                "provision a Helixir-managed local HelixDB",
                "connect to a remote HelixDB",
            ]
        };
        let selected = Select::new()
            .with_prompt("Backend")
            .default(0)
            .items(&options)
            .interact()?;
        let remote_selected = options[selected].starts_with("connect");
        if remote_selected {
            let host = Input::<String>::new()
                .with_prompt("Remote HelixDB host")
                .interact_text()?;
            let port = Input::<u16>::new()
                .with_prompt("Remote HelixDB port")
                .default(helixir::DEFAULT_HELIX_PORT)
                .interact_text()?;
            BackendChoice::JoinRemote { host, port }
        } else if detected && selected == 0 {
            BackendChoice::ReuseDetected
        } else {
            BackendChoice::ProvisionLocal
        }
    } else if matches!(state.backend, helixir::installer::BackendState::Missing) {
        BackendChoice::ProvisionLocal
    } else {
        BackendChoice::ReuseDetected
    };

    let mut options = InstallOptions {
        mode: effective_mode,
        backend,
        clients: state.client_registered.keys().copied().collect(),
        ..InstallOptions::default()
    };

    let use_remote_embeddings = if model_args.remote_embeddings {
        true
    } else if interactive {
        Select::new()
            .with_prompt("Embedding runtime")
            .default(0)
            .items(&[
                "install/use Ollama + nomic-embed-text (recommended)",
                "configure an OpenAI-compatible remote embedding service",
            ])
            .interact()?
            == 1
    } else {
        false
    };
    options.embeddings = if use_remote_embeddings {
        EmbeddingChoice::Remote(gather_remote_embedding_config(model_args, interactive)?)
    } else {
        EmbeddingChoice::LocalOllamaNomic
    };

    let recommendation = helixir::installer::models::recommend_local_llm(total_memory_bytes());
    options.local_llm_model = if model_args.no_local_llm {
        None
    } else if let Some(model) = &model_args.local_llm_model {
        Some(model.clone())
    } else if use_remote_embeddings {
        None
    } else {
        Some(recommendation.model.to_string())
    };

    if interactive {
        let local_llm = if model_args.local_llm_model.is_some() || model_args.no_local_llm {
            options.local_llm_model.is_some()
        } else {
            Confirm::new()
                .with_prompt(format!(
                    "Provision a local fallback LLM? Recommended: {} ({}, {})",
                    recommendation.model, recommendation.download, recommendation.rationale
                ))
                .default(!use_remote_embeddings)
                .interact()?
        };
        if local_llm {
            let model = match &model_args.local_llm_model {
                Some(model) => model.clone(),
                None => Input::<String>::new()
                    .with_prompt("Local LLM model")
                    .default(recommendation.model.to_string())
                    .interact_text()?,
            };
            options.local_llm_model = Some(model);
        } else {
            options.local_llm_model = None;
        }

        let available: Vec<_> = state.client_registered.keys().copied().collect();
        if !available.is_empty() {
            let labels: Vec<_> = available.iter().map(|client| client.label()).collect();
            let selected = MultiSelect::new()
                .with_prompt("Register Helixir in which clients?")
                .items(&labels)
                .defaults(&vec![true; available.len()])
                .interact()?;
            options.clients = selected.into_iter().map(|idx| available[idx]).collect();
        } else {
            options.clients = BTreeSet::new();
        }
    }

    let operator_default = security_args
        .rbac_operator
        .clone()
        .or_else(|| std::env::var("HELIXIR_RBAC_ACTOR").ok())
        .or_else(|| std::env::var("USER").ok())
        .or_else(|| std::env::var("USERNAME").ok())
        .unwrap_or_else(|| "helixir-operator".to_string());
    let operator_id = if interactive && security_args.rbac_operator.is_none() {
        Input::<String>::new()
            .with_prompt("Initial RBAC administrator id")
            .default(operator_default)
            .interact_text()?
    } else {
        operator_default
    };
    anyhow::ensure!(
        !operator_id.trim().is_empty(),
        "RBAC operator id cannot be empty"
    );
    let mut principals = security_args
        .rbac_principals
        .iter()
        .map(|principal| principal.trim())
        .filter(|principal| !principal.is_empty())
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    principals.extend(
        options
            .clients
            .iter()
            .map(|client| client.principal_id().to_string()),
    );
    principals.insert(operator_id.trim().to_string());
    options.rbac = helixir::installer::rbac::RbacInstallOptions {
        operator_id: operator_id.trim().to_string(),
        principals,
    };

    Ok(options)
}

pub(crate) fn gather_remote_embedding_config(
    model_args: &OnboardModelArgs,
    interactive: bool,
) -> Result<helixir::installer::RemoteEmbeddingConfig> {
    let current = helixir::core::config::HelixirConfig::from_env();
    let current_is_remote = !current.embedding_provider.eq_ignore_ascii_case("ollama");

    let provider = remote_embedding_value(
        model_args.embedding_provider.clone(),
        "HELIX_EMBEDDING_PROVIDER",
        current_is_remote.then(|| current.embedding_provider.clone()),
        "Remote embedding provider",
        interactive,
        Some("openai"),
    )?
    .to_lowercase();
    anyhow::ensure!(
        provider == "openai",
        "remote embeddings currently require the openai adapter for an OpenAI-compatible API"
    );
    let model = remote_embedding_value(
        model_args.embedding_model.clone(),
        "HELIX_EMBEDDING_MODEL",
        current_is_remote.then(|| current.embedding_model.clone()),
        "Remote embedding model",
        interactive,
        None,
    )?;
    let url = remote_embedding_value(
        model_args.embedding_url.clone(),
        "HELIX_EMBEDDING_URL",
        current_is_remote.then(|| current.embedding_url.clone()),
        "Remote embedding API root",
        interactive,
        None,
    )?;
    let parsed = reqwest::Url::parse(&url).context("remote embedding URL is invalid")?;
    anyhow::ensure!(
        matches!(parsed.scheme(), "http" | "https"),
        "remote embedding URL must use http or https"
    );

    let existing_key = std::env::var("HELIX_EMBEDDING_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            current_is_remote
                .then_some(current.embedding_api_key)
                .flatten()
        });
    let api_key = match existing_key {
        Some(key) => key,
        None if interactive => Password::new()
            .with_prompt("Remote embedding API key")
            .interact()?,
        None => anyhow::bail!(
            "--remote-embeddings requires HELIX_EMBEDDING_API_KEY or a key in the protected central config"
        ),
    };
    anyhow::ensure!(
        !api_key.trim().is_empty(),
        "remote embedding API key is empty"
    );

    Ok(helixir::installer::RemoteEmbeddingConfig {
        provider,
        model,
        url: url.trim_end_matches('/').to_string(),
        api_key,
    })
}

pub(crate) fn remote_embedding_value(
    cli_value: Option<String>,
    env_name: &str,
    current_value: Option<String>,
    prompt: &str,
    interactive: bool,
    default: Option<&str>,
) -> Result<String> {
    let found = cli_value
        .or_else(|| std::env::var(env_name).ok())
        .filter(|value| !value.trim().is_empty())
        .or(current_value.filter(|value| !value.trim().is_empty()));
    let value = match found {
        Some(value) => value,
        None if interactive => {
            let input = Input::<String>::new().with_prompt(prompt);
            match default {
                Some(default) => input.default(default.to_string()).interact_text()?,
                None => input.interact_text()?,
            }
        }
        None => anyhow::bail!(
            "--remote-embeddings requires an explicit {env_name} value or matching CLI option"
        ),
    };
    let value = value.trim().to_string();
    anyhow::ensure!(!value.is_empty(), "{prompt} cannot be empty");
    Ok(value)
}

pub(crate) fn total_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout).trim().parse().ok()
    }
    #[cfg(target_os = "linux")]
    {
        let contents = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kib = contents
            .lines()
            .find(|line| line.starts_with("MemTotal:"))?
            .split_whitespace()
            .nth(1)?
            .parse::<u64>()
            .ok()?;
        kib.checked_mul(1024)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

pub(crate) fn install_action_label(action: &helixir::installer::InstallAction) -> String {
    use helixir::installer::InstallAction;
    match action {
        InstallAction::ProvisionBackend => "Provision persistent HelixDB".to_string(),
        InstallAction::StartBackend => "Start detected HelixDB".to_string(),
        InstallAction::BackupBackend => "Back up HelixDB before schema transition".to_string(),
        InstallAction::DeploySchema => "Deploy compatible Helixir schema".to_string(),
        InstallAction::VerifyBackend => "Verify backend health and schema".to_string(),
        InstallAction::InstallOllama => "Install Ollama".to_string(),
        InstallAction::StartOllama => "Start Ollama".to_string(),
        InstallAction::PullOllamaModel(model) => format!("Pull Ollama model {model}"),
        InstallAction::DownloadNli => "Download and verify NLI model".to_string(),
        InstallAction::WriteCentralConfig => "Write protected ~/.helixir/helixir.toml".to_string(),
        InstallAction::BootstrapRbac { .. } => {
            "Converge permanent default/onboarding/Moirai RBAC workspaces".to_string()
        }
        InstallAction::RegisterClient(client) => {
            format!("Register helixir-local in {}", client.label())
        }
        InstallAction::InstallAgentSkill(_) => {
            "Install canonical Helixir memory/RBAC Agent Skill".to_string()
        }
        InstallAction::RunDoctor => "Run helixir doctor with embedding recovery".to_string(),
    }
}
