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
    let mut options = gather_onboard_options(
        &state,
        interactive,
        mode,
        &model_args,
        &backend_args,
        &security_args,
    )?;
    approve_client_registration_conflicts(&mut options, interactive)?;
    let service = helixir::installer::service::InstallerService::default();
    let prepared = service
        .prepare(&options)
        .await
        .map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))?;
    let plan = prepared.plan;

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
        let report = service
            .apply(&options)
            .await
            .map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))?;
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
        println!("\nOnboarding complete. Run `helixir doctor` to re-check readiness.");
    }
    Ok(())
}

pub(crate) async fn apply_install_json() -> Result<()> {
    use std::io::Read;

    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("read typed install options from supervisor")?;
    let options: helixir::installer::InstallOptions =
        serde_json::from_str(&input).context("decode typed install options")?;
    let service = helixir::installer::service::InstallerService::default();
    let observer = |event: helixir::installer::InstallEvent| {
        if let Ok(encoded) = serde_json::to_string(&event) {
            println!(
                "{}{}",
                helixir::installer::operation_worker::EVENT_PREFIX,
                encoded
            );
        }
    };
    let report = service
        .apply_observed(&options, &observer)
        .await
        .map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))?;
    println!(
        "{}{}",
        helixir::installer::operation_worker::REPORT_PREFIX,
        serde_json::to_string(&report).context("encode install report")?
    );
    anyhow::ensure!(report.ready, "installation plan did not reach readiness");
    Ok(())
}

fn approve_client_registration_conflicts(
    options: &mut helixir::installer::InstallOptions,
    interactive: bool,
) -> Result<()> {
    let conflicts =
        helixir::installer::client_registration::registration_conflicts(&options.clients)
            .map_err(anyhow::Error::msg)?;
    if conflicts.is_empty() {
        return Ok(());
    }
    anyhow::ensure!(
        interactive || options.replace_conflicting_clients,
        "conflicting MCP registrations require --interactive approval"
    );
    if options.replace_conflicting_clients {
        return Ok(());
    }
    for conflict in conflicts {
        println!(
            "{} helixir-local change:\n  old: {}\n  new: {}",
            conflict.client.label(),
            conflict.existing,
            conflict.requested
        );
        let approved = Confirm::new()
            .with_prompt(format!(
                "Replace {} helixir-local registration?",
                conflict.client.label()
            ))
            .default(false)
            .interact()?;
        anyhow::ensure!(
            approved,
            "{} registration replacement declined",
            conflict.client.label()
        );
    }
    options.replace_conflicting_clients = true;
    Ok(())
}
