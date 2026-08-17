//! Cached category atlas access and shared memory/group projections.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Deserialize;

use crate::db::HelixClient;
use crate::utils::nullable_string;

use super::dto::{MemoryFieldProjection, MemoryGroupProjection, MemoryIdentityProjection};
use super::graph_project::project;
use super::graph_snapshot::CategoryGraphCache;
use super::stats::admin_policy;

pub(super) struct MemoryFieldRequest<'a> {
    pub group: Option<&'a str>,
    pub identity: Option<&'a str>,
    pub focus: Option<&'a str>,
    pub query: Option<&'a str>,
    pub page: usize,
}

pub(super) async fn load_memory_field(
    cache: &CategoryGraphCache,
    db: &Arc<HelixClient>,
    actor: &str,
    request: MemoryFieldRequest<'_>,
) -> Option<MemoryFieldProjection> {
    let policy = admin_policy(db, actor).await.ok()?;
    let selected_group = normalized(request.group);
    if selected_group.is_some_and(|group| !policy.groups.contains_key(group)) {
        return None;
    }
    let agent_rows: AgentCatalogResponse = db
        .execute_query_no_retry("listAgents", &serde_json::json!({}))
        .await
        .unwrap_or_default();
    let identities = identity_catalog(policy.users.keys().map(String::as_str), &agent_rows.agents);
    let selected_identity = normalized(request.identity);
    let identity_filter = selected_identity
        .map(SelectedIdentity::parse)
        .transpose()
        .ok()?
        .flatten();
    if identity_filter.as_ref().is_some_and(|selected| {
        !identities
            .iter()
            .any(|known| known.identity == selected.value && known.kind == selected.kind)
    }) {
        return None;
    }
    let groups = policy
        .groups
        .iter()
        .map(|(group_id, group)| MemoryGroupProjection {
            group_id: group_id.clone(),
            name: group.name.clone(),
        })
        .collect::<Vec<_>>();
    let snapshot = cache.snapshot(db).await?;
    project(
        &snapshot,
        groups,
        identities,
        selected_group,
        selected_identity,
        identity_filter.as_ref(),
        normalized(request.focus),
        normalized(request.query),
        request.page.max(1),
    )
}

fn normalized(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "all")
}

pub(super) async fn load_memory_groups(
    db: &HelixClient,
    ids: impl IntoIterator<Item = String>,
) -> HashMap<String, Vec<String>> {
    let memory_ids = ids.into_iter().collect::<Vec<_>>();
    if memory_ids.is_empty() {
        return HashMap::new();
    }
    let response: GroupBatch = db
        .execute_query_no_retry(
            "getMemoryRbacGroupsBatch",
            &serde_json::json!({"memory_ids": memory_ids}),
        )
        .await
        .unwrap_or_default();
    let internal_to_memory = response
        .memories
        .into_iter()
        .map(|row| (row.id, row.memory_id))
        .collect::<HashMap<_, _>>();
    let internal_to_group = response
        .groups
        .into_iter()
        .map(|row| (row.id, row.group_id))
        .collect::<HashMap<_, _>>();
    let mut groups = HashMap::<String, Vec<String>>::new();
    for link in response.links {
        let (Some(memory_id), Some(group_id)) = (
            internal_to_memory.get(&link.from_node),
            internal_to_group.get(&link.to_node),
        ) else {
            continue;
        };
        let values = groups.entry(memory_id.clone()).or_default();
        if !values.contains(group_id) {
            values.push(group_id.clone())
        }
    }
    groups
}

#[derive(Debug, Default, Deserialize)]
struct GroupBatch {
    #[serde(default)]
    memories: Vec<super::stats::MemoryRow>,
    #[serde(default)]
    groups: Vec<RbacGroupRow>,
    #[serde(default)]
    links: Vec<GroupLinkRow>,
}

#[derive(Debug, Deserialize)]
struct RbacGroupRow {
    #[serde(default, deserialize_with = "nullable_string")]
    id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    group_id: String,
}

#[derive(Debug, Deserialize)]
struct GroupLinkRow {
    #[serde(default, deserialize_with = "nullable_string")]
    from_node: String,
    #[serde(default, deserialize_with = "nullable_string")]
    to_node: String,
}

#[derive(Debug)]
pub(super) struct SelectedIdentity<'a> {
    pub(super) kind: &'static str,
    pub(super) value: &'a str,
}

impl<'a> SelectedIdentity<'a> {
    pub(super) fn parse(raw: &'a str) -> Result<Option<Self>, ()> {
        let Some((kind, value)) = raw.split_once(':') else {
            return Err(());
        };
        if value.trim().is_empty() {
            return Err(());
        }
        let kind = match kind {
            "user" => "user",
            "agent" => "agent",
            _ => return Err(()),
        };
        Ok(Some(Self { kind, value }))
    }
}

#[derive(Debug, Default, Deserialize)]
struct AgentCatalogResponse {
    #[serde(default)]
    agents: Vec<AgentCatalogRow>,
}

#[derive(Debug, Deserialize)]
struct AgentCatalogRow {
    #[serde(default, deserialize_with = "nullable_string")]
    agent_id: String,
}

fn identity_catalog<'a>(
    users: impl Iterator<Item = &'a str>,
    agents: &[AgentCatalogRow],
) -> Vec<MemoryIdentityProjection> {
    let mut identities = users
        .filter(|value| !value.trim().is_empty())
        .map(|value| MemoryIdentityProjection {
            identity: value.to_string(),
            kind: "user",
        })
        .chain(
            agents
                .iter()
                .filter(|agent| !agent.agent_id.trim().is_empty())
                .map(|agent| MemoryIdentityProjection {
                    identity: agent.agent_id.clone(),
                    kind: "agent",
                }),
        )
        .collect::<Vec<_>>();
    identities.sort_by(|left, right| {
        (left.kind, left.identity.as_str()).cmp(&(right.kind, right.identity.as_str()))
    });
    identities
}
