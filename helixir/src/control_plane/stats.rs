//! Bounded, read-only projections for the administrator control plane.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;

use crate::core::{RbacManager, RbacPolicy};
use crate::db::HelixClient;
use crate::utils::nullable_string;

use super::dto::DedupGroupProjection;
use super::{
    AccessProjection, AgentFamilyProjection, AgentProjection, ContributorProjection,
    GroupProjection, GroupRoleProjection, MemoryProjection, OverviewStats, PrincipalProjection,
    SystemProjection,
};

const CONTRIBUTOR_SAMPLE_LIMIT: i64 = 1_000;

pub(super) async fn load_overview(db: &Arc<HelixClient>, actor: &str) -> OverviewStats {
    let config = crate::core::HelixirConfig::from_env();
    let Ok(policy) = admin_policy(db, actor).await else {
        return empty_stats(actor, config.mode.label());
    };
    let known_principals = policy.users.keys().cloned().collect::<BTreeSet<_>>();
    let agents = load_agents(db, config.swarm.active_window_secs, &known_principals).await;
    let agent_families = aggregate_agent_families(&agents);
    let subagents = agents
        .iter()
        .filter(|agent| agent.agent_id != agent.principal_id)
        .collect::<Vec<_>>();
    let memories = query_count(db, "countAllMemories").await;
    let users = query_count(db, "countAllUsers").await;
    let entities = query_count(db, "countAllEntities").await;
    let concepts = query_count(db, "countAllConcepts").await;
    let graph_nodes = [memories, users, entities, concepts]
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .map(|counts| {
            counts.into_iter().sum::<u64>()
                + agents.len() as u64
                + policy.groups.len() as u64
                + policy.dedup_groups.len() as u64
        });

    OverviewStats {
        actor_id: actor.to_string(),
        access_scope: "global",
        mode: config.mode.label(),
        memories,
        graph_nodes,
        principals: policy.users.len(),
        active_agents: agent_families.iter().filter(|family| family.active).count(),
        agents: agent_families.len(),
        active_agent_instances: agents.iter().filter(|agent| agent.active).count(),
        agent_instances: agents.len(),
        active_subagents: subagents.iter().filter(|agent| agent.active).count(),
        subagents: subagents.len(),
        workspaces: policy.groups.len(),
        entities,
        concepts,
    }
}

pub(super) async fn load_access(db: &Arc<HelixClient>, actor: &str) -> Option<AccessProjection> {
    let policy = admin_policy(db, actor).await.ok()?;
    let active_window_secs = crate::core::HelixirConfig::from_env()
        .swarm
        .active_window_secs;
    let known_principals = policy.users.keys().cloned().collect::<BTreeSet<_>>();
    let agents = load_agents(db, active_window_secs, &known_principals).await;
    let agent_families = aggregate_agent_families(&agents);
    let subagents = agents
        .iter()
        .filter(|agent| agent.agent_id != agent.principal_id)
        .cloned()
        .collect();
    let principals = policy
        .users
        .iter()
        .map(|(subject_id, binding)| PrincipalProjection {
            subject_id: subject_id.clone(),
            global_roles: binding
                .global_roles
                .iter()
                .map(|role| role.label())
                .collect(),
            groups: binding
                .groups
                .iter()
                .map(|(group_id, roles)| GroupRoleProjection {
                    group_id: group_id.clone(),
                    roles: roles.iter().map(|role| role.label()).collect(),
                })
                .collect(),
        })
        .collect();
    let groups = policy
        .groups
        .iter()
        .map(|(group_id, group)| GroupProjection {
            group_id: group_id.clone(),
            name: group.name.clone(),
            description: group.description.clone(),
            dedup_group_id: group.dedup_group_id.clone(),
            member_count: policy
                .users
                .values()
                .filter(|binding| binding.groups.contains_key(group_id))
                .count(),
            reserved: matches!(group_id.as_str(), "default" | "onboarding" | "moirai"),
        })
        .collect();
    let dedup_groups = policy
        .dedup_groups
        .iter()
        .map(|(dedup_group_id, dedup)| DedupGroupProjection {
            dedup_group_id: dedup_group_id.clone(),
            name: dedup.name.clone(),
            description: dedup.description.clone(),
            groups: policy
                .groups
                .iter()
                .filter(|(_, group)| {
                    group.dedup_group_id.as_deref() == Some(dedup_group_id.as_str())
                })
                .map(|(group_id, _)| group_id.clone())
                .collect(),
        })
        .collect();
    let sample: RecentResponse = db
        .execute_query_no_retry(
            "getRecentMemories",
            &serde_json::json!({"limit": CONTRIBUTOR_SAMPLE_LIMIT}),
        )
        .await
        .unwrap_or_default();
    let contributor_sample_size = sample.memories.len();
    let mut contribution_counts = HashMap::<String, usize>::new();
    for memory in sample.memories {
        if !memory.user_id.is_empty() {
            *contribution_counts.entry(memory.user_id).or_default() += 1;
        }
    }
    let mut contributors = contribution_counts
        .into_iter()
        .map(|(user_id, memories)| ContributorProjection { user_id, memories })
        .collect::<Vec<_>>();
    contributors.sort_by_key(|row| std::cmp::Reverse(row.memories));
    contributors.truncate(8);
    Some(AccessProjection {
        active_window_secs,
        agents,
        agent_families,
        subagents,
        principals,
        groups,
        dedup_groups,
        contributors,
        contributor_sample_size,
    })
}

pub(super) async fn load_system(db: &Arc<HelixClient>, actor: &str) -> Option<SystemProjection> {
    admin_policy(db, actor).await.ok()?;
    let config = crate::core::HelixirConfig::from_env();
    Some(SystemProjection {
        mode: config.mode.label(),
        database: format!("{}:{}", config.host, config.port),
        embedding_provider: config.embedding_provider,
        embedding_model: config.embedding_model,
        llm_provider: config.llm_provider,
        llm_model: config.llm_model,
        nli_required: config.write.nli_route,
        rbac_permanent: true,
    })
}

pub(super) fn resolve_operator_id() -> String {
    std::env::var("HELIXIR_RBAC_ACTOR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(manifest_operator_id)
        .unwrap_or_else(|| "bootstrap".to_string())
}

pub(super) async fn admin_policy(db: &Arc<HelixClient>, actor: &str) -> Result<RbacPolicy, ()> {
    let policy = RbacManager::new(Arc::clone(db))
        .snapshot()
        .await
        .map_err(|error| {
            tracing::warn!(%error, "control-plane RBAC snapshot unavailable");
        })?;
    policy.is_admin(actor).then_some(policy).ok_or(())
}

fn manifest_operator_id() -> Option<String> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    crate::installer::manifest::read(&home.join(".helixir/install.json"))
        .ok()
        .flatten()?
        .rbac
        .map(|rbac| rbac.operator_id)
        .filter(|value| !value.trim().is_empty())
}

async fn query_count(db: &HelixClient, query: &str) -> Option<u64> {
    db.execute_query_no_retry::<serde_json::Value, _>(query, &serde_json::json!({}))
        .await
        .ok()
        .and_then(|value| first_unsigned(&value))
}

pub(super) async fn load_agents(
    db: &HelixClient,
    active_window_secs: u64,
    known_principals: &BTreeSet<String>,
) -> Vec<AgentProjection> {
    let response: AgentsResponse = db
        .execute_query_no_retry("listAgents", &serde_json::json!({}))
        .await
        .unwrap_or_default();
    let now = chrono::Utc::now();
    let mut agents: Vec<_> = response
        .agents
        .into_iter()
        .filter(|row| !row.agent_id.is_empty())
        .map(|row| {
            let age_seconds = chrono::DateTime::parse_from_rfc3339(row.last_seen.trim())
                .ok()
                .map(|seen| (now - seen.with_timezone(&chrono::Utc)).num_seconds());
            let principal_id =
                resolve_agent_principal(&row.principal_id, &row.agent_id, known_principals);
            AgentProjection {
                active: crate::toolkit::tooling_manager::swarm::status_allows_activity(
                    &row.status,
                ) && matches!(age_seconds, Some(age) if (0..=active_window_secs as i64).contains(&age)),
                age_seconds,
                agent_id: row.agent_id,
                principal_id,
                name: row.name,
                role: row.role,
                host: row.host,
                status: row.status,
                last_seen: row.last_seen,
            }
        })
        .collect();
    agents.sort_by_key(|agent| (!agent.active, agent.age_seconds.unwrap_or(i64::MAX)));
    agents
}

/// Legacy presence rows predate `Agent.principal_id`. The admin projection may
/// group those rows by an existing RBAC principal for display, but this result
/// is never used for authorization or persisted back into the graph. Explicit
/// principal ids always win for new rows.
pub(super) fn resolve_agent_principal(
    explicit: &str,
    agent_id: &str,
    known_principals: &BTreeSet<String>,
) -> String {
    crate::utils::resolve_agent_principal_for_display(explicit, agent_id, known_principals)
}

fn aggregate_agent_families(agents: &[AgentProjection]) -> Vec<AgentFamilyProjection> {
    let mut families = BTreeMap::<String, AgentFamilyProjection>::new();
    for agent in agents {
        let family = families
            .entry(agent.principal_id.clone())
            .or_insert_with(|| AgentFamilyProjection {
                principal_id: agent.principal_id.clone(),
                instance_count: 0,
                active_instances: 0,
                hosts: Vec::new(),
                instances: Vec::new(),
                active: false,
            });
        family.instance_count += 1;
        if !agent.host.is_empty() && !family.hosts.contains(&agent.host) {
            family.hosts.push(agent.host.clone());
        }
        family.instances.push(agent.clone());
        if agent.active {
            family.active_instances += 1;
            family.active = true;
        }
    }
    let mut families = families.into_values().collect::<Vec<_>>();
    for family in &mut families {
        family.hosts.sort();
        family.instances.sort_by(|left, right| {
            right
                .active
                .cmp(&left.active)
                .then_with(|| left.agent_id.cmp(&right.agent_id))
        });
    }
    families.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then_with(|| left.principal_id.cmp(&right.principal_id))
    });
    families
}

fn first_unsigned(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(number) => number.as_u64(),
        serde_json::Value::Array(values) => values.iter().find_map(first_unsigned),
        serde_json::Value::Object(values) => values.values().find_map(first_unsigned),
        _ => None,
    }
}

fn empty_stats(actor: &str, mode: &'static str) -> OverviewStats {
    OverviewStats {
        actor_id: actor.to_string(),
        access_scope: "denied",
        mode,
        memories: None,
        graph_nodes: None,
        principals: 0,
        agents: 0,
        active_agents: 0,
        agent_instances: 0,
        active_agent_instances: 0,
        subagents: 0,
        active_subagents: 0,
        workspaces: 0,
        entities: None,
        concepts: None,
    }
}

#[derive(Debug, Deserialize, Default)]
struct AgentsResponse {
    #[serde(default)]
    agents: Vec<AgentRow>,
}

#[derive(Debug, Deserialize)]
struct AgentRow {
    #[serde(default, deserialize_with = "nullable_string")]
    agent_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    principal_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    name: String,
    #[serde(default, deserialize_with = "nullable_string")]
    role: String,
    #[serde(default, deserialize_with = "nullable_string")]
    host: String,
    #[serde(default, deserialize_with = "nullable_string")]
    last_seen: String,
    #[serde(default, deserialize_with = "nullable_string")]
    status: String,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RecentResponse {
    #[serde(default)]
    pub memories: Vec<MemoryRow>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct MemoryRow {
    #[serde(default, deserialize_with = "nullable_string")]
    pub id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub memory_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub content: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub memory_type: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub user_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub created_at: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub source: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub rbac_scope: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub context_tags: String,
}

impl MemoryRow {
    pub(super) fn into_projection(self, groups: Vec<String>) -> MemoryProjection {
        MemoryProjection {
            id: self.memory_id,
            internal_id: self.id,
            content: self.content,
            memory_type: self.memory_type,
            user_id: self.user_id,
            created_at: self.created_at,
            source: self.source,
            rbac_scope: self.rbac_scope,
            context_tags: self.context_tags,
            groups,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_counts_and_null_strings_are_supported() {
        assert_eq!(
            first_unsigned(&serde_json::json!({"count": [42]})),
            Some(42)
        );
        let response: AgentsResponse = serde_json::from_value(serde_json::json!({
            "agents": [{"agent_id":"codex", "last_seen": null}]
        }))
        .unwrap();
        assert_eq!(response.agents[0].agent_id, "codex");
        assert!(response.agents[0].last_seen.is_empty());
    }

    #[test]
    fn explicit_principal_wins_and_legacy_rows_use_longest_known_prefix() {
        let known = ["codex".to_string(), "codex-web".to_string()]
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert_eq!(
            resolve_agent_principal("claude", "codex-web-build", &known),
            "claude"
        );
        assert_eq!(
            resolve_agent_principal("", "codex-web-build", &known),
            "codex-web"
        );
        assert_eq!(
            resolve_agent_principal("", "unregistered-worker", &known),
            "unregistered-worker"
        );
    }

    #[test]
    fn control_plane_families_keep_instance_and_logical_counts_distinct() {
        let instance = |agent_id: &str, principal_id: &str, active: bool| AgentProjection {
            agent_id: agent_id.to_string(),
            principal_id: principal_id.to_string(),
            name: agent_id.to_string(),
            role: "developer".to_string(),
            host: "host".to_string(),
            status: if active { "working" } else { "done" }.to_string(),
            last_seen: "2026-08-22T00:00:00Z".to_string(),
            age_seconds: Some(0),
            active,
        };
        let agents = vec![
            instance("codex", "codex", false),
            instance("codex-build", "codex", true),
            instance("codex-review", "codex", true),
            instance("codex-research", "codex", true),
        ];
        let families = aggregate_agent_families(&agents);

        assert_eq!(families.len(), 1);
        let codex = families
            .iter()
            .find(|family| family.principal_id == "codex")
            .unwrap();
        assert_eq!(codex.instance_count, 4);
        assert_eq!(codex.active_instances, 3);
        assert_eq!(codex.hosts, vec!["host"]);
        assert_eq!(codex.instances.len(), 4);
        assert!(codex.active);
        assert_eq!(
            agents
                .iter()
                .filter(|agent| agent.agent_id != agent.principal_id)
                .count(),
            3
        );
    }
}
