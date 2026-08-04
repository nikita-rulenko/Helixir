use super::*;

pub(crate) struct OnboardExecutor {
    options: helixir::installer::InstallOptions,
    backend: helixir::installer::backend::BackendSpec,
    backup_dir: PathBuf,
    backup_name: String,
    embedding_repaired: std::sync::atomic::AtomicBool,
    rbac_enabled_before: std::sync::Mutex<Option<bool>>,
}

impl OnboardExecutor {
    pub(crate) fn new(options: &helixir::installer::InstallOptions) -> Self {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
        Self {
            options: options.clone(),
            backend: helixir::installer::backend::BackendSpec {
                host: match &options.backend {
                    helixir::installer::BackendChoice::JoinRemote { host, .. } => host.clone(),
                    _ => "localhost".to_string(),
                },
                schema_dir: schema_dir_for_install(),
                ..Default::default()
            },
            backup_dir: home.join(".helixir/backups"),
            backup_name: format!(
                "helixdb-{}.tar.gz",
                chrono::Utc::now().format("%Y%m%d-%H%M%S")
            ),
            embedding_repaired: std::sync::atomic::AtomicBool::new(false),
            rbac_enabled_before: std::sync::Mutex::new(None),
        }
    }

    pub(crate) fn effective_options(&self) -> helixir::installer::InstallOptions {
        let mut options = self.options.clone();
        if self
            .embedding_repaired
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            options.embeddings = helixir::installer::EmbeddingChoice::LocalOllamaNomic;
        }
        options
    }

    pub(crate) fn mark_embedding_repaired(&self) {
        self.embedding_repaired
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn run(program: &str, args: &[String]) -> Result<()> {
        let status = Command::new(program)
            .args(args)
            .status()
            .with_context(|| format!("run {program}"))?;
        anyhow::ensure!(status.success(), "{program} exited with {status}");
        Ok(())
    }

    fn run_docker(&self, command: helixir::installer::backend::DockerCommand) -> Result<()> {
        Self::run("docker", &command.args)
    }

    fn write_central_config(&self) -> Result<()> {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
        let path = home.join(".helixir/helixir.toml");
        let resolved = helixir::core::config::HelixirConfig::from_env();
        let mut patch = helixir::installer::config::ConfigPatch::default()
            .set("mode", format!("{:?}", self.options.mode))
            .set(
                "host",
                match &self.options.backend {
                    helixir::installer::BackendChoice::JoinRemote { host, .. } => host.clone(),
                    _ => "localhost".to_string(),
                },
            )
            .set("port", self.backend.port.to_string())
            .set("instance", "default");
        patch = match &self.options.embeddings {
            helixir::installer::EmbeddingChoice::LocalOllamaNomic => patch
                .set("embedding_provider", "ollama")
                .set("embedding_model", helixir::DEFAULT_EMBEDDING_MODEL)
                .set("embedding_url", helixir::DEFAULT_OLLAMA_URL),
            helixir::installer::EmbeddingChoice::Remote(remote) => patch
                .set("embedding_provider", &remote.provider)
                .set("embedding_model", &remote.model)
                .set("embedding_url", &remote.url)
                .set("embedding_api_key", &remote.api_key),
        };
        if let Some(model) = &self.options.local_llm_model {
            patch = patch.set("llm_provider", "ollama").set("llm_model", model);
        } else {
            patch = patch
                .set("llm_provider", &resolved.llm_provider)
                .set("llm_model", &resolved.llm_model);
            if let Some(base_url) = &resolved.llm_base_url {
                patch = patch.set("llm_base_url", base_url);
            }
            if let Some(key) = &resolved.llm_api_key {
                patch = patch.set("llm_api_key", key);
            }
        }
        helixir::installer::config::write_patch(&path, &patch)
            .map_err(Into::into)
            .map(|_| ())
    }

    pub(crate) async fn install_ollama(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            if resolve_program("brew").is_some() {
                return Self::run("brew", &["install".to_string(), "ollama".to_string()]);
            }

            let home = PathBuf::from(
                std::env::var("HOME").context("HOME is required to install Ollama.app")?,
            );
            let work = std::env::temp_dir().join(format!(
                "helixir-ollama-install-{}",
                uuid::Uuid::new_v4().simple()
            ));
            let archive = work.join("Ollama-darwin.zip");
            let unpacked = work.join("unpacked");
            let applications = home.join("Applications");
            std::fs::create_dir_all(&unpacked)?;
            std::fs::create_dir_all(&applications)?;
            let install_result = (|| {
                Self::run(
                    "curl",
                    &[
                        "-fL".to_string(),
                        "--retry".to_string(),
                        "5".to_string(),
                        "--continue-at".to_string(),
                        "-".to_string(),
                        "--output".to_string(),
                        archive.display().to_string(),
                        "https://ollama.com/download/Ollama-darwin.zip".to_string(),
                    ],
                )?;
                Self::run(
                    "ditto",
                    &[
                        "-x".to_string(),
                        "-k".to_string(),
                        archive.display().to_string(),
                        unpacked.display().to_string(),
                    ],
                )?;
                let source = unpacked.join("Ollama.app");
                anyhow::ensure!(source.is_dir(), "download did not contain Ollama.app");
                Self::run(
                    "ditto",
                    &[
                        source.display().to_string(),
                        applications.join("Ollama.app").display().to_string(),
                    ],
                )
            })();
            let _ = std::fs::remove_dir_all(&work);
            install_result
        }

        #[cfg(target_os = "linux")]
        {
            let script_path = std::env::temp_dir().join(format!(
                "helixir-ollama-install-{}.sh",
                uuid::Uuid::new_v4().simple()
            ));
            let result = async {
                let script = reqwest::get("https://ollama.com/install.sh")
                    .await
                    .context("download official Ollama installer")?
                    .error_for_status()
                    .context("official Ollama installer returned an error")?
                    .bytes()
                    .await
                    .context("read official Ollama installer")?;
                std::fs::write(&script_path, script)?;
                Self::run("sh", &[script_path.display().to_string()])
            }
            .await;
            let _ = std::fs::remove_file(&script_path);
            result
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            anyhow::bail!(
                "automatic Ollama installation is supported on macOS and Linux; install from https://ollama.com/download"
            )
        }
    }

    pub(crate) async fn start_ollama(&self) -> Result<()> {
        use helixir::installer::models::OllamaAdapter;

        if OllamaAdapter::api_ready(helixir::DEFAULT_OLLAMA_URL).await {
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
            let user_app = home.join("Applications/Ollama.app");
            let system_app = PathBuf::from("/Applications/Ollama.app");
            let app = [system_app, user_app]
                .into_iter()
                .find(|candidate| candidate.is_dir());
            if let Some(app) = app {
                let _ = Self::run("open", &[app.display().to_string()]);
            } else if resolve_program("brew").is_some() {
                let status = Command::new("brew")
                    .args(["services", "start", "ollama"])
                    .status();
                if !matches!(status, Ok(status) if status.success()) {
                    self.spawn_ollama_serve()?;
                }
            } else {
                self.spawn_ollama_serve()?;
            }
        }

        #[cfg(target_os = "linux")]
        {
            let started = resolve_program("systemctl")
                .and_then(|systemctl| {
                    Command::new(systemctl)
                        .args(["start", "ollama"])
                        .status()
                        .ok()
                })
                .is_some_and(|status| status.success());
            if !started {
                self.spawn_ollama_serve()?;
            }
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        self.spawn_ollama_serve()?;

        OllamaAdapter::wait_until_ready(helixir::DEFAULT_OLLAMA_URL, Duration::from_secs(30)).await
    }

    fn spawn_ollama_serve(&self) -> Result<()> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let binary = helixir::installer::models::OllamaAdapter::resolve_binary(home.as_deref())
            .context("Ollama binary was not found after installation")?;
        Command::new(binary)
            .arg("serve")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("start ollama serve")?;
        Ok(())
    }

    async fn verify_selected_models(&self) -> Result<()> {
        use helixir::installer::models::OllamaAdapter;

        anyhow::ensure!(onboard_nli_installed(), "mandatory NLI judge is not ready");
        let selected: Vec<&str> = self
            .options
            .local_llm_model
            .as_deref()
            .into_iter()
            .chain(
                matches!(
                    self.options.embeddings,
                    helixir::installer::EmbeddingChoice::LocalOllamaNomic
                )
                .then_some(helixir::DEFAULT_EMBEDDING_MODEL),
            )
            .collect();
        if !selected.is_empty() {
            let models = OllamaAdapter::list_api(helixir::DEFAULT_OLLAMA_URL).await?;
            for model in selected {
                anyhow::ensure!(
                    OllamaAdapter::has_model(&models, model),
                    "selected Ollama model {model} is not available"
                );
            }
        }
        if let Err(error) = probe_embedding_choice(&self.options.embeddings).await {
            repair_embeddings_with_local_fallback(self, &error.to_string()).await?;
        }
        Ok(())
    }

    async fn verify_selected_rbac(&self) -> Result<()> {
        let db = helixir::db::HelixClient::new(&self.backend.host, self.backend.port)?;
        let manager = helixir::core::RbacManager::new(std::sync::Arc::new(db));
        let state = helixir::installer::rbac::inspect(&manager).await?;
        anyhow::ensure!(
            state.satisfies(&self.options.rbac),
            "selected RBAC profile is not ready"
        );
        Ok(())
    }
}

#[async_trait::async_trait]
impl helixir::installer::PlanExecutor for OnboardExecutor {
    async fn apply(
        &self,
        action: &helixir::installer::InstallAction,
    ) -> std::result::Result<(), String> {
        use helixir::installer::InstallAction;
        let result: std::result::Result<(), String> = match action {
            InstallAction::ProvisionBackend => self
                .run_docker(helixir::installer::backend::provision(&self.backend))
                .map_err(|error| error.to_string()),
            InstallAction::StartBackend => self
                .run_docker(helixir::installer::backend::start(&self.backend))
                .map_err(|error| error.to_string()),
            InstallAction::BackupBackend => {
                std::fs::create_dir_all(&self.backup_dir).map_err(|error| error.to_string())?;
                self.run_docker(helixir::installer::backend::backup(
                    &self.backend,
                    &self.backup_dir,
                    &self.backup_name,
                ))
                .map_err(|error| error.to_string())
            }
            InstallAction::DeploySchema => {
                let deploy = current_sibling("helixir-deploy");
                let argv = helixir::installer::backend::deploy_schema(&deploy, &self.backend);
                Self::run(&argv[0], &argv[1..]).map_err(|error| error.to_string())
            }
            InstallAction::VerifyBackend => {
                if !backend_reachable(&self.backend.host, self.backend.port) {
                    return Err(format!(
                        "HelixDB is not reachable on port {}",
                        self.backend.port
                    ));
                }
                Ok(())
            }
            InstallAction::InstallOllama => self
                .install_ollama()
                .await
                .map_err(|error| error.to_string()),
            InstallAction::StartOllama => {
                self.start_ollama().await.map_err(|error| error.to_string())
            }
            InstallAction::PullOllamaModel(model) => {
                helixir::installer::models::OllamaAdapter::pull_and_verify(
                    helixir::DEFAULT_OLLAMA_URL,
                    model,
                    3,
                )
                .await
                .map_err(|error| error.to_string())
            }
            InstallAction::DownloadNli => {
                helixir::llm::nli::download(true)
                    .await
                    .map_err(|error| error.to_string())?;
                helixir::llm::nli::verify_readiness()
                    .map_err(|error| format!("verify downloaded NLI model: {error}"))?;
                Ok(())
            }
            InstallAction::WriteCentralConfig => self
                .write_central_config()
                .map_err(|error| error.to_string()),
            InstallAction::BootstrapRbac {
                operator_id,
                principals,
            } => {
                let db = helixir::db::HelixClient::new(&self.backend.host, self.backend.port)
                    .map_err(|error| error.to_string())?;
                let manager = helixir::core::RbacManager::new(std::sync::Arc::new(db));
                let enabled_before = manager
                    .snapshot()
                    .await
                    .map_err(|error| error.to_string())?
                    .enabled;
                *self
                    .rbac_enabled_before
                    .lock()
                    .map_err(|_| "RBAC rollback state lock is poisoned".to_string())? =
                    Some(enabled_before);
                manager
                    .bootstrap_compatibility(operator_id, principals)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(())
            }
            InstallAction::DisableRbac { operator_id } => {
                let db = helixir::db::HelixClient::new(&self.backend.host, self.backend.port)
                    .map_err(|error| error.to_string())?;
                let manager = helixir::core::RbacManager::new(std::sync::Arc::new(db));
                let enabled_before = manager
                    .snapshot()
                    .await
                    .map_err(|error| error.to_string())?
                    .enabled;
                *self
                    .rbac_enabled_before
                    .lock()
                    .map_err(|_| "RBAC rollback state lock is poisoned".to_string())? =
                    Some(enabled_before);
                manager
                    .set_enabled(false, operator_id)
                    .await
                    .map_err(|error| error.to_string())
            }
            InstallAction::RegisterClient(client) => register_onboard_client(*client),
            InstallAction::InstallAgentSkill(clients) => install_agent_skills(clients),
            InstallAction::RunDoctor => {
                if !doctor_config_ready() {
                    return Err("central config is not ready".to_string());
                }
                if detect_local_backend_tcp().is_none() {
                    return Err("backend verification failed".to_string());
                }
                self.verify_selected_models()
                    .await
                    .map_err(|error| error.to_string())?;
                self.verify_selected_rbac()
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(())
            }
        };
        result
    }

    async fn rollback(
        &self,
        completed: &[helixir::installer::InstallAction],
    ) -> std::result::Result<(), String> {
        use helixir::installer::InstallAction;
        if completed
            .iter()
            .any(|action| matches!(action, InstallAction::BootstrapRbac { .. }))
            && self
                .rbac_enabled_before
                .lock()
                .map_err(|_| "RBAC rollback state lock is poisoned".to_string())?
                .is_some_and(|enabled| !enabled)
        {
            let db = helixir::db::HelixClient::new(&self.backend.host, self.backend.port)
                .map_err(|error| error.to_string())?;
            let manager = helixir::core::RbacManager::new(std::sync::Arc::new(db));
            manager
                .set_enabled(false, &self.options.rbac.operator_id)
                .await
                .map_err(|error| error.to_string())?;
        }
        if completed
            .iter()
            .any(|action| matches!(action, InstallAction::DisableRbac { .. }))
            && self
                .rbac_enabled_before
                .lock()
                .map_err(|_| "RBAC rollback state lock is poisoned".to_string())?
                .is_some_and(|enabled| enabled)
        {
            let db = helixir::db::HelixClient::new(&self.backend.host, self.backend.port)
                .map_err(|error| error.to_string())?;
            let manager = helixir::core::RbacManager::new(std::sync::Arc::new(db));
            manager
                .set_enabled(true, &self.options.rbac.operator_id)
                .await
                .map_err(|error| error.to_string())?;
        }
        if completed
            .iter()
            .any(|action| matches!(action, InstallAction::BackupBackend))
        {
            let _ = self.run_docker(helixir::installer::backend::stop(&self.backend));
            self.run_docker(helixir::installer::backend::restore(
                &self.backend,
                &self.backup_dir,
                &self.backup_name,
            ))
            .map_err(|error| error.to_string())?;
            let _ = self.run_docker(helixir::installer::backend::start(&self.backend));
        }
        Ok(())
    }
}
