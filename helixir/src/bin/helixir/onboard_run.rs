use super::*;

pub(crate) async fn onboard_run(
    interactive: bool,
    dry_run: bool,
    mode: Option<String>,
    model_args: OnboardModelArgs,
    backend_args: OnboardBackendArgs,
    security_args: OnboardSecurityArgs,
) -> Result<()> {
    println!("Helixir onboarding plan\n");
    let state = detect_onboard_state().await;
    let options = gather_onboard_options(
        &state,
        interactive,
        mode,
        &model_args,
        &backend_args,
        &security_args,
    )?;
    let plan = helixir::installer::Planner::build(&state, &options)
        .map_err(|error| anyhow::anyhow!("cannot build a safe install plan: {error}"))?;

    println!("Selected tier: {}", options.mode.label());
    match &options.local_llm_model {
        Some(model) => println!(
            "Local fallback LLM: {model} ({})",
            helixir::installer::models::download_estimate(model).unwrap_or("size unknown")
        ),
        None => println!("Local fallback LLM: skipped"),
    }
    match &options.embeddings {
        helixir::installer::EmbeddingChoice::LocalOllamaNomic => {
            println!("Embedding runtime: Ollama");
            println!(
                "Embedding model: {} ({})",
                helixir::DEFAULT_EMBEDDING_MODEL,
                helixir::installer::models::download_estimate(helixir::DEFAULT_EMBEDDING_MODEL)
                    .unwrap_or("size unknown")
            );
        }
        helixir::installer::EmbeddingChoice::Remote(remote) => {
            println!(
                "Embedding runtime: remote {} at {}",
                remote.provider, remote.url
            );
            println!("Embedding model: {}", remote.model);
            println!("Embedding recovery: Ollama + nomic-embed-text on doctor failure");
        }
    }
    println!("Local NLI judge: required");
    println!(
        "RBAC: permanent (operator {}, legacy group {}, admission group {})",
        options.rbac.operator_id,
        helixir::core::DEFAULT_GROUP_ID,
        helixir::core::ONBOARDING_GROUP_ID
    );
    println!("\nOrdered actions:");
    for (index, step) in plan.steps.iter().enumerate() {
        println!(
            "  {:>2}. {} — {}",
            index + 1,
            install_action_label(&step.action),
            step.reason
        );
    }

    if dry_run {
        println!("\nDry run: no system changes were made.");
    } else {
        println!("\nApplying plan...");
        let executor = OnboardExecutor::new(&options, &state, interactive);
        let report = helixir::installer::apply_plan(&executor, &plan).await;
        for step in &report.steps {
            let marker = if step.succeeded { "✓" } else { "✗" };
            println!("  {marker} {}", install_action_label(&step.action));
            if let Some(detail) = &step.detail {
                println!("      {detail}");
            }
        }
        if report.rollback_attempted {
            println!("  rollback: attempted");
        }
        if let Some(error) = report.rollback_error {
            println!("  rollback error: {error}");
        }
        anyhow::ensure!(
            report.ready,
            "onboarding failed; inspect the step report above"
        );
        write_install_manifest(&executor.effective_options(), executor.backend_manifest()?)?;
        println!("\nOnboarding complete. Run `helixir doctor` to re-check readiness.");
    }
    Ok(())
}

pub(crate) fn write_install_manifest(
    options: &helixir::installer::InstallOptions,
    backend: helixir::installer::manifest::BackendManifest,
) -> Result<()> {
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
            matches!(
                options.embeddings,
                helixir::installer::EmbeddingChoice::LocalOllamaNomic
            )
            .then(|| helixir::DEFAULT_EMBEDDING_MODEL.to_string()),
        )
        .collect();
    let clients = options
        .clients
        .iter()
        .map(|client| client.label().to_string())
        .collect();
    let manifest = helixir::installer::manifest::InstallManifest {
        version: env!("CARGO_PKG_VERSION").to_string(),
        install_dir,
        backend_volume: backend.volume.clone(),
        backend,
        models,
        clients,
        rbac: Some(helixir::installer::rbac::RbacManifest {
            enabled: true,
            operator_id: options.rbac.operator_id.clone(),
            group_id: helixir::core::DEFAULT_GROUP_ID.to_string(),
            principals: options.rbac.principals.iter().cloned().collect(),
        }),
        last_backup: None,
    };
    helixir::installer::manifest::write(&home.join(".helixir/install.json"), &manifest)
        .map_err(Into::into)
}

// The concrete executor is implemented in the adjacent module.
