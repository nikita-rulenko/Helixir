use crate::commands::auth::Credentials;
use crate::commands::cloud_api::{
    CliBillingResponse, CliProject, fetch_project_details, fetch_workspace_billing,
    fetch_workspaces, find_workspace_by_id, resolve_current_workspace, resolve_or_create_project,
};
use crate::commands::integrations::helix::cloud_base_url;
use crate::config::{AvailabilityMode, BuildMode, WorkspaceConfig};
use crate::prompts;
use eyre::{Result, eyre};
use serde::{Deserialize, Serialize};

// ============================================================================
// Result types
// ============================================================================

pub struct StandardClusterResult {
    pub cluster_id: String,
    pub instance_name: String,
    pub build_mode: BuildMode,
}

pub struct EnterpriseClusterResult {
    pub cluster_id: String,
    pub instance_name: String,
    pub availability_mode: AvailabilityMode,
    pub gateway_node_type: String,
    pub db_node_type: String,
    pub min_instances: u64,
    pub max_instances: u64,
}

pub enum ClusterResult {
    Standard(StandardClusterResult),
    Enterprise(EnterpriseClusterResult),
}

pub struct WorkspaceProjectClusterFlowResult {
    pub cluster: ClusterResult,
    pub resolved_project_name: String,
    pub resolved_project_id: String,
}

// ============================================================================
// API response types
// ============================================================================

struct SelectedWorkspace {
    id: String,
    workspace_type: String,
}

#[derive(Deserialize)]
struct CreateClusterResponse {
    cluster_id: String,
}

#[derive(Serialize)]
struct CreateEnterpriseClusterRequest<'a> {
    cluster_name: &'a str,
    availability_mode: &'a str,
    gateway_node_type: &'a str,
    db_node_type: &'a str,
    min_gateway_count: u64,
    max_gateway_count: u64,
    min_hyperscale_count: u64,
    max_hyperscale_count: u64,
}

fn build_enterprise_cluster_request<'a>(
    cluster_name: &'a str,
    availability_mode: &'a str,
    gateway_node_type: &'a str,
    db_node_type: &'a str,
    min_instances: u64,
    max_instances: u64,
) -> CreateEnterpriseClusterRequest<'a> {
    CreateEnterpriseClusterRequest {
        cluster_name,
        availability_mode,
        gateway_node_type,
        db_node_type,
        min_gateway_count: min_instances,
        max_gateway_count: min_instances,
        min_hyperscale_count: max_instances,
        max_hyperscale_count: max_instances,
    }
}

// ============================================================================
// Main flow
// ============================================================================

/// Run the workspace → project → cluster selection/creation flow.
/// Returns a ClusterResult describing the created cluster.
pub async fn run_workspace_project_cluster_flow(
    project_name: &str,
    project_id_hint: Option<&str>,
    credentials: &Credentials,
    preferred_cluster_name: Option<&str>,
) -> Result<WorkspaceProjectClusterFlowResult> {
    let client = reqwest::Client::new();
    let base_url = cloud_base_url();

    // Step 1: Workspace selection
    let workspace =
        select_or_load_workspace(&client, &base_url, credentials, project_id_hint).await?;

    // Step 2: Billing check
    check_billing(&client, &base_url, credentials, &workspace.id).await?;

    // Step 3: Project matching
    let resolved_project = match_or_create_project(
        &client,
        &base_url,
        credentials,
        &workspace.id,
        project_name,
        project_id_hint,
    )
    .await?;

    // Step 4: Cluster type selection
    let cluster_type = if workspace.workspace_type == "enterprise" {
        prompts::select_cluster_type()?
    } else {
        crate::output::info("Selected workspace is not enterprise; creating a standard cluster.");
        "standard"
    };

    // Step 5/6: Configure and create cluster
    match cluster_type {
        "enterprise" => Ok(WorkspaceProjectClusterFlowResult {
            cluster: create_enterprise_cluster_flow(
                &client,
                &base_url,
                credentials,
                &resolved_project.id,
                preferred_cluster_name,
            )
            .await?,
            resolved_project_name: resolved_project.name,
            resolved_project_id: resolved_project.id,
        }),
        _ => Ok(WorkspaceProjectClusterFlowResult {
            cluster: create_standard_cluster_flow(
                &client,
                &base_url,
                credentials,
                &resolved_project.id,
                preferred_cluster_name,
            )
            .await?,
            resolved_project_name: resolved_project.name,
            resolved_project_id: resolved_project.id,
        }),
    }
}

async fn select_or_load_workspace(
    client: &reqwest::Client,
    base_url: &str,
    credentials: &Credentials,
    project_id_hint: Option<&str>,
) -> Result<SelectedWorkspace> {
    let mut workspace_config = WorkspaceConfig::load()?;
    let workspaces = fetch_workspaces(client, base_url, &credentials.helix_admin_key).await?;

    if workspaces.is_empty() {
        return Err(eyre!(
            "No workspaces found. Go to the dashboard to create a workspace first."
        ));
    }

    if let Some(project_id) = project_id_hint {
        let project = match fetch_project_details(
            client,
            base_url,
            &credentials.helix_admin_key,
            project_id,
        )
        .await
        {
            Ok(project) => Some(project),
            Err(error) => {
                crate::output::warning(&format!(
                    "Could not resolve workspace from project.id '{}': {}. Falling back to the selected workspace.",
                    project_id, error
                ));
                None
            }
        };

        if let Some(project) = project
            && let Some(workspace) = find_workspace_by_id(&workspaces, &project.workspace_id)
        {
            return Ok(SelectedWorkspace {
                id: workspace.id.clone(),
                workspace_type: workspace.workspace_type.clone(),
            });
        }
    }

    let workspace = resolve_current_workspace(
        client,
        base_url,
        &credentials.helix_admin_key,
        &mut workspace_config,
    )
    .await?;

    Ok(SelectedWorkspace {
        id: workspace.id,
        workspace_type: workspace.workspace_type,
    })
}

async fn check_billing(
    client: &reqwest::Client,
    base_url: &str,
    credentials: &Credentials,
    workspace_id: &str,
) -> Result<CliBillingResponse> {
    let billing =
        fetch_workspace_billing(client, base_url, &credentials.helix_admin_key, workspace_id)
            .await?;

    if !billing.has_billing {
        return Err(eyre!(
            "No active billing found for this workspace. Go to the dashboard to set up billing first."
        ));
    }

    Ok(billing)
}

async fn match_or_create_project(
    client: &reqwest::Client,
    base_url: &str,
    credentials: &Credentials,
    workspace_id: &str,
    project_name: &str,
    project_id_hint: Option<&str>,
) -> Result<CliProject> {
    let project = resolve_or_create_project(
        client,
        base_url,
        &credentials.helix_admin_key,
        workspace_id,
        project_name,
        project_id_hint,
    )
    .await?;

    Ok(project)
}

async fn create_standard_cluster_flow(
    client: &reqwest::Client,
    base_url: &str,
    credentials: &Credentials,
    project_id: &str,
    preferred_cluster_name: Option<&str>,
) -> Result<ClusterResult> {
    let cluster_name = if let Some(name) = preferred_cluster_name {
        name.to_string()
    } else if prompts::is_interactive() {
        prompts::input_cluster_name("prod")?
    } else {
        "prod".to_string()
    };

    let build_mode = if prompts::is_interactive() {
        prompts::select_build_mode()?
    } else {
        BuildMode::Release
    };

    let build_mode_str = match build_mode {
        BuildMode::Dev => "dev",
        BuildMode::Release => "release",
        BuildMode::Debug => "dev",
    };

    let resp: CreateClusterResponse = client
        .post(format!(
            "{}/api/cli/projects/{}/clusters",
            base_url, project_id
        ))
        .header("x-api-key", &credentials.helix_admin_key)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "cluster_name": cluster_name,
            "build_mode": build_mode_str,
        }))
        .send()
        .await
        .map_err(|e| eyre!("Failed to create cluster: {}", e))?
        .error_for_status()
        .map_err(|e| eyre!("Failed to create cluster: {}", e))?
        .json()
        .await
        .map_err(|e| eyre!("Failed to parse create cluster response: {}", e))?;

    crate::output::success(&format!(
        "Cluster '{}' created (ID: {})",
        cluster_name, resp.cluster_id
    ));

    Ok(ClusterResult::Standard(StandardClusterResult {
        cluster_id: resp.cluster_id,
        instance_name: cluster_name,
        build_mode,
    }))
}

async fn create_enterprise_cluster_flow(
    client: &reqwest::Client,
    base_url: &str,
    credentials: &Credentials,
    project_id: &str,
    preferred_cluster_name: Option<&str>,
) -> Result<ClusterResult> {
    let cluster_name = if let Some(name) = preferred_cluster_name {
        name.to_string()
    } else if prompts::is_interactive() {
        prompts::input_cluster_name("prod")?
    } else {
        "prod".to_string()
    };

    let availability_mode = AvailabilityMode::Ha;
    if prompts::is_interactive() {
        crate::output::info("Enterprise dev mode has been removed; creating an HA cluster.");
    }
    let is_ha = availability_mode == AvailabilityMode::Ha;

    let gateway_node_type = if prompts::is_interactive() {
        prompts::select_gateway_node_type(is_ha)?
    } else if is_ha {
        "GW-40".to_string()
    } else {
        "GW-20".to_string()
    };

    let db_node_type = if prompts::is_interactive() {
        prompts::select_db_node_type(is_ha)?
    } else if is_ha {
        "HLX-160".to_string()
    } else {
        "HLX-40".to_string()
    };

    let (min_instances, max_instances) = if is_ha && prompts::is_interactive() {
        let min = prompts::input_min_instances()?;
        let max = prompts::input_max_instances(min)?;
        (min, max)
    } else if is_ha {
        (3, 3)
    } else {
        (1, 1)
    };

    // Show summary
    println!();
    crate::output::info(&format!("Cluster: {}", cluster_name));
    crate::output::info(&format!("Mode: {}", availability_mode));
    crate::output::info(&format!("Gateway: {}", gateway_node_type));
    crate::output::info(&format!("DB: {}", db_node_type));
    if is_ha {
        crate::output::info(&format!("Instances: {} - {}", min_instances, max_instances));
    }
    println!();

    if prompts::is_interactive() && !prompts::confirm("Create this enterprise cluster?")? {
        return Err(eyre!("Cluster creation cancelled"));
    }

    let availability_mode_str = match availability_mode {
        AvailabilityMode::Dev => "dev",
        AvailabilityMode::Ha => "ha",
    };
    let request_body = build_enterprise_cluster_request(
        &cluster_name,
        availability_mode_str,
        &gateway_node_type,
        &db_node_type,
        min_instances,
        max_instances,
    );

    let resp: CreateClusterResponse = client
        .post(format!(
            "{}/api/cli/projects/{}/enterprise-clusters",
            base_url, project_id
        ))
        .header("x-api-key", &credentials.helix_admin_key)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| eyre!("Failed to create enterprise cluster: {}", e))?
        .error_for_status()
        .map_err(|e| eyre!("Failed to create enterprise cluster: {}", e))?
        .json()
        .await
        .map_err(|e| eyre!("Failed to parse response: {}", e))?;

    crate::output::success(&format!(
        "Enterprise cluster '{}' created (ID: {})",
        cluster_name, resp.cluster_id
    ));

    Ok(ClusterResult::Enterprise(EnterpriseClusterResult {
        cluster_id: resp.cluster_id,
        instance_name: cluster_name,
        availability_mode,
        gateway_node_type,
        db_node_type,
        min_instances,
        max_instances,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn enterprise_cluster_create_request_uses_role_based_count_fields() {
        let request = build_enterprise_cluster_request("prod", "ha", "GW-40", "HLX-160", 3, 5);
        let request_json = serde_json::to_value(&request).unwrap();

        assert_eq!(
            request_json,
            json!({
                "cluster_name": "prod",
                "availability_mode": "ha",
                "gateway_node_type": "GW-40",
                "db_node_type": "HLX-160",
                "min_gateway_count": 3,
                "max_gateway_count": 3,
                "min_hyperscale_count": 5,
                "max_hyperscale_count": 5
            })
        );
    }
}
