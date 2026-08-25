use super::*;

#[derive(Debug, Clone)]
pub(super) struct PreviousBackendIdentity {
    pub(super) image_id: String,
    pub(super) engine_revision: String,
    pub(super) schema_fingerprint: String,
}

impl NativeInstallExecutor {
    /// Resolve the concrete backend and rollback state from detected state and choices.
    #[must_use]
    pub fn new(
        options: &helixir::installer::InstallOptions,
        state: &helixir::installer::SystemState,
    ) -> Self {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
        let schema_dir = schema_dir_for_install();
        let project_dir = schema_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let descriptor_result =
            helixir::installer::backend::BackendImageDescriptor::load(&project_dir);
        let (backend_descriptor, backend_descriptor_error) = match descriptor_result {
            Ok(descriptor) => (descriptor, None),
            Err(error) => (None, Some(error.to_string())),
        };
        let schema_fingerprint =
            helixir::installer::backend::schema_fingerprint(&schema_dir).unwrap_or_default();
        let configured = helixir::core::config::HelixirConfig::from_env();
        let (mut backend, managed_backend, recreate_managed_backend) =
            match (&options.backend, &state.backend) {
                (
                    helixir::installer::BackendChoice::ReuseDetected,
                    helixir::installer::BackendState::ManagedLocal {
                        host,
                        port,
                        container,
                        volume,
                        image,
                        ..
                    },
                ) => (
                    helixir::installer::backend::BackendSpec {
                        host: host.clone(),
                        port: *port,
                        container: container.clone(),
                        volume: volume.clone(),
                        image: image.clone(),
                        engine_revision: String::new(),
                        schema_fingerprint: String::new(),
                        schema_dir,
                        project_dir,
                    },
                    true,
                    true,
                ),
                (
                    helixir::installer::BackendChoice::ReuseDetected,
                    helixir::installer::BackendState::ExistingLocal { host, port, .. }
                    | helixir::installer::BackendState::Remote { host, port, .. },
                ) => (
                    helixir::installer::backend::BackendSpec {
                        host: host.clone(),
                        port: *port,
                        schema_dir,
                        project_dir,
                        ..Default::default()
                    },
                    false,
                    false,
                ),
                (helixir::installer::BackendChoice::JoinRemote { host, port }, _) => (
                    helixir::installer::backend::BackendSpec {
                        host: host.clone(),
                        port: *port,
                        schema_dir,
                        project_dir,
                        ..Default::default()
                    },
                    false,
                    false,
                ),
                _ => (
                    helixir::installer::backend::BackendSpec {
                        host: "localhost".to_string(),
                        port: configured.port,
                        schema_dir,
                        project_dir,
                        ..Default::default()
                    },
                    true,
                    false,
                ),
            };
        backend.engine_revision = helixir::installer::backend::ENGINE_REVISION.to_string();
        backend.schema_fingerprint = schema_fingerprint;
        if managed_backend && let Some(descriptor) = backend_descriptor.as_ref() {
            backend.image = descriptor.image.clone();
        }
        Self {
            options: options.clone(),
            backend,
            backup_dir: home.join(".helixir/backups"),
            backup_name: format!(
                "helixdb-{}.tar.gz",
                chrono::Utc::now().format("%Y%m%d-%H%M%S")
            ),
            embedding_repaired: std::sync::atomic::AtomicBool::new(false),
            managed_backend,
            recreate_managed_backend,
            previous_backend: std::sync::Mutex::new(None),
            backend_descriptor,
            backend_descriptor_error,
        }
    }

    pub(crate) fn backend_manifest(&self) -> Result<helixir::installer::manifest::BackendManifest> {
        let kind = if self.managed_backend {
            "managed_local"
        } else if matches!(
            self.options.backend,
            helixir::installer::BackendChoice::JoinRemote { .. }
        ) || !matches!(
            self.backend.host.trim().to_ascii_lowercase().as_str(),
            "localhost" | "127.0.0.1" | "::1"
        ) {
            "remote"
        } else {
            "existing_local"
        };
        let (container, image, volume) = if self.managed_backend {
            (
                self.backend.container.clone(),
                self.backend.image.clone(),
                self.backend.volume.clone(),
            )
        } else {
            (String::new(), String::new(), String::new())
        };
        Ok(helixir::installer::manifest::BackendManifest {
            kind: kind.to_string(),
            host: self.backend.host.clone(),
            port: self.backend.port,
            container,
            image,
            volume,
            helix_cli_version: helixir::installer::backend::HELIX_CLI_VERSION.to_string(),
            engine_revision: if self.managed_backend {
                self.backend.engine_revision.clone()
            } else {
                String::new()
            },
            schema_fingerprint: helixir::installer::backend::schema_fingerprint(
                &self.backend.schema_dir,
            )?,
        })
    }

    pub(super) fn run(program: &str, args: &[String]) -> Result<()> {
        let status = Command::new(program)
            .args(args)
            .status()
            .with_context(|| format!("run {program}"))?;
        anyhow::ensure!(status.success(), "{program} exited with {status}");
        Ok(())
    }

    fn run_helix_in(program: &Path, fork: &Path, args: &[String], directory: &Path) -> Result<()> {
        let status = Command::new(program)
            .args(args)
            .current_dir(directory)
            .env("HELIX_REPO_PATH", fork)
            .status()
            .with_context(|| format!("run {} in {}", program.display(), directory.display()))?;
        anyhow::ensure!(
            status.success(),
            "{} exited with {status}",
            program.display()
        );
        Ok(())
    }

    fn capture_previous_image(&self) -> Result<()> {
        let output = Command::new("docker")
            .args(["inspect", &self.backend.container])
            .output()
            .context("inspect current managed HelixDB container image")?;
        if !output.status.success() {
            anyhow::ensure!(
                !self.recreate_managed_backend,
                "cannot capture the previous managed HelixDB identity before replacement"
            );
            return Ok(());
        }
        let rows: Vec<serde_json::Value> =
            serde_json::from_slice(&output.stdout).context("decode managed backend inspection")?;
        let row = rows
            .first()
            .context("Docker returned no managed backend inspection row")?;
        let image_id = row["Image"]
            .as_str()
            .filter(|value| !value.is_empty())
            .context("managed backend inspection has no immutable image id")?;
        let label = |name: &str| {
            row["Config"]["Labels"][name]
                .as_str()
                .unwrap_or_default()
                .to_string()
        };
        *self
            .previous_backend
            .lock()
            .map_err(|_| anyhow::anyhow!("backend image rollback state lock is poisoned"))? =
            Some(PreviousBackendIdentity {
                image_id: image_id.to_string(),
                engine_revision: label("io.helixir.engine-revision"),
                schema_fingerprint: label("io.helixir.schema-fingerprint"),
            });
        Ok(())
    }

    pub(super) fn build_managed_backend_image(&self) -> Result<()> {
        anyhow::ensure!(
            self.managed_backend,
            "external backends are never schema-mutated locally"
        );
        if let Some(error) = self.backend_descriptor_error.as_deref() {
            anyhow::bail!("invalid packaged managed-backend descriptor: {error}");
        }
        self.capture_previous_image()?;
        if let Some(descriptor) = self.backend_descriptor.as_ref() {
            anyhow::ensure!(
                descriptor.image == self.backend.image,
                "managed backend image drifted from the release descriptor"
            );
            Self::run("docker", &["pull".to_string(), descriptor.image.clone()])?;
        } else {
            let (helix, fork) = local_fork_helix_cli(&self.backend.project_dir)?;
            let version = Command::new(&helix)
                .arg("--version")
                .output()
                .context("run maintained local HelixDB CLI")?;
            let version_text = String::from_utf8_lossy(&version.stdout);
            anyhow::ensure!(
                version.status.success()
                    && version_text.contains(helixir::installer::backend::HELIX_CLI_VERSION),
                "maintained HelixDB CLI {} is required; found {}",
                helixir::installer::backend::HELIX_CLI_VERSION,
                version_text.trim()
            );
            Self::run_helix_in(
                &helix,
                &fork,
                &helixir::installer::backend::check_schema(),
                &self.backend.project_dir,
            )?;
            Self::run_helix_in(
                &helix,
                &fork,
                &helixir::installer::backend::build_image(),
                &self.backend.project_dir,
            )?;
        }
        if self.recreate_managed_backend {
            let _ = self.run_docker(helixir::installer::backend::stop(&self.backend));
            self.run_docker(helixir::installer::backend::remove(&self.backend))?;
            self.run_docker(helixir::installer::backend::provision(&self.backend))?;
        }
        Ok(())
    }

    pub(super) fn run_docker(
        &self,
        command: helixir::installer::backend::DockerCommand,
    ) -> Result<()> {
        Self::run("docker", &command.args)
    }

    pub(super) fn verify_managed_backend_contract(&self) -> Result<()> {
        let output = Command::new("docker")
            .args(["inspect", &self.backend.container])
            .output()
            .context("inspect managed HelixDB container")?;
        anyhow::ensure!(
            output.status.success(),
            "managed HelixDB container {} is not inspectable",
            self.backend.container
        );
        let containers: serde_json::Value = serde_json::from_slice(&output.stdout)
            .context("decode managed HelixDB container inspection")?;
        let container = containers
            .as_array()
            .and_then(|items| items.first())
            .context("Docker returned no managed HelixDB container")?;
        let target = Command::new("docker")
            .args([
                "image",
                "inspect",
                "--format",
                "{{.Id}}",
                &self.backend.image,
            ])
            .output()
            .context("inspect release-pinned managed HelixDB image")?;
        anyhow::ensure!(
            target.status.success(),
            "release-pinned managed HelixDB image is not present"
        );
        let target_id = String::from_utf8_lossy(&target.stdout).trim().to_string();
        anyhow::ensure!(
            !target_id.is_empty() && container["Image"].as_str() == Some(target_id.as_str()),
            "managed HelixDB container does not run the release-pinned image"
        );

        let persistent_mount = container["Mounts"].as_array().is_some_and(|mounts| {
            mounts.iter().any(|mount| {
                mount["Destination"].as_str() == Some("/data")
                    && mount["Name"].as_str() == Some(self.backend.volume.as_str())
            })
        });
        anyhow::ensure!(
            persistent_mount,
            "managed HelixDB container does not mount volume {} at /data",
            self.backend.volume
        );

        let environment = container["Config"]["Env"]
            .as_array()
            .context("managed HelixDB inspection has no environment")?;
        let has_env = |expected: &str| {
            environment
                .iter()
                .any(|value| value.as_str() == Some(expected))
        };
        let data_dir = has_env("HELIX_DATA_DIR=/data");
        anyhow::ensure!(
            data_dir,
            "managed HelixDB does not persist data under /data"
        );
        let labels = container["Config"]["Labels"]
            .as_object()
            .context("managed HelixDB inspection has no labels")?;
        anyhow::ensure!(
            labels
                .get("io.helixir.engine-revision")
                .and_then(serde_json::Value::as_str)
                == Some(self.backend.engine_revision.as_str()),
            "managed HelixDB engine revision does not match this Helixir build"
        );
        anyhow::ensure!(
            labels
                .get("io.helixir.schema-fingerprint")
                .and_then(serde_json::Value::as_str)
                == Some(self.backend.schema_fingerprint.as_str()),
            "managed HelixDB schema fingerprint label does not match this Helixir build"
        );
        for expected in [
            format!(
                "HELIX_CORES_OVERRIDE={}",
                helixir::installer::backend::MANAGED_HELIX_CORES
            ),
            format!(
                "HELIX_WORKERS_PER_CORE={}",
                helixir::installer::backend::MANAGED_HELIX_WORKERS_PER_CORE
            ),
            format!(
                "MIMALLOC_PURGE_DELAY={}",
                helixir::installer::backend::MIMALLOC_PURGE_DELAY
            ),
            format!(
                "MIMALLOC_PURGE_DECOMMITS={}",
                helixir::installer::backend::MIMALLOC_PURGE_DECOMMITS
            ),
            format!(
                "MIMALLOC_ARENA_PURGE_MULT={}",
                helixir::installer::backend::MIMALLOC_ARENA_PURGE_MULT
            ),
        ] {
            anyhow::ensure!(
                has_env(&expected),
                "managed HelixDB resource policy is missing {expected}"
            );
        }

        anyhow::ensure!(
            container["HostConfig"]["Memory"].as_i64()
                == Some(helixir::installer::backend::MANAGED_MEMORY_LIMIT_BYTES),
            "managed HelixDB memory limit must be {}",
            helixir::installer::backend::MANAGED_MEMORY_LIMIT
        );
        anyhow::ensure!(
            container["HostConfig"]["MemorySwap"].as_i64()
                == Some(helixir::installer::backend::MANAGED_MEMORY_LIMIT_BYTES),
            "managed HelixDB memory+swap limit must be {}",
            helixir::installer::backend::MANAGED_MEMORY_LIMIT
        );

        let port_key = format!("{}/tcp", self.backend.port);
        let expected_port = self.backend.port.to_string();
        let published_port = container["HostConfig"]["PortBindings"][&port_key]
            .as_array()
            .is_some_and(|bindings| {
                bindings
                    .iter()
                    .any(|binding| binding["HostPort"].as_str() == Some(expected_port.as_str()))
            });
        anyhow::ensure!(
            published_port,
            "managed HelixDB does not publish port {}",
            self.backend.port
        );
        Ok(())
    }

    pub(super) fn rollback_backend_if_uncommitted(
        &self,
        completed: &[helixir::installer::InstallAction],
    ) -> Result<()> {
        use helixir::installer::InstallAction;

        let backup_exists = completed
            .iter()
            .any(|action| matches!(action, InstallAction::BackupBackend));
        let backend_committed = completed
            .iter()
            .any(|action| matches!(action, InstallAction::VerifyBackend));
        if !backup_exists || backend_committed {
            return Ok(());
        }

        let _ = self.run_docker(helixir::installer::backend::stop(&self.backend));
        let _ = self.run_docker(helixir::installer::backend::remove(&self.backend));
        let mut rollback_backend = self.backend.clone();
        if let Some(previous) = self
            .previous_backend
            .lock()
            .map_err(|_| anyhow::anyhow!("backend image rollback state lock is poisoned"))?
            .clone()
        {
            rollback_backend.image = previous.image_id;
            rollback_backend.engine_revision = previous.engine_revision;
            rollback_backend.schema_fingerprint = previous.schema_fingerprint;
        }
        self.run_docker(helixir::installer::backend::clear_volume(&rollback_backend))?;
        self.run_docker(helixir::installer::backend::restore(
            &rollback_backend,
            &self.backup_dir,
            &self.backup_name,
        ))?;
        self.run_docker(helixir::installer::backend::provision(&rollback_backend))
    }
}

fn local_fork_helix_cli(project_dir: &Path) -> Result<(PathBuf, PathBuf)> {
    for root in project_dir.ancestors() {
        let fork = root.join("helixdb");
        let candidate = fork.join("target/release/helix");
        if candidate.is_file() {
            return Ok((candidate, fork));
        }
    }
    anyhow::bail!(
        "release backend descriptor is missing and the maintained local HelixDB CLI was not built; run `make build-helixdb-cli`"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuse_remote_preserves_the_detected_endpoint() {
        let options = helixir::installer::InstallOptions {
            backend: helixir::installer::BackendChoice::ReuseDetected,
            ..Default::default()
        };
        let state = helixir::installer::SystemState {
            backend: helixir::installer::BackendState::Remote {
                host: "helix.internal".to_string(),
                port: 7443,
                healthy: true,
                schema_compatible: true,
            },
            ..Default::default()
        };

        let executor = NativeInstallExecutor::new(&options, &state);

        assert_eq!(executor.backend.host, "helix.internal");
        assert_eq!(executor.backend.port, 7443);
        assert!(!executor.managed_backend);
    }
}
