//! Stable JSON projections consumed by the browser frontend.

use serde::{Deserialize, Serialize};

use crate::installer::SystemState;

/// Version and lifecycle metadata for the control-plane shell.
#[derive(Debug, Clone, Serialize)]
pub struct ControlPlaneMeta {
    pub product: &'static str,
    pub version: &'static str,
    pub api_version: &'static str,
    pub phase: &'static str,
    pub transport: &'static str,
    pub runtime: &'static str,
    pub host_operations_available: bool,
}

/// Read-only installation discovery projected to the browser.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryResponse {
    pub phase: &'static str,
    pub state: SystemState,
}

/// Bounded, actor-scoped counters for the observatory landing page.
#[derive(Debug, Clone, Serialize)]
pub struct OverviewStats {
    pub actor_id: String,
    pub access_scope: &'static str,
    pub mode: &'static str,
    pub memories: Option<u64>,
    pub graph_nodes: Option<u64>,
    pub principals: usize,
    /// Number of logical agent families represented by presence rows.
    pub agents: usize,
    /// Number of logical principals with at least one active instance.
    pub active_agents: usize,
    /// Total execution-instance rows (sub-agents/processes included).
    pub agent_instances: usize,
    /// Execution instances with a live, non-terminal presence lease.
    pub active_agent_instances: usize,
    /// Child execution instances (`agent_id != principal_id`).
    pub subagents: usize,
    /// Child instances with a live, non-terminal presence lease.
    pub active_subagents: usize,
    pub workspaces: usize,
    pub entities: Option<u64>,
    pub concepts: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentProjection {
    pub agent_id: String,
    pub principal_id: String,
    pub name: String,
    pub role: String,
    pub host: String,
    pub status: String,
    pub last_seen: String,
    pub age_seconds: Option<i64>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentFamilyProjection {
    pub principal_id: String,
    pub instance_count: usize,
    pub active_instances: usize,
    pub hosts: Vec<String>,
    pub instances: Vec<AgentProjection>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrincipalProjection {
    pub subject_id: String,
    pub global_roles: Vec<&'static str>,
    pub groups: Vec<GroupRoleProjection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupRoleProjection {
    pub group_id: String,
    pub roles: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupProjection {
    pub group_id: String,
    pub name: String,
    pub description: String,
    pub dedup_group_id: Option<String>,
    pub member_count: usize,
    pub reserved: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccessProjection {
    pub active_window_secs: u64,
    /// Execution-level roster retained for diagnostics and farewell state.
    pub agents: Vec<AgentProjection>,
    /// Human-facing logical agents, grouped by stable RBAC principal.
    pub agent_families: Vec<AgentFamilyProjection>,
    /// Execution instances that are children of a logical principal.
    pub subagents: Vec<AgentProjection>,
    pub principals: Vec<PrincipalProjection>,
    /// Principals whose temporary admission grant is still active.
    pub onboarding_principals: Vec<PrincipalProjection>,
    pub groups: Vec<GroupProjection>,
    pub dedup_groups: Vec<DedupGroupProjection>,
    pub contributors: Vec<ContributorProjection>,
    pub contributor_sample_size: usize,
}

/// Safe client connection coordinates. The bearer value itself is never
/// returned; only whether transport authentication is enabled.
#[derive(Debug, Clone, Serialize)]
pub struct GatewayConnectionProjection {
    pub bind: String,
    pub client_url: String,
    pub advertised: bool,
    pub shareable: bool,
    pub auth_enabled: bool,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DedupGroupProjection {
    pub dedup_group_id: String,
    pub name: String,
    pub description: String,
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContributorProjection {
    pub user_id: String,
    pub memories: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryProjection {
    pub id: String,
    pub internal_id: String,
    pub content: String,
    pub memory_type: String,
    pub user_id: String,
    pub created_at: String,
    pub source: String,
    pub rbac_scope: String,
    pub context_tags: String,
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphEdgeProjection {
    pub source: String,
    pub target: String,
    pub edge_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryNodeProjection {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub description: String,
    pub memory_count: usize,
    pub child_count: usize,
    pub relation_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryEdgeProjection {
    pub source: String,
    pub target: String,
    pub edge_type: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelationCountProjection {
    pub edge_type: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryBreadcrumbProjection {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryFieldProjection {
    pub view: &'static str,
    pub focus: Option<String>,
    pub breadcrumbs: Vec<CategoryBreadcrumbProjection>,
    pub categories: Vec<CategoryNodeProjection>,
    pub category_edges: Vec<CategoryEdgeProjection>,
    pub relation_totals: Vec<RelationCountProjection>,
    pub memories: Vec<MemoryProjection>,
    pub memory_edges: Vec<GraphEdgeProjection>,
    pub total_memories: usize,
    pub total_categories: usize,
    pub uncategorized_memories: usize,
    pub page: usize,
    pub page_size: usize,
    pub page_count: usize,
    pub groups: Vec<MemoryGroupProjection>,
    pub identities: Vec<MemoryIdentityProjection>,
    pub selected_group: Option<String>,
    pub selected_identity: Option<String>,
    pub query: Option<String>,
    pub snapshot_at: String,
    pub next_refresh_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryGroupProjection {
    pub group_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryIdentityProjection {
    pub identity: String,
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct MoiraiInsightProjection {
    pub memory: MemoryProjection,
    pub source_groups: Vec<String>,
    pub witness_count: usize,
    pub witnesses: Vec<MemoryProjection>,
    pub orphaned: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MoiraiStageProjection {
    pub name: &'static str,
    pub responsibility: &'static str,
    pub state: &'static str,
    pub artifact_count: usize,
    pub last_activity_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MoiraiProjection {
    pub enabled: bool,
    pub mode: &'static str,
    pub daemon_active: bool,
    pub daemon_status: Option<String>,
    pub insights: Vec<MoiraiInsightProjection>,
    pub stages: Vec<MoiraiStageProjection>,
    pub witness_count: usize,
    pub orphan_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoleMutation {
    pub subject_id: String,
    pub role: String,
    pub group_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GroupMutation {
    pub group_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GroupMemberMutation {
    pub group_id: String,
    pub subject_id: String,
    #[serde(default = "default_worker_role")]
    pub role: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GroupUserMutation {
    pub group_id: String,
    pub subject_id: String,
}

/// Place a self-enrolled principal into an existing working group.
#[derive(Debug, Clone, Deserialize)]
pub struct OnboardingPlacementMutation {
    pub principal_id: String,
    pub group_id: String,
    #[serde(default = "default_worker_role")]
    pub role: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResourceMutation {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DedupGroupMutation {
    pub dedup_group_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DedupMembershipMutation {
    pub group_id: String,
    #[serde(default)]
    pub dedup_group_id: String,
}

fn default_worker_role() -> String {
    "worker".to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct MutationReceipt {
    pub ok: bool,
    pub message: String,
}

/// Global-admin permission simulation request, equivalent to `helixir rbac check`.
#[derive(Debug, Clone, Deserialize)]
pub struct AccessCheckRequest {
    pub subject_id: String,
    pub action: String,
    pub owner_id: Option<String>,
}

/// Explainable result for an RBAC permission simulation.
#[derive(Debug, Clone, Serialize)]
pub struct AccessCheckResult {
    pub allowed: bool,
    pub subject_id: String,
    pub action: String,
    pub owner_id: Option<String>,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemProjection {
    pub mode: &'static str,
    pub database: String,
    pub embedding_provider: String,
    pub embedding_model: String,
    pub llm_provider: String,
    pub llm_model: String,
    pub nli_required: bool,
    pub rbac_permanent: bool,
}

/// Stable API error envelope; internal causes stay in server logs.
#[derive(Debug, Clone, Serialize)]
pub struct ApiProblem {
    pub code: &'static str,
    pub message: String,
}

impl DiscoveryResponse {
    pub(super) fn from_state(state: SystemState) -> Self {
        let backend_ready = match &state.backend {
            crate::installer::BackendState::Missing => false,
            crate::installer::BackendState::ManagedLocal {
                healthy,
                schema_compatible,
                ..
            }
            | crate::installer::BackendState::ExistingLocal {
                healthy,
                schema_compatible,
                ..
            }
            | crate::installer::BackendState::Remote {
                healthy,
                schema_compatible,
                ..
            } => *healthy && *schema_compatible,
        };
        let phase = if backend_ready && state.nli_installed && state.rbac.enabled {
            "ready"
        } else {
            "setup"
        };
        Self { phase, state }
    }
}
