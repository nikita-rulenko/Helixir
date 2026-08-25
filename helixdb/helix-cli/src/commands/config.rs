use crate::commands::auth::require_auth;
use crate::commands::cloud_api::{
    CliProject, CliProjectClusters, CliProjectDetails, CliProjectEnterpriseCluster,
    CliProjectStandardCluster, CliWorkspace, CliWorkspaceClusters, CliWorkspaceMember,
    fetch_project_clusters, fetch_project_details, fetch_projects, fetch_workspace_clusters,
    fetch_workspace_members, fetch_workspaces, find_project_by_name, find_workspace_by_id,
    find_workspace_by_slug, workspace_prompt_items,
};
use crate::commands::integrations::helix::cloud_base_url;
use crate::config::{BuildMode, HelixConfig, WorkspaceConfig};
use crate::project::ProjectContext;
use crate::prompts;
use crate::{
    ClusterConfigAction, ConfigAction, ConfigOutputFormat, ProjectConfigAction,
    WorkspaceConfigAction,
};
use color_eyre::owo_colors::OwoColorize;
use eyre::{Result, eyre};
use serde::Serialize;
use std::path::Path;

const SECTION_RULE: &str = "----------------------------------------";
const CONTEXT_LABEL_WIDTH: usize = 11;

#[derive(Debug, Serialize)]
struct WorkspaceListItem {
    id: String,
    name: String,
    url_slug: String,
    workspace_type: String,
    current: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    members: Option<Vec<CliWorkspaceMember>>,
}

#[derive(Debug, Serialize)]
struct WorkspaceListOutput {
    current_workspace_id: Option<String>,
    workspaces: Vec<WorkspaceListItem>,
}

#[derive(Debug, Serialize)]
struct WorkspaceShowOutput {
    workspace: Option<WorkspaceListItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct MessageOutput {
    message: String,
}

#[derive(Debug, Serialize)]
struct ProjectListItem {
    id: String,
    name: String,
    current: bool,
}

#[derive(Debug, Serialize)]
struct ProjectListOutput {
    workspace: CliWorkspace,
    current_project_id: Option<String>,
    projects: Vec<ProjectListItem>,
}

#[derive(Debug, Serialize)]
struct ProjectShowOutput {
    project: Option<CliProjectDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct LocalInstanceSummary {
    name: String,
    port: Option<u16>,
    build_mode: BuildMode,
}

#[derive(Debug, Serialize)]
struct StandardClusterSummary {
    cluster_id: String,
    cluster_name: String,
    project_id: String,
    project_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    build_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_memory_gb: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_vcpus: Option<f32>,
}

#[derive(Debug, Serialize)]
struct EnterpriseClusterSummary {
    cluster_id: String,
    cluster_name: String,
    project_id: String,
    project_name: String,
    availability_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    gateway_node_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    db_node_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_gateway_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_gateway_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_hyperscale_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_hyperscale_count: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ClusterListOutput {
    workspace: Option<CliWorkspace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<CliProjectDetails>,
    local_instances: Vec<LocalInstanceSummary>,
    standard_clusters: Vec<StandardClusterSummary>,
    enterprise_clusters: Vec<EnterpriseClusterSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

pub async fn run(action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Workspace { action } => run_workspace(action).await,
        ConfigAction::Project { action } => run_project(action).await,
        ConfigAction::Cluster { action } => run_cluster(action).await,
    }
}

async fn run_workspace(action: WorkspaceConfigAction) -> Result<()> {
    match action {
        WorkspaceConfigAction::List { members, format } => workspace_list(members, format).await,
        WorkspaceConfigAction::Show { format } => workspace_show(format).await,
        WorkspaceConfigAction::Switch { workspace, id } => workspace_switch(workspace, id).await,
    }
}

async fn run_project(action: ProjectConfigAction) -> Result<()> {
    match action {
        ProjectConfigAction::List {
            workspace,
            id,
            format,
        } => project_list(workspace, id, format).await,
        ProjectConfigAction::Show { format } => project_show(format).await,
        ProjectConfigAction::Switch { project, id } => project_switch(project, id).await,
    }
}

async fn run_cluster(action: ClusterConfigAction) -> Result<()> {
    match action {
        ClusterConfigAction::List {
            workspace,
            workspace_id,
            project,
            project_id,
            format,
        } => cluster_list(workspace, workspace_id, project, project_id, format).await,
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn load_project_context() -> Result<ProjectContext> {
    ProjectContext::find_and_load(None)
        .map_err(|_| eyre!("No helix.toml found. Run 'helix init' to create a project first."))
}

fn load_project_context_optional() -> Option<ProjectContext> {
    ProjectContext::find_and_load(None).ok()
}

fn selected_project_id_from_config(project: Option<&ProjectContext>) -> Option<String> {
    project.and_then(|project| project.config.project.id.clone())
}

async fn fetch_selected_workspace(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> Result<Option<CliWorkspace>> {
    let mut workspace_config = WorkspaceConfig::load()?;
    let Some(selected_workspace_id) = workspace_config.workspace_id.clone() else {
        return Ok(None);
    };

    let workspaces = fetch_workspaces(client, base_url, api_key).await?;
    if let Some(workspace) = find_workspace_by_id(&workspaces, &selected_workspace_id) {
        return Ok(Some(workspace.clone()));
    }

    workspace_config.workspace_id = None;
    workspace_config.save()?;
    Ok(None)
}

async fn resolve_workspace_selector(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    workspace: Option<&str>,
    use_id: bool,
) -> Result<Option<CliWorkspace>> {
    let workspaces = fetch_workspaces(client, base_url, api_key).await?;

    if let Some(selector) = workspace {
        let selected = if use_id {
            find_workspace_by_id(&workspaces, selector)
        } else {
            find_workspace_by_slug(&workspaces, selector)
        };

        return selected
            .cloned()
            .map(Some)
            .ok_or_else(|| eyre!("Workspace '{}' was not found.", selector));
    }

    let mut workspace_config = WorkspaceConfig::load()?;
    if let Some(selected_workspace_id) = workspace_config.workspace_id.clone() {
        if let Some(selected_workspace) = find_workspace_by_id(&workspaces, &selected_workspace_id)
        {
            return Ok(Some(selected_workspace.clone()));
        }

        workspace_config.workspace_id = None;
        workspace_config.save()?;
    }

    Ok(None)
}

async fn resolve_projects_for_workspace(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    workspace: &CliWorkspace,
) -> Result<Vec<CliProject>> {
    fetch_projects(client, base_url, api_key, &workspace.id).await
}

fn save_project_selection(project_root: &Path, config: &HelixConfig) -> Result<()> {
    config.save_to_file(&project_root.join("helix.toml"))?;
    Ok(())
}

fn local_instance_summaries(project: &ProjectContext) -> Vec<LocalInstanceSummary> {
    let mut items: Vec<LocalInstanceSummary> = project
        .config
        .local
        .iter()
        .map(|(name, config)| LocalInstanceSummary {
            name: name.clone(),
            port: config.port,
            build_mode: config.build_mode,
        })
        .collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));
    items
}

fn workspace_standard_summaries(clusters: &CliWorkspaceClusters) -> Vec<StandardClusterSummary> {
    let mut items: Vec<StandardClusterSummary> = clusters
        .standard
        .iter()
        .map(|cluster| StandardClusterSummary {
            cluster_id: cluster.cluster_id.clone(),
            cluster_name: cluster.cluster_name.clone(),
            project_id: cluster.project_id.clone(),
            project_name: cluster.project_name.clone(),
            build_mode: None,
            max_memory_gb: None,
            max_vcpus: None,
        })
        .collect();
    items.sort_by(|a, b| a.cluster_name.cmp(&b.cluster_name));
    items
}

fn workspace_enterprise_summaries(
    clusters: &CliWorkspaceClusters,
) -> Vec<EnterpriseClusterSummary> {
    let mut items: Vec<EnterpriseClusterSummary> = clusters
        .enterprise
        .iter()
        .map(|cluster| EnterpriseClusterSummary {
            cluster_id: cluster.cluster_id.clone(),
            cluster_name: cluster.cluster_name.clone(),
            project_id: cluster.project_id.clone(),
            project_name: cluster.project_name.clone(),
            availability_mode: cluster.availability_mode.clone(),
            gateway_node_type: None,
            db_node_type: None,
            min_gateway_count: None,
            max_gateway_count: None,
            min_hyperscale_count: None,
            max_hyperscale_count: None,
        })
        .collect();
    items.sort_by(|a, b| a.cluster_name.cmp(&b.cluster_name));
    items
}

fn project_standard_summaries(clusters: &CliProjectClusters) -> Vec<StandardClusterSummary> {
    let mut items: Vec<StandardClusterSummary> = clusters
        .standard
        .iter()
        .map(
            |cluster: &CliProjectStandardCluster| StandardClusterSummary {
                cluster_id: cluster.cluster_id.clone(),
                cluster_name: cluster.cluster_name.clone(),
                project_id: clusters.project_id.clone(),
                project_name: clusters.project_name.clone(),
                build_mode: Some(cluster.build_mode.clone()),
                max_memory_gb: Some(cluster.max_memory_gb),
                max_vcpus: Some(cluster.max_vcpus),
            },
        )
        .collect();
    items.sort_by(|a, b| a.cluster_name.cmp(&b.cluster_name));
    items
}

fn project_enterprise_summaries(clusters: &CliProjectClusters) -> Vec<EnterpriseClusterSummary> {
    let mut items: Vec<EnterpriseClusterSummary> = clusters
        .enterprise
        .iter()
        .map(
            |cluster: &CliProjectEnterpriseCluster| EnterpriseClusterSummary {
                cluster_id: cluster.cluster_id.clone(),
                cluster_name: cluster.cluster_name.clone(),
                project_id: clusters.project_id.clone(),
                project_name: clusters.project_name.clone(),
                availability_mode: cluster.availability_mode.clone(),
                gateway_node_type: Some(cluster.gateway_node_type.clone()),
                db_node_type: Some(cluster.db_node_type.clone()),
                min_gateway_count: cluster.min_gateway_count,
                max_gateway_count: cluster.max_gateway_count,
                min_hyperscale_count: cluster.min_hyperscale_count,
                max_hyperscale_count: cluster.max_hyperscale_count,
            },
        )
        .collect();
    items.sort_by(|a, b| a.cluster_name.cmp(&b.cluster_name));
    items
}

fn build_mode_label(build_mode: BuildMode) -> &'static str {
    match build_mode {
        BuildMode::Dev => "dev",
        BuildMode::Release => "release",
        BuildMode::Debug => "debug",
    }
}

fn trim_trailing_zeroes(value: f32) -> String {
    let formatted = format!("{value:.2}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

fn print_context_block(title: &str, fields: &[(&str, String)]) {
    println!("{}", title.bold());
    println!("{}", SECTION_RULE.dimmed());
    for (label, value) in fields {
        let label = format!("{label}:");
        println!(
            "  {:<width$} {}",
            label.dimmed(),
            value,
            width = CONTEXT_LABEL_WIDTH
        );
    }
    println!();
}

fn print_section(title: &str, count: usize) {
    println!("{}", format!("{title} ({count})").bold());
    println!("{}", SECTION_RULE.dimmed());
}

fn print_empty_section(title: &str) {
    print_section(title, 0);
    println!("  {}", "None".dimmed());
    println!();
}

fn print_metadata_line(indent: usize, fields: &[String]) {
    if fields.is_empty() {
        return;
    }

    println!(
        "{:indent$}{}",
        "",
        fields.join("  ").dimmed(),
        indent = indent
    );
}

fn print_list_item(name: &str, current: bool, indent: usize, metadata: &[String]) {
    let prefix = if current {
        format!("{}", "*".green().bold())
    } else {
        format!("{}", "-".dimmed())
    };
    println!("{:indent$}{} {}", "", prefix, name.bold(), indent = indent);
    print_metadata_line(indent + 4, metadata);
}

fn print_workspace_item(item: &WorkspaceListItem) {
    let mut metadata = vec![
        format!("slug={}", item.url_slug),
        format!("type={}", item.workspace_type),
        format!("id={}", item.id),
    ];
    if let Some(members) = &item.members {
        metadata.push(format!("members={}", members.len()));
    }

    print_list_item(&item.name, item.current, 0, &metadata);

    if let Some(members) = &item.members {
        for member in members {
            let display_name = if member.user.github_name.is_empty()
                || member.user.github_name == member.user.github_login
            {
                member.user.github_login.clone()
            } else {
                format!("{} ({})", member.user.github_login, member.user.github_name)
            };

            let metadata = vec![
                format!("role={}", member.role),
                format!("email={}", member.user.github_email),
            ];
            print_list_item(&display_name, false, 4, &metadata);
        }
    }
}

fn print_local_instances(local_instances: &[LocalInstanceSummary]) {
    if local_instances.is_empty() {
        print_empty_section("Local Instances");
        return;
    }

    print_section("Local Instances", local_instances.len());
    for instance in local_instances {
        let mut metadata = vec![format!("build={}", build_mode_label(instance.build_mode))];
        if let Some(port) = instance.port {
            metadata.push(format!("port={port}"));
        }
        print_list_item(&instance.name, false, 0, &metadata);
    }
    println!();
}

fn print_project_heading(project_name: &str) {
    println!("{}", project_name.blue().bold());
}

fn print_standard_clusters(clusters: &[StandardClusterSummary]) {
    if clusters.is_empty() {
        print_empty_section("Helix Cloud Clusters");
        return;
    }

    print_section("Helix Cloud Clusters", clusters.len());
    let mut items: Vec<&StandardClusterSummary> = clusters.iter().collect();
    items.sort_by(|a, b| {
        (&a.project_name, &a.cluster_name, &a.cluster_id).cmp(&(
            &b.project_name,
            &b.cluster_name,
            &b.cluster_id,
        ))
    });

    let mut current_project: Option<&str> = None;
    for cluster in items {
        if current_project != Some(cluster.project_name.as_str()) {
            if current_project.is_some() {
                println!();
            }
            print_project_heading(&cluster.project_name);
            current_project = Some(cluster.project_name.as_str());
        }

        let mut metadata = vec![format!("id={}", cluster.cluster_id)];
        if let Some(build_mode) = &cluster.build_mode {
            metadata.push(format!("build={build_mode}"));
        }
        if let Some(max_memory_gb) = cluster.max_memory_gb {
            metadata.push(format!("memory={}GB", max_memory_gb));
        }
        if let Some(max_vcpus) = cluster.max_vcpus {
            metadata.push(format!("vcpus={}", trim_trailing_zeroes(max_vcpus)));
        }
        print_list_item(&cluster.cluster_name, false, 2, &metadata);
    }
    println!();
}

fn print_enterprise_clusters(clusters: &[EnterpriseClusterSummary]) {
    if clusters.is_empty() {
        print_empty_section("Enterprise Clusters");
        return;
    }

    print_section("Enterprise Clusters", clusters.len());
    let mut items: Vec<&EnterpriseClusterSummary> = clusters.iter().collect();
    items.sort_by(|a, b| {
        (&a.project_name, &a.cluster_name, &a.cluster_id).cmp(&(
            &b.project_name,
            &b.cluster_name,
            &b.cluster_id,
        ))
    });

    let mut current_project: Option<&str> = None;
    for cluster in items {
        if current_project != Some(cluster.project_name.as_str()) {
            if current_project.is_some() {
                println!();
            }
            print_project_heading(&cluster.project_name);
            current_project = Some(cluster.project_name.as_str());
        }

        let mut metadata = vec![
            format!("id={}", cluster.cluster_id),
            format!("mode={}", cluster.availability_mode),
        ];
        if let Some(gateway_node_type) = &cluster.gateway_node_type {
            metadata.push(format!("gateway={gateway_node_type}"));
        }
        if let Some(db_node_type) = &cluster.db_node_type {
            metadata.push(format!("db={db_node_type}"));
        }
        if let (Some(min_gateway_count), Some(max_gateway_count)) =
            (cluster.min_gateway_count, cluster.max_gateway_count)
        {
            if min_gateway_count == max_gateway_count {
                metadata.push(format!("gateway_nodes={min_gateway_count}"));
            } else {
                metadata.push(format!(
                    "gateway_nodes={min_gateway_count}-{max_gateway_count}"
                ));
            }
        }
        if let (Some(min_hyperscale_count), Some(max_hyperscale_count)) =
            (cluster.min_hyperscale_count, cluster.max_hyperscale_count)
        {
            if min_hyperscale_count == max_hyperscale_count {
                metadata.push(format!("db_nodes={min_hyperscale_count}"));
            } else {
                metadata.push(format!(
                    "db_nodes={min_hyperscale_count}-{max_hyperscale_count}"
                ));
            }
        }
        print_list_item(&cluster.cluster_name, false, 2, &metadata);
    }
    println!();
}

async fn workspace_list(include_members: bool, format: ConfigOutputFormat) -> Result<()> {
    let credentials = require_auth().await?;
    let client = reqwest::Client::new();
    let base_url = cloud_base_url();
    let workspaces = fetch_workspaces(&client, &base_url, &credentials.helix_admin_key).await?;
    let current_workspace =
        fetch_selected_workspace(&client, &base_url, &credentials.helix_admin_key).await?;
    let current_workspace_id = current_workspace
        .as_ref()
        .map(|workspace| workspace.id.clone());

    let mut items = Vec::with_capacity(workspaces.len());
    for workspace in workspaces {
        let members = if include_members {
            Some(
                fetch_workspace_members(
                    &client,
                    &base_url,
                    &credentials.helix_admin_key,
                    &workspace.id,
                )
                .await?,
            )
        } else {
            None
        };

        items.push(WorkspaceListItem {
            current: current_workspace_id.as_deref() == Some(workspace.id.as_str()),
            id: workspace.id,
            name: workspace.name,
            url_slug: workspace.url_slug,
            workspace_type: workspace.workspace_type,
            members,
        });
    }

    match format {
        ConfigOutputFormat::Json => print_json(&WorkspaceListOutput {
            current_workspace_id,
            workspaces: items,
        }),
        ConfigOutputFormat::Human if items.is_empty() => {
            println!("No workspaces found.");
            Ok(())
        }
        ConfigOutputFormat::Human => {
            print_section("Workspaces", items.len());
            for item in &items {
                print_workspace_item(item);
            }
            println!();
            Ok(())
        }
    }
}

async fn workspace_show(format: ConfigOutputFormat) -> Result<()> {
    let credentials = require_auth().await?;
    let client = reqwest::Client::new();
    let base_url = cloud_base_url();
    let workspace =
        fetch_selected_workspace(&client, &base_url, &credentials.helix_admin_key).await?;

    let workspace_item = workspace.map(|workspace| WorkspaceListItem {
        id: workspace.id,
        name: workspace.name,
        url_slug: workspace.url_slug,
        workspace_type: workspace.workspace_type,
        current: true,
        members: None,
    });
    let output = WorkspaceShowOutput {
        message: if workspace_item.is_some() {
            None
        } else {
            Some("No workspace selected".to_string())
        },
        workspace: workspace_item,
    };

    match format {
        ConfigOutputFormat::Json => print_json(&output),
        ConfigOutputFormat::Human => {
            match &output.workspace {
                Some(workspace) => {
                    print_context_block(
                        "Current Workspace",
                        &[
                            ("Name", workspace.name.clone()),
                            ("Slug", workspace.url_slug.clone()),
                            ("Type", workspace.workspace_type.clone()),
                            ("ID", workspace.id.clone()),
                        ],
                    );
                }
                None => println!("No workspace selected."),
            }
            Ok(())
        }
    }
}

async fn workspace_switch(workspace: Option<String>, use_id: bool) -> Result<()> {
    let credentials = require_auth().await?;
    let client = reqwest::Client::new();
    let base_url = cloud_base_url();
    let workspaces = fetch_workspaces(&client, &base_url, &credentials.helix_admin_key).await?;

    if workspaces.is_empty() {
        return Err(eyre!(
            "No workspaces found. Go to the dashboard to create a workspace first."
        ));
    }

    let selected = if let Some(selector) = workspace.as_deref() {
        let workspace = if use_id {
            find_workspace_by_id(&workspaces, selector)
        } else {
            find_workspace_by_slug(&workspaces, selector)
        };

        workspace
            .cloned()
            .ok_or_else(|| eyre!("Workspace '{}' was not found.", selector))?
    } else if prompts::is_interactive() {
        let selected_workspace_id =
            prompts::select_workspace(&workspace_prompt_items(&workspaces))?;
        workspaces
            .into_iter()
            .find(|candidate| candidate.id == selected_workspace_id)
            .ok_or_else(|| eyre!("Selected workspace was not found in response"))?
    } else {
        return Err(eyre!(
            "No workspace specified. Pass a workspace slug or run interactively."
        ));
    };

    let mut workspace_config = WorkspaceConfig::load()?;
    workspace_config.workspace_id = Some(selected.id.clone());
    workspace_config.save()?;
    crate::output::success("Updated workspace selection");
    print_context_block(
        "Current Workspace",
        &[
            ("Name", selected.name.clone()),
            ("Slug", selected.url_slug.clone()),
            ("Type", selected.workspace_type.clone()),
            ("ID", selected.id.clone()),
        ],
    );
    Ok(())
}

async fn project_list(
    workspace: Option<String>,
    use_id: bool,
    format: ConfigOutputFormat,
) -> Result<()> {
    let credentials = require_auth().await?;
    let client = reqwest::Client::new();
    let base_url = cloud_base_url();
    let Some(workspace) = resolve_workspace_selector(
        &client,
        &base_url,
        &credentials.helix_admin_key,
        workspace.as_deref(),
        use_id,
    )
    .await?
    else {
        return match format {
            ConfigOutputFormat::Json => print_json(&MessageOutput {
                message: "No workspace selected".to_string(),
            }),
            ConfigOutputFormat::Human => {
                println!("No workspace selected.");
                Ok(())
            }
        };
    };

    let current_project_id =
        selected_project_id_from_config(load_project_context_optional().as_ref());
    let projects = resolve_projects_for_workspace(
        &client,
        &base_url,
        &credentials.helix_admin_key,
        &workspace,
    )
    .await?;
    let items: Vec<ProjectListItem> = projects
        .into_iter()
        .map(|project| ProjectListItem {
            current: current_project_id.as_deref() == Some(project.id.as_str()),
            id: project.id,
            name: project.name,
        })
        .collect();

    match format {
        ConfigOutputFormat::Json => print_json(&ProjectListOutput {
            workspace,
            current_project_id,
            projects: items,
        }),
        ConfigOutputFormat::Human => {
            print_context_block(
                "Workspace",
                &[
                    ("Name", workspace.name.clone()),
                    ("Slug", workspace.url_slug.clone()),
                    ("ID", workspace.id.clone()),
                ],
            );
            if items.is_empty() {
                print_empty_section("Projects");
                return Ok(());
            }

            print_section("Projects", items.len());
            for item in &items {
                let metadata = [format!("id={}", item.id)];
                print_list_item(&item.name, item.current, 0, &metadata);
            }
            println!();
            Ok(())
        }
    }
}

async fn project_show(format: ConfigOutputFormat) -> Result<()> {
    let project = load_project_context()?;
    let Some(project_id) = project.config.project.id.as_deref() else {
        return match format {
            ConfigOutputFormat::Json => print_json(&ProjectShowOutput {
                project: None,
                message: Some("No project selected in helix.toml".to_string()),
            }),
            ConfigOutputFormat::Human => {
                println!("No project selected in helix.toml.");
                Ok(())
            }
        };
    };

    let credentials = require_auth().await?;
    let client = reqwest::Client::new();
    let base_url = cloud_base_url();
    let project_details =
        fetch_project_details(&client, &base_url, &credentials.helix_admin_key, project_id).await?;

    match format {
        ConfigOutputFormat::Json => print_json(&ProjectShowOutput {
            project: Some(project_details),
            message: None,
        }),
        ConfigOutputFormat::Human => {
            print_context_block(
                "Current Project",
                &[
                    ("Name", project_details.name.clone()),
                    ("ID", project_details.id.clone()),
                    (
                        "Workspace",
                        format!(
                            "{} ({})",
                            project_details.workspace_name, project_details.workspace_slug
                        ),
                    ),
                    ("Workspace ID", project_details.workspace_id.clone()),
                ],
            );
            Ok(())
        }
    }
}

async fn project_switch(project: Option<String>, use_id: bool) -> Result<()> {
    let project_context = load_project_context()?;
    let credentials = require_auth().await?;
    let client = reqwest::Client::new();
    let base_url = cloud_base_url();

    let selected_project = if use_id {
        if let Some(project_id) = project.as_deref() {
            let project =
                fetch_project_details(&client, &base_url, &credentials.helix_admin_key, project_id)
                    .await?;
            CliProject {
                id: project.id,
                name: project.name,
            }
        } else {
            let Some(workspace) =
                fetch_selected_workspace(&client, &base_url, &credentials.helix_admin_key).await?
            else {
                return Err(eyre!("No workspace selected."));
            };

            if !prompts::is_interactive() {
                return Err(eyre!(
                    "No project specified. Pass a project ID or run interactively."
                ));
            }

            let projects = resolve_projects_for_workspace(
                &client,
                &base_url,
                &credentials.helix_admin_key,
                &workspace,
            )
            .await?;
            let selected_project_id = prompts::select_project(
                &projects
                    .iter()
                    .map(|project| (project.id.clone(), project.name.clone()))
                    .collect::<Vec<_>>(),
            )?;
            projects
                .into_iter()
                .find(|candidate| candidate.id == selected_project_id)
                .ok_or_else(|| eyre!("Selected project was not found in response"))?
        }
    } else {
        let Some(workspace) =
            fetch_selected_workspace(&client, &base_url, &credentials.helix_admin_key).await?
        else {
            return Err(eyre!("No workspace selected."));
        };

        let projects = resolve_projects_for_workspace(
            &client,
            &base_url,
            &credentials.helix_admin_key,
            &workspace,
        )
        .await?;
        if projects.is_empty() {
            return Err(eyre!(
                "No projects exist in workspace '{}'.",
                workspace.url_slug
            ));
        }

        if let Some(project_name) = project.as_deref() {
            find_project_by_name(&projects, project_name)
                .cloned()
                .ok_or_else(|| {
                    eyre!(
                        "Project '{}' was not found in workspace '{}'.",
                        project_name,
                        workspace.url_slug
                    )
                })?
        } else if prompts::is_interactive() {
            let selected_project_id = prompts::select_project(
                &projects
                    .iter()
                    .map(|project| (project.id.clone(), project.name.clone()))
                    .collect::<Vec<_>>(),
            )?;
            projects
                .into_iter()
                .find(|candidate| candidate.id == selected_project_id)
                .ok_or_else(|| eyre!("Selected project was not found in response"))?
        } else {
            return Err(eyre!(
                "No project specified. Pass a project name or run interactively."
            ));
        }
    };

    let mut config = project_context.config.clone();
    config.project.id = Some(selected_project.id.clone());
    config.project.name = selected_project.name.clone();
    save_project_selection(&project_context.root, &config)?;

    crate::output::success("Updated project selection");
    print_context_block(
        "Current Project",
        &[
            ("Name", selected_project.name.clone()),
            ("ID", selected_project.id.clone()),
            (
                "Config",
                project_context
                    .root
                    .join("helix.toml")
                    .display()
                    .to_string(),
            ),
        ],
    );
    Ok(())
}

async fn cluster_list(
    workspace: Option<String>,
    workspace_id: Option<String>,
    project: Option<String>,
    project_id: Option<String>,
    format: ConfigOutputFormat,
) -> Result<()> {
    let credentials = require_auth().await?;
    let client = reqwest::Client::new();
    let base_url = cloud_base_url();
    let workspace_selector = workspace.or(workspace_id.clone());
    let use_workspace_id = workspace_id.is_some();
    let selected_workspace = resolve_workspace_selector(
        &client,
        &base_url,
        &credentials.helix_admin_key,
        workspace_selector.as_deref(),
        use_workspace_id,
    )
    .await?;

    let Some(workspace) = selected_workspace else {
        return match format {
            ConfigOutputFormat::Json => print_json(&ClusterListOutput {
                workspace: None,
                project: None,
                local_instances: Vec::new(),
                standard_clusters: Vec::new(),
                enterprise_clusters: Vec::new(),
                message: Some("No workspace selected".to_string()),
            }),
            ConfigOutputFormat::Human => {
                println!("No workspace selected.");
                Ok(())
            }
        };
    };

    let local_instances = load_project_context_optional()
        .as_ref()
        .map(local_instance_summaries)
        .unwrap_or_default();

    let (selected_project, standard_clusters, enterprise_clusters) =
        if let Some(project_id) = project_id {
            let project_details = fetch_project_details(
                &client,
                &base_url,
                &credentials.helix_admin_key,
                &project_id,
            )
            .await?;
            if project_details.workspace_id != workspace.id {
                return Err(eyre!(
                    "Project '{}' does not belong to workspace '{}'.",
                    project_details.id,
                    workspace.url_slug
                ));
            }
            let clusters = fetch_project_clusters(
                &client,
                &base_url,
                &credentials.helix_admin_key,
                &project_details.id,
            )
            .await?;
            (
                Some(project_details),
                project_standard_summaries(&clusters),
                project_enterprise_summaries(&clusters),
            )
        } else if let Some(project_name) = project {
            let projects = resolve_projects_for_workspace(
                &client,
                &base_url,
                &credentials.helix_admin_key,
                &workspace,
            )
            .await?;
            let selected_project = find_project_by_name(&projects, &project_name)
                .cloned()
                .ok_or_else(|| {
                    eyre!(
                        "Project '{}' was not found in workspace '{}'.",
                        project_name,
                        workspace.url_slug
                    )
                })?;
            let project_details = fetch_project_details(
                &client,
                &base_url,
                &credentials.helix_admin_key,
                &selected_project.id,
            )
            .await?;
            let clusters = fetch_project_clusters(
                &client,
                &base_url,
                &credentials.helix_admin_key,
                &selected_project.id,
            )
            .await?;
            (
                Some(project_details),
                project_standard_summaries(&clusters),
                project_enterprise_summaries(&clusters),
            )
        } else {
            let clusters = fetch_workspace_clusters(
                &client,
                &base_url,
                &credentials.helix_admin_key,
                &workspace.id,
            )
            .await?;
            (
                None,
                workspace_standard_summaries(&clusters),
                workspace_enterprise_summaries(&clusters),
            )
        };

    let output = ClusterListOutput {
        workspace: Some(workspace.clone()),
        project: selected_project,
        local_instances,
        standard_clusters,
        enterprise_clusters,
        message: None,
    };

    match format {
        ConfigOutputFormat::Json => print_json(&output),
        ConfigOutputFormat::Human => {
            let mut fields = vec![
                ("Name", workspace.name.clone()),
                ("Slug", workspace.url_slug.clone()),
                ("ID", workspace.id.clone()),
            ];
            if let Some(project) = &output.project {
                fields.push(("Project", format!("{} ({})", project.name, project.id)));
            }
            print_context_block("Selection", &fields);
            print_local_instances(&output.local_instances);
            print_standard_clusters(&output.standard_clusters);
            print_enterprise_clusters(&output.enterprise_clusters);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CloudConfig, CloudInstanceConfig, LocalInstanceConfig};
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn local_instance_summaries_ignore_remote_instances() {
        let temp_dir = TempDir::new().unwrap();
        let mut config = HelixConfig::default_config("demo");
        config.cloud.insert(
            "remote".to_string(),
            CloudConfig::Helix(CloudInstanceConfig {
                cluster_id: "cl_123".to_string(),
                region: None,
                build_mode: BuildMode::Release,
                env_vars: HashMap::new(),
                db_config: Default::default(),
            }),
        );
        config.local.insert(
            "dev-2".to_string(),
            LocalInstanceConfig {
                port: Some(7979),
                build_mode: BuildMode::Release,
                db_config: Default::default(),
            },
        );
        let config_path = temp_dir.path().join("helix.toml");
        config.save_to_file(&config_path).unwrap();

        let project = ProjectContext::find_and_load(Some(temp_dir.path())).unwrap();
        let summaries = local_instance_summaries(&project);

        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].name, "dev");
        assert_eq!(summaries[1].name, "dev-2");
    }

    #[test]
    fn save_project_selection_updates_project_id_and_name() {
        let temp_dir = TempDir::new().unwrap();
        let config = HelixConfig::default_config("demo");
        let config_path = temp_dir.path().join("helix.toml");
        config.save_to_file(&config_path).unwrap();

        let project = ProjectContext::find_and_load(Some(temp_dir.path())).unwrap();
        let mut updated = project.config.clone();
        updated.project.id = Some("proj_123".to_string());
        updated.project.name = "cloud-demo".to_string();
        save_project_selection(&project.root, &updated).unwrap();

        let reloaded = HelixConfig::from_file(&config_path).unwrap();
        assert_eq!(reloaded.project.id.as_deref(), Some("proj_123"));
        assert_eq!(reloaded.project.name, "cloud-demo");
    }
}
