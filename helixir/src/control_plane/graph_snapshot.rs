//! Asynchronously refreshed raw topology for the administrator category atlas.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};

use crate::db::HelixClient;
use crate::utils::nullable_string;

use super::stats::MemoryRow;

const CATEGORY_LIMIT: i64 = 1_000;
const MEMORY_LIMIT: i64 = 25_000;
pub(super) const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Default)]
pub(super) struct CategoryGraphCache {
    state: Arc<RwLock<Option<Arc<CategorySnapshot>>>>,
    refresh_lock: Arc<Mutex<()>>,
}

impl CategoryGraphCache {
    pub(super) fn spawn(&self, db: Arc<HelixClient>) {
        let cache = self.clone();
        tokio::spawn(async move {
            cache.refresh(&db).await;
            let mut interval = tokio::time::interval(REFRESH_INTERVAL);
            interval.tick().await;
            loop {
                interval.tick().await;
                cache.refresh(&db).await;
            }
        });
    }

    pub(super) async fn snapshot(&self, db: &HelixClient) -> Option<Arc<CategorySnapshot>> {
        if let Some(snapshot) = self.state.read().await.clone() {
            return Some(snapshot);
        }
        let _guard = self.refresh_lock.lock().await;
        if let Some(snapshot) = self.state.read().await.clone() {
            return Some(snapshot);
        }
        self.load_and_store(db).await;
        self.state.read().await.clone()
    }

    async fn refresh(&self, db: &HelixClient) {
        let _guard = self.refresh_lock.lock().await;
        self.load_and_store(db).await;
    }

    async fn load_and_store(&self, db: &HelixClient) {
        match CategorySnapshot::load(db).await {
            Ok(snapshot) => {
                tracing::info!(
                    categories = snapshot.categories.len(),
                    memories = snapshot.memories.len(),
                    relations = snapshot.relations.len(),
                    "category graph snapshot refreshed"
                );
                *self.state.write().await = Some(Arc::new(snapshot));
            }
            Err(error) => {
                tracing::warn!(%error, "category graph snapshot refresh failed; retaining last good snapshot");
            }
        }
    }
}

pub(super) struct CategorySnapshot {
    pub categories: BTreeMap<String, CategoryRecord>,
    pub memories: BTreeMap<String, MemoryRow>,
    pub memory_categories: HashMap<String, Vec<String>>,
    pub memory_groups: HashMap<String, Vec<String>>,
    pub memory_agents: HashMap<String, Vec<String>>,
    pub parent_by_category: HashMap<String, String>,
    pub relations: Vec<SnapshotRelation>,
    pub snapshot_at: chrono::DateTime<chrono::Utc>,
}

impl CategorySnapshot {
    async fn load(db: &HelixClient) -> anyhow::Result<Self> {
        let raw: RawCategorySnapshot = db
            .execute_query_no_retry(
                "getCategoryGraphSnapshot",
                &serde_json::json!({
                    "category_limit": CATEGORY_LIMIT,
                    "memory_limit": MEMORY_LIMIT,
                }),
            )
            .await?;
        Ok(Self::from_raw(raw))
    }

    fn from_raw(raw: RawCategorySnapshot) -> Self {
        let categories_by_internal = raw
            .categories
            .into_iter()
            .filter(|row| !row.id.is_empty() && !row.category_id.is_empty())
            .map(|row| (row.id.clone(), row))
            .collect::<HashMap<_, _>>();
        let alias_targets = raw
            .alias_edges
            .iter()
            .filter(|edge| !edge.from_node.is_empty() && !edge.to_node.is_empty())
            .map(|edge| (edge.from_node.clone(), edge.to_node.clone()))
            .collect::<HashMap<_, _>>();
        let canonical_id = |internal: &str| {
            let canonical = follow_alias(internal, &alias_targets);
            categories_by_internal
                .get(canonical)
                .map(|row| row.category_id.clone())
        };
        let categories = categories_by_internal
            .values()
            .filter(|row| !alias_targets.contains_key(&row.id))
            .map(|row| {
                (
                    row.category_id.clone(),
                    CategoryRecord {
                        name: row.name.clone(),
                        kind: row.kind.clone(),
                        description: row.description.clone(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let memories = raw
            .memories
            .into_iter()
            .filter(|row| !row.id.is_empty() && !row.memory_id.is_empty())
            .map(|row| (row.id.clone(), row))
            .collect::<BTreeMap<_, _>>();
        let mut memory_categories = HashMap::<String, Vec<String>>::new();
        for edge in raw.tag_edges {
            let Some(category_id) = canonical_id(&edge.to_node) else {
                continue;
            };
            push_unique(&mut memory_categories, edge.from_node, category_id);
        }
        let group_ids = raw
            .groups
            .into_iter()
            .filter(|row| !row.id.is_empty() && !row.group_id.is_empty())
            .map(|row| (row.id, row.group_id))
            .collect::<HashMap<_, _>>();
        let mut memory_groups = HashMap::<String, Vec<String>>::new();
        for edge in raw.group_edges {
            if let Some(group_id) = group_ids.get(&edge.to_node) {
                push_unique(&mut memory_groups, edge.from_node, group_id.clone());
            }
        }
        let agent_ids = raw
            .agents
            .into_iter()
            .filter(|row| !row.id.is_empty() && !row.agent_id.is_empty())
            .map(|row| (row.id, row.agent_id))
            .collect::<HashMap<_, _>>();
        let mut memory_agents = HashMap::<String, Vec<String>>::new();
        for edge in raw.agent_edges {
            if let Some(agent_id) = agent_ids.get(&edge.from_node) {
                push_unique(&mut memory_agents, edge.to_node, agent_id.clone());
            }
        }
        let mut parent_by_category = HashMap::new();
        for edge in raw.subcategory_edges {
            let (Some(child), Some(parent)) =
                (canonical_id(&edge.from_node), canonical_id(&edge.to_node))
            else {
                continue;
            };
            if child != parent {
                parent_by_category.insert(child, parent);
            }
        }
        let mut seen_relations = BTreeSet::new();
        let relations =
            raw.relation_edges
                .into_iter()
                .filter_map(|edge| SnapshotRelation::from_edge(edge, None))
                .chain(
                    raw.contradiction_edges
                        .into_iter()
                        .filter_map(|edge| SnapshotRelation::from_edge(edge, Some("CONTRADICTS"))),
                )
                .chain(
                    raw.implies_edges
                        .into_iter()
                        .filter_map(|edge| SnapshotRelation::from_edge(edge, Some("IMPLIES"))),
                )
                .chain(
                    raw.because_edges
                        .into_iter()
                        .filter_map(|edge| SnapshotRelation::from_edge(edge, Some("BECAUSE"))),
                )
                .chain(raw.moirai_edges.into_iter().filter_map(|edge| {
                    SnapshotRelation::from_edge(edge, Some("MOIRAI_DERIVED_FROM"))
                }))
                .filter(|edge| {
                    memories.contains_key(&edge.source) && memories.contains_key(&edge.target)
                })
                .filter(|edge| {
                    seen_relations.insert((
                        edge.source.clone(),
                        edge.target.clone(),
                        edge.edge_type.clone(),
                    ))
                })
                .collect();
        Self {
            categories,
            memories,
            memory_categories,
            memory_groups,
            memory_agents,
            parent_by_category,
            relations,
            snapshot_at: chrono::Utc::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct CategoryRecord {
    pub name: String,
    pub kind: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub(super) struct SnapshotRelation {
    pub source: String,
    pub target: String,
    pub edge_type: String,
}

impl SnapshotRelation {
    fn from_edge(edge: RawEdge, family: Option<&str>) -> Option<Self> {
        if edge.from_node.is_empty() || edge.to_node.is_empty() {
            return None;
        }
        let edge_type = family
            .unwrap_or(&edge.relation_type)
            .trim()
            .to_ascii_uppercase()
            .replace(' ', "_");
        if edge_type.is_empty() {
            return None;
        }
        Some(Self {
            source: edge.from_node,
            target: edge.to_node,
            edge_type,
        })
    }
}

fn follow_alias<'a>(start: &'a str, aliases: &'a HashMap<String, String>) -> &'a str {
    let mut current = start;
    let mut visited = HashSet::new();
    while let Some(next) = aliases.get(current) {
        if !visited.insert(current) || visited.len() > 32 {
            break;
        }
        current = next;
    }
    current
}

fn push_unique(map: &mut HashMap<String, Vec<String>>, key: String, value: String) {
    let values = map.entry(key).or_default();
    if !values.contains(&value) {
        values.push(value);
    }
}

#[derive(Debug, Default, Deserialize)]
struct RawCategorySnapshot {
    #[serde(default)]
    categories: Vec<CategoryRow>,
    #[serde(default)]
    memories: Vec<MemoryRow>,
    #[serde(default)]
    tag_edges: Vec<RawEdge>,
    #[serde(default)]
    subcategory_edges: Vec<RawEdge>,
    #[serde(default)]
    alias_edges: Vec<RawEdge>,
    #[serde(default)]
    group_edges: Vec<RawEdge>,
    #[serde(default)]
    groups: Vec<GroupRow>,
    #[serde(default)]
    agent_edges: Vec<RawEdge>,
    #[serde(default)]
    agents: Vec<AgentRow>,
    #[serde(default)]
    relation_edges: Vec<RawEdge>,
    #[serde(default)]
    contradiction_edges: Vec<RawEdge>,
    #[serde(default)]
    implies_edges: Vec<RawEdge>,
    #[serde(default)]
    because_edges: Vec<RawEdge>,
    #[serde(default)]
    moirai_edges: Vec<RawEdge>,
}

#[derive(Debug, Deserialize)]
struct CategoryRow {
    #[serde(default, deserialize_with = "nullable_string")]
    id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    category_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    name: String,
    #[serde(default, deserialize_with = "nullable_string")]
    kind: String,
    #[serde(default, deserialize_with = "nullable_string")]
    description: String,
}

#[derive(Debug, Default, Deserialize)]
struct RawEdge {
    #[serde(default, deserialize_with = "nullable_string")]
    from_node: String,
    #[serde(default, deserialize_with = "nullable_string")]
    to_node: String,
    #[serde(default, deserialize_with = "nullable_string")]
    relation_type: String,
}

#[derive(Debug, Deserialize)]
struct GroupRow {
    #[serde(default, deserialize_with = "nullable_string")]
    id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    group_id: String,
}

#[derive(Debug, Deserialize)]
struct AgentRow {
    #[serde(default, deserialize_with = "nullable_string")]
    id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    agent_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_chains_resolve_without_looping_forever() {
        let aliases = HashMap::from([
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "c".to_string()),
        ]);
        assert_eq!(follow_alias("a", &aliases), "c");
        let cycle = HashMap::from([
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "a".to_string()),
        ]);
        assert!(matches!(follow_alias("a", &cycle), "a" | "b"));
    }
}
