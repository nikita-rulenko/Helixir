use crate::commands::integrations::fly::FlyManager;
use crate::config::CloudConfig;
use crate::docker::DockerManager;
use crate::output::{Operation, Step};
use crate::project::ProjectContext;
use crate::prompts;
use eyre::{OptionExt, Result};

pub async fn run(instance_name: Option<String>) -> Result<()> {
    // Load project context
    let project = ProjectContext::find_and_load(None)?;

    // Get instance name - prompt if not provided
    let instance_name = match instance_name {
        Some(name) => name,
        None if prompts::is_interactive() => {
            let instances = project.config.list_instances_with_types();
            prompts::select_instance(&instances)?
        }
        None => {
            let instances = project.config.list_instances();
            return Err(eyre::eyre!(
                "No instance specified. Available instances: {}",
                instances
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    };

    // Get instance config
    let instance_config = project.config.get_instance(&instance_name)?;

    if instance_config.is_local() {
        restart_local_instance(&project, &instance_name).await
    } else {
        restart_cloud_instance(&project, &instance_name, instance_config.into()).await
    }
}

async fn restart_local_instance(project: &ProjectContext, instance_name: &str) -> Result<()> {
    let op = Operation::new("Restarting", instance_name);

    let docker = DockerManager::new(project);

    // Check Docker availability
    DockerManager::check_runtime_available(docker.runtime)?;

    // Check if instance is built (has docker-compose.yml)
    let workspace = project.instance_workspace(instance_name);
    let compose_file = workspace.join("docker-compose.yml");

    if !compose_file.exists() {
        op.failure();
        let error = crate::errors::CliError::new(format!(
            "instance '{instance_name}' has not been built yet"
        ))
        .with_hint(format!(
            "run 'helix build {instance_name}' first to build the instance"
        ));
        return Err(eyre::eyre!("{}", error.render()));
    }

    // Restart the instance
    let mut restart_step = Step::with_messages("Restarting container", "Container restarted");
    restart_step.start();
    docker.restart_instance(instance_name)?;
    restart_step.done();

    op.success();

    Ok(())
}

async fn restart_cloud_instance(
    project: &ProjectContext,
    instance_name: &str,
    instance_config: CloudConfig,
) -> Result<()> {
    let op = Operation::new("Restarting", instance_name);

    let _cluster_id = instance_config.get_cluster_id().ok_or_eyre(format!(
        "Cloud instance '{instance_name}' must have a cluster_id"
    ))?;

    let mut restart_step =
        Step::with_messages("Restarting cloud instance", "Cloud instance restarted");
    restart_step.start();

    match instance_config {
        CloudConfig::FlyIo(config) => {
            Step::verbose_substep("Stopping Fly.io instance...");
            let fly = FlyManager::new(project, config.auth_type.clone()).await?;
            fly.stop_instance(instance_name).await?;

            Step::verbose_substep("Starting Fly.io instance...");
            fly.start_instance(instance_name).await?;
        }
        CloudConfig::Helix(_config) => {
            todo!()
        }
        CloudConfig::Ecr(_config) => {
            unimplemented!()
        }
    }

    restart_step.done();
    op.success();

    Ok(())
}
