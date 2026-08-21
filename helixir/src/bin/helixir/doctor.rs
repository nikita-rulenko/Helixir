use super::*;

pub(crate) async fn doctor_run(json_output: bool) -> Result<()> {
    let config = helixir::core::config::HelixirConfig::from_env();
    let selected_embeddings = helixir::installer::executor::configured_embedding_choice(&config);
    let embedding_probe = match &selected_embeddings {
        Ok(choice) => helixir::installer::executor::probe_embedding_choice(choice).await,
        Err(error) => Err(anyhow::anyhow!(error.to_string())),
    };
    let repaired_embeddings = if let Err(error) = embedding_probe {
        let options = helixir::installer::InstallOptions {
            local_llm_model: None,
            embeddings: helixir::installer::EmbeddingChoice::LocalOllamaNomic,
            ..helixir::installer::InstallOptions::default()
        };
        let executor = helixir::installer::executor::NativeInstallExecutor::new(
            &options,
            &helixir::installer::SystemState::default(),
        );
        helixir::installer::executor::repair_embeddings_with_local_fallback(
            &executor,
            &error.to_string(),
        )
        .await?;
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
    let mcp_binary = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("helixir-mcp")));
    let binaries_ready = mcp_binary.as_ref().is_some_and(|mcp| mcp.is_file());
    let state = detect_onboard_state().await;
    let backend_options = helixir::installer::InstallOptions {
        backend: helixir::installer::BackendChoice::ReuseDetected,
        local_llm_model: None,
        ..helixir::installer::InstallOptions::default()
    };
    let backend_executor =
        helixir::installer::executor::NativeInstallExecutor::new(&backend_options, &state);
    let backend_ready = helixir::installer::PlanExecutor::apply(
        &backend_executor,
        &helixir::installer::InstallAction::VerifyBackend,
    )
    .await
    .is_ok();
    let mcp_ready = mcp_binary
        .as_deref()
        .is_some_and(|binary| doctor_mcp_smoke(binary).is_ok());
    let rbac_ready = doctor_rbac_ready().await;
    let inputs = helixir::installer::doctor::DoctorInputs {
        binaries: Some(binaries_ready),
        config: Some(helixir::installer::executor::doctor_config_ready()),
        backend: Some(backend_ready),
        llm: Some(llm_ready),
        embeddings: Some(embeddings_ready),
        nomic: nomic_ready,
        nomic_required,
        nli: nli_ready,
        mcp: Some(mcp_ready),
        clients: Some(doctor_clients_ready()),
        rbac: Some(rbac_ready),
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

async fn doctor_rbac_ready() -> bool {
    use std::sync::Arc;

    let Some(port) = detect_local_backend_tcp() else {
        return false;
    };
    let config = helixir::core::config::HelixirConfig::from_env();
    let Ok(db) = helixir::db::HelixClient::new(&config.host, port) else {
        return false;
    };
    let manager = helixir::core::RbacManager::new(Arc::new(db));
    let Ok(state) = helixir::installer::rbac::inspect(&manager).await else {
        return false;
    };
    let manifest = std::env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|home| {
            helixir::installer::manifest::read(&home.join(".helixir/install.json"))
                .ok()
                .flatten()
        });
    match manifest.and_then(|manifest| manifest.rbac) {
        Some(expected) => state.satisfies(&helixir::installer::rbac::RbacInstallOptions {
            operator_id: expected.operator_id,
            principals: expected.principals.into_iter().collect(),
        }),
        None => {
            state.enabled
                && state.migration_active
                && state.default_group_exists
                && state.onboarding_group_exists
                && !state.global_admins.is_empty()
                && state.all_users_registered
                && state.legacy_memories_covered
        }
    }
}

pub(crate) fn doctor_clients_ready() -> bool {
    use helixir::installer::ClientKind;

    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let manifest = match helixir::installer::manifest::read(&home.join(".helixir/install.json")) {
        Ok(manifest) => manifest,
        Err(_) => return false,
    };
    let mut selected: Vec<ClientKind> = manifest
        .as_ref()
        .map(|manifest| {
            manifest
                .clients
                .iter()
                .filter_map(|label| client_kind_from_label(label))
                .collect()
        })
        .unwrap_or_default();

    let mcp_binary = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("helixir-mcp")))
        .unwrap_or_else(|| PathBuf::from("helixir-mcp"))
        .display()
        .to_string();
    if selected.is_empty() {
        selected = [
            ClientKind::ClaudeCode,
            ClientKind::Codex,
            ClientKind::Cursor,
        ]
        .into_iter()
        .filter(|client| client_available(*client))
        .collect();
    }
    if selected.is_empty() {
        return false;
    }
    selected.into_iter().all(|client| {
        let server = helixir::installer::clients::StdioServer::new(&mcp_binary)
            .with_env("HELIXIR_RBAC_ACTOR", client.principal_id());
        helixir::installer::client_registration::client_has_valid_helixir_registration(
            client,
            "helixir-local",
            &server,
        )
    })
}

fn client_kind_from_label(label: &str) -> Option<helixir::installer::ClientKind> {
    match label.trim().to_ascii_lowercase().as_str() {
        "claude code" | "claude" => Some(helixir::installer::ClientKind::ClaudeCode),
        "codex" => Some(helixir::installer::ClientKind::Codex),
        "cursor" => Some(helixir::installer::ClientKind::Cursor),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_client_labels_map_only_supported_onboarding_clients() {
        assert_eq!(
            client_kind_from_label("Claude Code"),
            Some(helixir::installer::ClientKind::ClaudeCode)
        );
        assert_eq!(
            client_kind_from_label("Codex"),
            Some(helixir::installer::ClientKind::Codex)
        );
        assert_eq!(
            client_kind_from_label("Cursor"),
            Some(helixir::installer::ClientKind::Cursor)
        );
        assert_eq!(client_kind_from_label("Gemini CLI"), None);
    }
}

// Client wiring is implemented in the adjacent `wire` module.
