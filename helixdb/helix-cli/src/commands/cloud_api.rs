use crate::config::WorkspaceConfig;
use crate::prompts;
use eyre::{Result, eyre};
use reqwest::Client;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliWorkspace {
    pub id: String,
    pub name: String,
    pub url_slug: String,
    #[serde(default = "default_workspace_type")]
    pub workspace_type: String,
}

fn default_workspace_type() -> String {
    "organization".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliUser {
    pub id: String,
    pub github_id: u64,
    pub github_login: String,
    pub github_name: String,
    pub github_email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliWorkspaceMember {
    pub user: CliUser,
    pub role: String,
    pub role_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliCluster {
    pub cluster_id: String,
    pub cluster_name: String,
    pub project_id: String,
    pub project_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliEnterpriseCluster {
    pub cluster_id: String,
    pub cluster_name: String,
    pub project_id: String,
    pub project_name: String,
    pub availability_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliWorkspaceClusters {
    pub standard: Vec<CliCluster>,
    pub enterprise: Vec<CliEnterpriseCluster>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliProject {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliProjectDetails {
    pub id: String,
    pub name: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub workspace_slug: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliProjectClusters {
    pub project_id: String,
    pub project_name: String,
    pub standard: Vec<CliProjectStandardCluster>,
    pub enterprise: Vec<CliProjectEnterpriseCluster>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliProjectStandardCluster {
    pub cluster_id: String,
    pub cluster_name: String,
    pub build_mode: String,
    pub max_memory_gb: u32,
    pub max_vcpus: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliProjectEnterpriseCluster {
    pub cluster_id: String,
    pub cluster_name: String,
    pub availability_mode: String,
    pub gateway_node_type: String,
    pub db_node_type: String,
    #[serde(default)]
    pub min_gateway_count: Option<u64>,
    #[serde(default)]
    pub max_gateway_count: Option<u64>,
    #[serde(default)]
    pub min_hyperscale_count: Option<u64>,
    #[serde(default)]
    pub max_hyperscale_count: Option<u64>,
    #[serde(default)]
    pub gateway_count: Option<u64>,
    #[serde(default)]
    pub hyperscale_count: Option<u64>,
    #[serde(default)]
    pub min_instances: Option<u64>,
    #[serde(default)]
    pub max_instances: Option<u64>,
}

impl CliProjectEnterpriseCluster {
    pub fn resolved_gateway_min_count(&self) -> Option<u64> {
        self.min_gateway_count
            .or(self.gateway_count)
            .or(self.max_gateway_count)
            .or(self.min_instances)
    }

    pub fn resolved_gateway_max_count(&self) -> Option<u64> {
        self.max_gateway_count
            .or(self.gateway_count)
            .or(self.min_gateway_count)
            .or(self.min_instances)
    }

    pub fn resolved_hyperscale_min_count(&self) -> Option<u64> {
        self.min_hyperscale_count
            .or(self.hyperscale_count)
            .or(self.max_hyperscale_count)
            .or(self.max_instances)
    }

    pub fn resolved_hyperscale_max_count(&self) -> Option<u64> {
        self.max_hyperscale_count
            .or(self.hyperscale_count)
            .or(self.min_hyperscale_count)
            .or(self.max_instances)
    }

    pub fn resolved_gateway_count(&self) -> Option<u64> {
        self.resolved_gateway_min_count()
    }

    pub fn resolved_hyperscale_count(&self) -> Option<u64> {
        self.resolved_hyperscale_min_count()
    }

    pub fn compatibility_min_instances(&self) -> Option<u64> {
        if let (Some(gateway_count), Some(hyperscale_count)) = (
            self.resolved_gateway_min_count(),
            self.resolved_hyperscale_min_count(),
        ) {
            Some(gateway_count.min(hyperscale_count))
        } else {
            self.min_instances
        }
    }

    pub fn compatibility_max_instances(&self) -> Option<u64> {
        if let (Some(gateway_count), Some(hyperscale_count)) = (
            self.resolved_gateway_max_count(),
            self.resolved_hyperscale_max_count(),
        ) {
            Some(gateway_count.max(hyperscale_count))
        } else {
            self.max_instances
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliClusterProject {
    pub cluster_id: String,
    pub project_id: String,
    pub project_name: String,
    pub workspace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliBillingResponse {
    pub has_billing: bool,
    pub workspace_type: String,
    pub plan: String,
}

#[derive(Debug, Deserialize)]
struct ApiErrorResponse {
    error: String,
}

#[derive(Debug, Deserialize)]
struct CreateProjectResponse {
    id: String,
    name: String,
}

async fn send_json<T>(request: reqwest::RequestBuilder, action: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    let response = request
        .send()
        .await
        .map_err(|e| eyre!("Failed to {action}: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        let error_message = serde_json::from_str::<ApiErrorResponse>(&error_body)
            .map(|error| error.error)
            .unwrap_or_else(|_| {
                if error_body.is_empty() {
                    format!("request failed with status {status}")
                } else {
                    error_body
                }
            });

        return Err(eyre!("Failed to {action}: {error_message}"));
    }

    response
        .json()
        .await
        .map_err(|e| eyre!("Failed to parse {action} response: {e}"))
}

pub fn workspace_prompt_items(workspaces: &[CliWorkspace]) -> Vec<(String, String, String)> {
    workspaces
        .iter()
        .map(|workspace| {
            (
                workspace.id.clone(),
                workspace.name.clone(),
                workspace.url_slug.clone(),
            )
        })
        .collect()
}

pub fn project_prompt_items(projects: &[CliProject]) -> Vec<(String, String)> {
    projects
        .iter()
        .map(|project| (project.id.clone(), project.name.clone()))
        .collect()
}

pub fn find_workspace_by_id<'a>(
    workspaces: &'a [CliWorkspace],
    workspace_id: &str,
) -> Option<&'a CliWorkspace> {
    workspaces
        .iter()
        .find(|workspace| workspace.id == workspace_id)
}

pub fn find_workspace_by_slug<'a>(
    workspaces: &'a [CliWorkspace],
    workspace_slug: &str,
) -> Option<&'a CliWorkspace> {
    workspaces
        .iter()
        .find(|workspace| workspace.url_slug == workspace_slug)
}

pub fn find_project_by_id<'a>(
    projects: &'a [CliProject],
    project_id: &str,
) -> Option<&'a CliProject> {
    projects.iter().find(|project| project.id == project_id)
}

pub fn find_project_by_name<'a>(
    projects: &'a [CliProject],
    project_name: &str,
) -> Option<&'a CliProject> {
    projects.iter().find(|project| project.name == project_name)
}

pub async fn fetch_workspaces(
    client: &Client,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<CliWorkspace>> {
    send_json(
        client
            .get(format!("{base_url}/api/cli/workspaces"))
            .header("x-api-key", api_key),
        "fetch workspaces",
    )
    .await
}

pub async fn fetch_workspace_members(
    client: &Client,
    base_url: &str,
    api_key: &str,
    workspace_id: &str,
) -> Result<Vec<CliWorkspaceMember>> {
    send_json(
        client
            .get(format!(
                "{base_url}/api/cli/workspaces/{workspace_id}/members"
            ))
            .header("x-api-key", api_key),
        "fetch workspace members",
    )
    .await
}

pub async fn fetch_workspace_clusters(
    client: &Client,
    base_url: &str,
    api_key: &str,
    workspace_id: &str,
) -> Result<CliWorkspaceClusters> {
    send_json(
        client
            .get(format!(
                "{base_url}/api/cli/workspaces/{workspace_id}/clusters"
            ))
            .header("x-api-key", api_key),
        "fetch workspace clusters",
    )
    .await
}

pub async fn fetch_workspace_billing(
    client: &Client,
    base_url: &str,
    api_key: &str,
    workspace_id: &str,
) -> Result<CliBillingResponse> {
    send_json(
        client
            .get(format!(
                "{base_url}/api/cli/workspaces/{workspace_id}/billing"
            ))
            .header("x-api-key", api_key),
        "check billing",
    )
    .await
}

pub async fn fetch_projects(
    client: &Client,
    base_url: &str,
    api_key: &str,
    workspace_id: &str,
) -> Result<Vec<CliProject>> {
    send_json(
        client
            .get(format!(
                "{base_url}/api/cli/workspaces/{workspace_id}/projects"
            ))
            .header("x-api-key", api_key),
        "fetch projects",
    )
    .await
}

pub async fn fetch_project_details(
    client: &Client,
    base_url: &str,
    api_key: &str,
    project_id: &str,
) -> Result<CliProjectDetails> {
    send_json(
        client
            .get(format!("{base_url}/api/cli/projects/{project_id}"))
            .header("x-api-key", api_key),
        "fetch project details",
    )
    .await
}

pub async fn fetch_project_clusters(
    client: &Client,
    base_url: &str,
    api_key: &str,
    project_id: &str,
) -> Result<CliProjectClusters> {
    send_json(
        client
            .get(format!("{base_url}/api/cli/projects/{project_id}/clusters"))
            .header("x-api-key", api_key),
        "fetch project clusters",
    )
    .await
}

pub async fn fetch_cluster_project(
    client: &Client,
    base_url: &str,
    api_key: &str,
    cluster_id: &str,
) -> Result<CliClusterProject> {
    send_json(
        client
            .get(format!("{base_url}/api/cli/clusters/{cluster_id}/project"))
            .header("x-api-key", api_key),
        "fetch cluster project",
    )
    .await
}

pub async fn fetch_enterprise_cluster_project(
    client: &Client,
    base_url: &str,
    api_key: &str,
    cluster_id: &str,
) -> Result<CliClusterProject> {
    send_json(
        client
            .get(format!(
                "{base_url}/api/cli/enterprise-clusters/{cluster_id}/project"
            ))
            .header("x-api-key", api_key),
        "fetch enterprise cluster project",
    )
    .await
}

pub async fn create_project(
    client: &Client,
    base_url: &str,
    api_key: &str,
    workspace_id: &str,
    name: &str,
) -> Result<CliProject> {
    let response: CreateProjectResponse = send_json(
        client
            .post(format!(
                "{base_url}/api/cli/workspaces/{workspace_id}/projects"
            ))
            .header("x-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "name": name })),
        "create project",
    )
    .await?;

    Ok(CliProject {
        id: response.id,
        name: response.name,
    })
}

pub async fn resolve_current_workspace(
    client: &Client,
    base_url: &str,
    api_key: &str,
    workspace_config: &mut WorkspaceConfig,
) -> Result<CliWorkspace> {
    let workspaces = fetch_workspaces(client, base_url, api_key).await?;

    if workspaces.is_empty() {
        return Err(eyre!(
            "No workspaces found. Go to the dashboard to create a workspace first."
        ));
    }

    if let Some(cached_workspace_id) = workspace_config.workspace_id.as_deref() {
        if let Some(workspace) = find_workspace_by_id(&workspaces, cached_workspace_id) {
            return Ok(workspace.clone());
        }

        crate::output::warning(
            "Saved workspace selection is no longer available. Please select a workspace again.",
        );
        workspace_config.workspace_id = None;
        workspace_config.save()?;
    }

    let selected_workspace_id = prompts::select_workspace(&workspace_prompt_items(&workspaces))?;
    workspace_config.workspace_id = Some(selected_workspace_id.clone());
    workspace_config.save()?;

    workspaces
        .into_iter()
        .find(|workspace| workspace.id == selected_workspace_id)
        .ok_or_else(|| eyre!("Selected workspace was not found in response"))
}

pub async fn resolve_or_create_project(
    client: &Client,
    base_url: &str,
    api_key: &str,
    workspace_id: &str,
    project_name: &str,
    project_id_hint: Option<&str>,
) -> Result<CliProject> {
    let projects = fetch_projects(client, base_url, api_key, workspace_id).await?;

    if let Some(expected_project_id) = project_id_hint
        && let Some(existing) = find_project_by_id(&projects, expected_project_id)
    {
        crate::output::info(&format!(
            "Using project '{}' from your selected workspace.",
            existing.name
        ));
        return Ok(existing.clone());
    }

    if let Some(existing) = find_project_by_name(&projects, project_name) {
        crate::output::info(&format!(
            "Using existing project '{}' from your selected workspace.",
            existing.name
        ));
        return Ok(existing.clone());
    }

    match prompts::select_missing_project_choice(project_name)? {
        prompts::MissingProjectChoice::ChooseExisting if projects.is_empty() => Err(eyre!(
            "No projects exist in this workspace yet. Create one to continue."
        )),
        prompts::MissingProjectChoice::ChooseExisting => {
            let selected_project_id = prompts::select_project(&project_prompt_items(&projects))?;
            let selected_project = projects
                .into_iter()
                .find(|project| project.id == selected_project_id)
                .ok_or_else(|| eyre!("Selected project was not found in response"))?;

            crate::output::info(&format!(
                "Using existing project '{}' from your selected workspace.",
                selected_project.name
            ));

            Ok(selected_project)
        }
        prompts::MissingProjectChoice::Create => {
            let chosen_name = prompts::input_project_name(project_name)?;
            let created_project =
                create_project(client, base_url, api_key, workspace_id, &chosen_name).await?;
            crate::output::success(&format!("Project '{}' created", created_project.name));
            Ok(created_project)
        }
    }
}
