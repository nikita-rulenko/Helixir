//! Pure projections from a cached category snapshot into UI-sized atlas views.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::dto::{
    CategoryBreadcrumbProjection, CategoryEdgeProjection, CategoryNodeProjection,
    GraphEdgeProjection, MemoryFieldProjection, MemoryGroupProjection, MemoryIdentityProjection,
    RelationCountProjection,
};
use super::graph::SelectedIdentity;
use super::graph_snapshot::{CategoryRecord, CategorySnapshot, REFRESH_INTERVAL};

const CATEGORY_PAGE_SIZE: usize = 12;
const MEMORY_PAGE_SIZE: usize = 24;
const UNCATEGORIZED_ID: &str = "atlas:uncategorized";

#[allow(clippy::too_many_arguments)]
pub(super) fn project(
    snapshot: &CategorySnapshot,
    groups: Vec<MemoryGroupProjection>,
    identities: Vec<MemoryIdentityProjection>,
    agent_principals: &HashMap<String, String>,
    selected_group: Option<&str>,
    selected_identity: Option<&str>,
    identity: Option<&SelectedIdentity<'_>>,
    focus: Option<&str>,
    query: Option<&str>,
    page: usize,
) -> Option<MemoryFieldProjection> {
    let allowed = allowed_memories(snapshot, selected_group, identity, agent_principals);
    if focus.is_some_and(|id| id != UNCATEGORIZED_ID && !snapshot.categories.contains_key(id)) {
        return None;
    }
    let children = focus
        .filter(|id| *id != UNCATEGORIZED_ID)
        .map(|parent| direct_children(snapshot, parent, &allowed))
        .unwrap_or_default();
    let view = if focus.is_none() || !children.is_empty() {
        "categories"
    } else {
        "memories"
    };
    let (categories, category_edges, total_categories, effective_page, page_count) =
        if view == "categories" {
            category_page(snapshot, &allowed, focus, children, query, page)
        } else {
            (Vec::new(), Vec::new(), 0, page, 1)
        };
    let (memories, memory_edges, total_memories, effective_page, page_count) = if view == "memories"
    {
        memory_page(snapshot, focus?, &allowed, page)
    } else {
        (
            Vec::new(),
            Vec::new(),
            allowed.len(),
            effective_page,
            page_count,
        )
    };
    let uncategorized_memories = members_for(snapshot, UNCATEGORIZED_ID, &allowed).len();
    let relation_totals = global_relation_counts(snapshot, &allowed);
    let snapshot_at = snapshot.snapshot_at.to_rfc3339();
    let next_refresh_at = (snapshot.snapshot_at
        + chrono::Duration::from_std(REFRESH_INTERVAL).unwrap_or_default())
    .to_rfc3339();
    Some(MemoryFieldProjection {
        view,
        focus: focus.map(str::to_string),
        breadcrumbs: breadcrumbs(snapshot, focus),
        categories,
        category_edges,
        relation_totals,
        memories,
        memory_edges,
        total_memories,
        total_categories,
        uncategorized_memories,
        page: effective_page,
        page_size: if view == "categories" {
            CATEGORY_PAGE_SIZE
        } else {
            MEMORY_PAGE_SIZE
        },
        page_count,
        groups,
        identities,
        selected_group: selected_group.map(str::to_string),
        selected_identity: selected_identity.map(str::to_string),
        query: query.map(str::to_string),
        snapshot_at,
        next_refresh_at,
    })
}

fn allowed_memories(
    snapshot: &CategorySnapshot,
    selected_group: Option<&str>,
    identity: Option<&SelectedIdentity<'_>>,
    agent_principals: &HashMap<String, String>,
) -> HashSet<String> {
    snapshot
        .memories
        .iter()
        .filter(|(internal, row)| {
            selected_group.is_none_or(|group| {
                snapshot
                    .memory_groups
                    .get(*internal)
                    .is_some_and(|values| values.iter().any(|value| value == group))
            }) && identity.is_none_or(|selected| match selected.kind {
                "user" => row.user_id == selected.value,
                "agent" => snapshot.memory_agents.get(*internal).is_some_and(|values| {
                    values.iter().any(|agent_id| {
                        agent_principals
                            .get(agent_id)
                            .map(String::as_str)
                            .unwrap_or(agent_id)
                            == selected.value
                    })
                }),
                _ => false,
            })
        })
        .map(|(internal, _)| internal.clone())
        .collect()
}

fn category_page(
    snapshot: &CategorySnapshot,
    allowed: &HashSet<String>,
    focus: Option<&str>,
    children: BTreeSet<String>,
    query: Option<&str>,
    requested_page: usize,
) -> (
    Vec<CategoryNodeProjection>,
    Vec<CategoryEdgeProjection>,
    usize,
    usize,
    usize,
) {
    let candidates = if focus.is_some() {
        children
    } else {
        snapshot.categories.keys().cloned().collect()
    };
    let query = query
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase);
    let mut counted = candidates
        .into_iter()
        .filter_map(|id| {
            let members = members_for(snapshot, &id, allowed);
            if members.is_empty() {
                return None;
            }
            let record = display_record(snapshot, &id);
            let matches = query.as_ref().is_none_or(|query| {
                id.to_lowercase().contains(query)
                    || record.name.to_lowercase().contains(query)
                    || record.description.to_lowercase().contains(query)
            });
            matches.then_some((id, record, members))
        })
        .collect::<Vec<_>>();
    counted.sort_by(|left, right| {
        right
            .2
            .len()
            .cmp(&left.2.len())
            .then_with(|| left.1.name.cmp(&right.1.name))
    });
    let total = counted.len();
    let page_count = total.div_ceil(CATEGORY_PAGE_SIZE).max(1);
    let page = requested_page.min(page_count).max(1);
    let displayed = counted
        .into_iter()
        .skip((page - 1) * CATEGORY_PAGE_SIZE)
        .take(CATEGORY_PAGE_SIZE)
        .collect::<Vec<_>>();
    let displayed_ids = displayed
        .iter()
        .map(|(id, _, _)| id.clone())
        .collect::<BTreeSet<_>>();
    let buckets = bucket_memories(snapshot, allowed, &displayed_ids);
    let category_edges = aggregate_edges(snapshot, allowed, &buckets);
    let relation_counts = relation_counts(&category_edges);
    let categories = displayed
        .into_iter()
        .map(|(id, record, members)| CategoryNodeProjection {
            child_count: direct_children(snapshot, &id, allowed).len(),
            relation_count: relation_counts.get(&id).copied().unwrap_or_default(),
            memory_count: members.len(),
            id,
            name: record.name,
            kind: record.kind,
            description: record.description,
        })
        .collect();
    (categories, category_edges, total, page, page_count)
}

fn direct_children(
    snapshot: &CategorySnapshot,
    parent: &str,
    allowed: &HashSet<String>,
) -> BTreeSet<String> {
    snapshot
        .parent_by_category
        .iter()
        .filter(|(_, candidate_parent)| candidate_parent.as_str() == parent)
        .map(|(child, _)| child)
        .filter(|child| !members_for(snapshot, child, allowed).is_empty())
        .cloned()
        .collect()
}

fn bucket_memories(
    snapshot: &CategorySnapshot,
    allowed: &HashSet<String>,
    displayed: &BTreeSet<String>,
) -> HashMap<String, BTreeSet<String>> {
    allowed
        .iter()
        .map(|internal| {
            let mut buckets = snapshot
                .memory_categories
                .get(internal)
                .into_iter()
                .flatten()
                .filter(|category| displayed.contains(*category))
                .cloned()
                .collect::<BTreeSet<_>>();
            if buckets.is_empty()
                && displayed.contains(UNCATEGORIZED_ID)
                && is_uncategorized(snapshot, internal)
            {
                buckets.insert(UNCATEGORIZED_ID.to_string());
            }
            (internal.clone(), buckets)
        })
        .collect()
}

fn aggregate_edges(
    snapshot: &CategorySnapshot,
    allowed: &HashSet<String>,
    buckets: &HashMap<String, BTreeSet<String>>,
) -> Vec<CategoryEdgeProjection> {
    let mut counts = BTreeMap::<(String, String, String), usize>::new();
    for relation in &snapshot.relations {
        if !allowed.contains(&relation.source) || !allowed.contains(&relation.target) {
            continue;
        }
        let (Some(sources), Some(targets)) =
            (buckets.get(&relation.source), buckets.get(&relation.target))
        else {
            continue;
        };
        for source in sources {
            for target in targets {
                *counts
                    .entry((source.clone(), target.clone(), relation.edge_type.clone()))
                    .or_default() += 1;
            }
        }
    }
    counts
        .into_iter()
        .map(
            |((source, target, edge_type), count)| CategoryEdgeProjection {
                source,
                target,
                edge_type,
                count,
            },
        )
        .collect()
}

fn relation_counts(edges: &[CategoryEdgeProjection]) -> HashMap<String, usize> {
    edges.iter().fold(HashMap::new(), |mut counts, edge| {
        *counts.entry(edge.source.clone()).or_default() += edge.count;
        *counts.entry(edge.target.clone()).or_default() += edge.count;
        counts
    })
}

fn global_relation_counts(
    snapshot: &CategorySnapshot,
    allowed: &HashSet<String>,
) -> Vec<RelationCountProjection> {
    let mut counts = BTreeMap::<String, usize>::new();
    for edge in &snapshot.relations {
        if allowed.contains(&edge.source) && allowed.contains(&edge.target) {
            *counts.entry(edge.edge_type.clone()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .map(|(edge_type, count)| RelationCountProjection { edge_type, count })
        .collect()
}

fn members_for(
    snapshot: &CategorySnapshot,
    category: &str,
    allowed: &HashSet<String>,
) -> HashSet<String> {
    allowed
        .iter()
        .filter(|internal| {
            if category == UNCATEGORIZED_ID {
                return is_uncategorized(snapshot, internal);
            }
            snapshot
                .memory_categories
                .get(*internal)
                .is_some_and(|values| values.iter().any(|value| value == category))
        })
        .cloned()
        .collect()
}

fn is_uncategorized(snapshot: &CategorySnapshot, internal: &str) -> bool {
    snapshot
        .memory_categories
        .get(internal)
        .is_none_or(Vec::is_empty)
}

fn memory_page(
    snapshot: &CategorySnapshot,
    category: &str,
    allowed: &HashSet<String>,
    requested_page: usize,
) -> (
    Vec<super::MemoryProjection>,
    Vec<GraphEdgeProjection>,
    usize,
    usize,
    usize,
) {
    let mut ids = members_for(snapshot, category, allowed)
        .into_iter()
        .collect::<Vec<_>>();
    ids.sort_by(|left, right| {
        snapshot.memories[right]
            .created_at
            .cmp(&snapshot.memories[left].created_at)
    });
    let total = ids.len();
    let page_count = total.div_ceil(MEMORY_PAGE_SIZE).max(1);
    let page = requested_page.min(page_count).max(1);
    let page_ids = ids
        .into_iter()
        .skip((page - 1) * MEMORY_PAGE_SIZE)
        .take(MEMORY_PAGE_SIZE)
        .collect::<Vec<_>>();
    let page_id_set = page_ids.iter().cloned().collect::<HashSet<_>>();
    let memories = page_ids
        .iter()
        .filter_map(|internal| {
            let groups = snapshot
                .memory_groups
                .get(internal)
                .cloned()
                .unwrap_or_default();
            snapshot
                .memories
                .get(internal)
                .cloned()
                .map(|row| row.into_projection(groups))
        })
        .collect::<Vec<_>>();
    let internal_to_id = memories
        .iter()
        .map(|memory| (memory.internal_id.clone(), memory.id.clone()))
        .collect::<HashMap<_, _>>();
    let memory_edges = snapshot
        .relations
        .iter()
        .filter(|edge| page_id_set.contains(&edge.source) && page_id_set.contains(&edge.target))
        .filter_map(|edge| {
            Some(GraphEdgeProjection {
                source: internal_to_id.get(&edge.source)?.clone(),
                target: internal_to_id.get(&edge.target)?.clone(),
                edge_type: edge.edge_type.clone(),
            })
        })
        .collect();
    (memories, memory_edges, total, page, page_count)
}

fn breadcrumbs(
    snapshot: &CategorySnapshot,
    focus: Option<&str>,
) -> Vec<CategoryBreadcrumbProjection> {
    let mut values = vec![CategoryBreadcrumbProjection {
        id: "all".to_string(),
        name: "All categories".to_string(),
    }];
    let Some(focus) = focus else { return values };
    let mut ancestry = Vec::new();
    let mut current = focus;
    let mut visited = HashSet::new();
    while current != UNCATEGORIZED_ID && visited.insert(current) {
        ancestry.push(current.to_string());
        let Some(parent) = snapshot.parent_by_category.get(current) else {
            break;
        };
        current = parent;
    }
    ancestry.reverse();
    values.extend(ancestry.into_iter().map(|id| {
        let record = display_record(snapshot, &id);
        CategoryBreadcrumbProjection {
            id,
            name: record.name,
        }
    }));
    if focus == UNCATEGORIZED_ID {
        let record = display_record(snapshot, focus);
        values.push(CategoryBreadcrumbProjection {
            id: focus.to_string(),
            name: record.name,
        });
    }
    values
}

fn display_record(snapshot: &CategorySnapshot, id: &str) -> CategoryRecord {
    snapshot
        .categories
        .get(id)
        .cloned()
        .unwrap_or_else(|| CategoryRecord {
            name: "Uncategorized".to_string(),
            kind: "system".to_string(),
            description: "Memories that have not yet been assigned a controlled category."
                .to_string(),
        })
}

#[cfg(test)]
#[path = "graph_tests.rs"]
mod tests;
