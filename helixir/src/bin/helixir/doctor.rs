use super::*;

pub(crate) async fn doctor_run(json_output: bool) -> Result<()> {
    let config = helixir::core::config::HelixirConfig::from_env();
    let selected_embeddings = configured_embedding_choice(&config);
    let embedding_probe = match &selected_embeddings {
        Ok(choice) => probe_embedding_choice(choice).await,
        Err(error) => Err(anyhow::anyhow!(error.to_string())),
    };
    let repaired_embeddings = if let Err(error) = embedding_probe {
        let options = helixir::installer::InstallOptions {
            local_llm_model: None,
            embeddings: helixir::installer::EmbeddingChoice::LocalOllamaNomic,
            ..helixir::installer::InstallOptions::default()
        };
        let executor = OnboardExecutor::new(&options);
        repair_embeddings_with_local_fallback(&executor, &error.to_string()).await?;
        true
    } else {
        false
    };

    let (ollama_installed, ollama_running, models) = detect_ollama().await;
    let llm_ready = if config.llm_provider.eq_ignore_ascii_case("ollama") {
        ollama_running
            && helixir::installer::models::OllamaAdapter::has_model(&models, &config.llm_model)
    } else {
        config.llm_api_key.is_some()
    };
    let embeddings_ready = true;
    let nomic_required = repaired_embeddings
        || matches!(
            selected_embeddings,
            Ok(helixir::installer::EmbeddingChoice::LocalOllamaNomic)
        );
    let nomic_ready = nomic_required.then(|| {
        ollama_running
            && helixir::installer::models::OllamaAdapter::has_model(
                &models,
                helixir::DEFAULT_EMBEDDING_MODEL,
            )
    });
    let nli_ready = Some(onboard_nli_installed());
    let binaries_ready = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("helixir-mcp")))
        .map(|mcp| mcp.exists())
        .unwrap_or(false);
    let inputs = helixir::installer::doctor::DoctorInputs {
        binaries: Some(binaries_ready),
        config: Some(doctor_config_ready()),
        backend: Some(detect_local_backend_tcp().is_some()),
        llm: Some(llm_ready),
        embeddings: Some(embeddings_ready),
        nomic: nomic_ready,
        nomic_required,
        nli: nli_ready,
        mcp: Some(binaries_ready),
        clients: Some(doctor_clients_ready()),
    };
    let report = helixir::installer::doctor::DoctorReport::from_inputs(&inputs);
    if json_output {
        println!("{}", report.to_json()?);
    } else {
        println!("Helixir doctor (embedding recovery enabled)");
        for check in &report.checks {
            let marker = match check.status {
                helixir::installer::doctor::CheckStatus::Pass => "✓",
                helixir::installer::doctor::CheckStatus::Warn => "!",
                helixir::installer::doctor::CheckStatus::Skipped => "-",
                helixir::installer::doctor::CheckStatus::Fail => "✗",
            };
            println!("  {marker} {:<12} {}", check.name, check.detail);
        }
        println!("\nready: {}", report.ready);
        if !ollama_installed && !nomic_required {
            println!(
                "note: remote embeddings are healthy; local Ollama fallback is not installed."
            );
        }
    }
    if report.ready {
        Ok(())
    } else {
        anyhow::bail!("doctor found required components that are not ready")
    }
}

pub(crate) fn doctor_config_ready() -> bool {
    let path = helixir::core::config::HelixirConfig::config_file_path().or_else(|| {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".helixir/helixir.toml"))
    });
    let Some(path) = path else { return false };
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return false;
    };
    if toml::from_str::<helixir::core::config::HelixirConfig>(&contents).is_err() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.permissions().mode() & 0o077 == 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    true
}

pub(crate) fn doctor_clients_ready() -> bool {
    let mut selected = 0usize;
    let mut ready = true;
    for client in native_client_targets() {
        selected += 1;
        ready &= native_registration_exists(client, "helixir-local");
    }
    let server = helixir::installer::clients::StdioServer::new(
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.join("helixir-mcp")))
            .unwrap_or_else(|| PathBuf::from("helixir-mcp"))
            .display()
            .to_string(),
    );
    let expected_command = server.json_entry()["command"].clone();
    for (_, path) in client_targets() {
        if !path.exists() {
            continue;
        }
        selected += 1;
        let present = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .and_then(|doc| {
                doc.get("mcpServers")
                    .and_then(|servers| servers.get("helixir-local"))
                    .cloned()
            })
            .map(|entry| entry.get("command") == Some(&expected_command))
            .unwrap_or(false);
        ready &= present;
    }
    selected == 0 || ready
}

// Client wiring is implemented in the adjacent `wire` module.
