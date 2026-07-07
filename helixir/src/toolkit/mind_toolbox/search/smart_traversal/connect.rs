//! `connect_memories(A, B)` — bidirectional path discovery between two
//! anchors (elder-brain #14).
//!
//! The elder-brain question "how is A related to B?" (Rajasthan weather →
//! guar harvest → guar gum → fracking → shale stocks) is a **path query
//! between two seed sets**, which none of the outward-walking tools can
//! answer. This module runs two breadth-first waves — one from each anchor —
//! over `getConnectionsLevelBatch` (one DB call per level per side) until the
//! waves meet, then reconstructs the path with edge types and a cumulative
//! confidence (product of edge weights).

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use tracing::{debug, info};

use super::batch_expansion::fetch_level;
use super::phases::TraversalError;
use crate::core::config::GraphConfig;
use crate::db::HelixClient;

#[derive(Debug, Clone, Serialize)]
pub struct PathNode {
    pub memory_id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PathEdge {
    /// Edge family, e.g. `BECAUSE` (or `BECAUSE_IN` when walked against the
    /// stored direction).
    pub edge_type: String,
    /// Weight contribution of this hop to the cumulative confidence.
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionPath {
    /// From an A-anchor to a B-anchor, inclusive.
    pub nodes: Vec<PathNode>,
    /// `edges[i]` connects `nodes[i]` and `nodes[i+1]`.
    pub edges: Vec<PathEdge>,
    /// Product of edge weights — a rough "how much to trust this chain".
    pub confidence: f64,
    pub hops: usize,
    /// `true` when both anchors resolved to the same memory (seed overlap on
    /// a small corpus) — a shared relevant fact, not a discovered graph path.
    pub shared_seed: bool,
}

/// Per-side traversal state: parent pointers for path reconstruction.
struct Wave {
    /// memory_id -> (parent memory_id, edge_type, weight); seeds map to None.
    parents: HashMap<String, Option<(String, &'static str, f64)>>,
    frontier: Vec<String>,
}

impl Wave {
    fn new(seeds: &[(String, String)]) -> Self {
        let mut parents = HashMap::new();
        let mut frontier = Vec::new();
        for (memory_id, _) in seeds {
            parents.insert(memory_id.clone(), None);
            frontier.push(memory_id.clone());
        }
        Self { parents, frontier }
    }

    /// Walks parent pointers back to this wave's seed. Returns the chain
    /// seed-first: `[(seed, None), ..., (node, Some(edge to previous))]`.
    fn chain_to(&self, node: &str) -> Vec<(String, Option<(String, &'static str, f64)>)> {
        let mut chain = Vec::new();
        let mut current = node.to_string();
        loop {
            let parent = self.parents.get(&current).cloned().flatten();
            chain.push((current.clone(), parent.clone()));
            match parent {
                Some((p, _, _)) => current = p,
                None => break,
            }
        }
        chain.reverse();
        chain
    }
}

/// Finds the shortest connection between two seed sets. `seeds_*` are
/// `(memory_id, content)` pairs (content used for endpoint display).
/// Returns `None` when the waves don't meet within `max_depth` total hops.
pub async fn connect(
    client: &HelixClient,
    seeds_a: &[(String, String)],
    seeds_b: &[(String, String)],
    max_depth: usize,
    graph: &GraphConfig,
) -> Result<Option<ConnectionPath>, TraversalError> {
    let ew = graph.edge_weights;
    let ed = graph.edge_damping;
    if seeds_a.is_empty() || seeds_b.is_empty() {
        return Ok(None);
    }

    let mut content_by_id: HashMap<String, String> = seeds_a
        .iter()
        .chain(seeds_b.iter())
        .map(|(id, content)| (id.clone(), content.clone()))
        .collect();

    let mut wave_a = Wave::new(seeds_a);
    let mut wave_b = Wave::new(seeds_b);

    // The same memory in both seed sets is already a (trivial) connection.
    if let Some(meeting) = wave_a
        .parents
        .keys()
        .find(|id| wave_b.parents.contains_key(*id))
    {
        let meeting = meeting.clone();
        let mut path = build_path(&wave_a, &wave_b, &meeting, &content_by_id);
        path.shared_seed = true;
        return Ok(Some(path));
    }

    for level in 0..max_depth {
        // Expand the smaller frontier — classic bidirectional heuristic.
        let (wave, other) = if wave_a.frontier.len() <= wave_b.frontier.len() {
            (&mut wave_a, &wave_b)
        } else {
            (&mut wave_b, &wave_a)
        };
        if wave.frontier.is_empty() {
            debug!("connect: frontier exhausted at level {level}");
            return Ok(None);
        }

        let ids: Vec<&str> = wave.frontier.iter().map(String::as_str).collect();
        let fetch = fetch_level(client, &ids, ew, ed).await?;

        let uuid_to_mid: HashMap<&str, &str> = fetch
            .nodes_by_uuid
            .iter()
            .map(|(uuid, node)| (uuid.as_str(), node.memory_id.as_str()))
            .collect();
        for node in fetch.nodes_by_uuid.values() {
            content_by_id
                .entry(node.memory_id.clone())
                .or_insert_with(|| node.content.clone());
        }

        let frontier_set: HashSet<&str> = wave.frontier.iter().map(String::as_str).collect();
        let mut next_frontier = Vec::new();
        let mut meeting: Option<String> = None;

        for edge in &fetch.edges {
            let (Some(parent_mid), Some(child_mid)) = (
                uuid_to_mid.get(edge.parent_uuid.as_str()),
                uuid_to_mid.get(edge.child_uuid.as_str()),
            ) else {
                continue;
            };
            if !frontier_set.contains(parent_mid) {
                continue;
            }
            if wave.parents.contains_key(*child_mid) {
                continue;
            }
            wave.parents.insert(
                (*child_mid).to_string(),
                Some((
                    (*parent_mid).to_string(),
                    edge.edge_type,
                    edge.weight * edge.strength_norm,
                )),
            );
            next_frontier.push((*child_mid).to_string());

            if other.parents.contains_key(*child_mid) && meeting.is_none() {
                meeting = Some((*child_mid).to_string());
            }
        }

        // Third-dimension routing (#33): besides reasoning edges, bridge to
        // memories that share a CATEGORY — a perpendicular axis over the flat
        // graph. The Category node is the shared projection; we route THROUGH it
        // (Memory -TAGGED_AS-> Category -In TAGGED_AS-> Memory), materialising no
        // pairwise edge. Skipped once the waves already met this level.
        if meeting.is_none() {
            for parent_mid in wave.frontier.clone() {
                let neighbours =
                    fetch_category_neighbours(client, &parent_mid, graph.connect_bridge_cap as i64)
                        .await?;
                for (child_mid, content) in neighbours {
                    content_by_id.entry(child_mid.clone()).or_insert(content);
                    if wave.parents.contains_key(&child_mid) {
                        continue;
                    }
                    wave.parents.insert(
                        child_mid.clone(),
                        Some((
                            parent_mid.clone(),
                            "VIA_CATEGORY",
                            graph.connect_bridge_weight,
                        )),
                    );
                    if other.parents.contains_key(&child_mid) && meeting.is_none() {
                        meeting = Some(child_mid.clone());
                    }
                    next_frontier.push(child_mid);
                }
            }
        }

        wave.frontier = next_frontier;

        if let Some(meeting) = meeting {
            info!("connect: waves met at {meeting} (level {level})");
            return Ok(Some(build_path(&wave_a, &wave_b, &meeting, &content_by_id)));
        }
    }

    Ok(None)
}

/// Memories sharing a category with `memory_id` (the third-dimension neighbours):
/// `Memory -TAGGED_AS-> Category -In TAGGED_AS-> Memory`. Routes through the
/// shared Category node and dedups; `cap` bounds the per-category fan-out so a
/// broad axis can't blow up the frontier. A memory with no categories (the
/// common case today) yields an empty list — the bridge is purely additive.
async fn fetch_category_neighbours(
    client: &HelixClient,
    memory_id: &str,
    cap: i64,
) -> Result<Vec<(String, String)>, TraversalError> {
    use serde::Deserialize;

    #[derive(Deserialize, Default)]
    struct CategoriesResp {
        #[serde(default)]
        categories: Vec<CatRow>,
    }
    #[derive(Deserialize)]
    struct CatRow {
        #[serde(default, deserialize_with = "crate::utils::nullable_string")]
        category_id: String,
    }
    #[derive(Deserialize, Default)]
    struct MemoriesResp {
        #[serde(default)]
        memories: Vec<MemRow>,
    }
    #[derive(Deserialize)]
    struct MemRow {
        #[serde(default, deserialize_with = "crate::utils::nullable_string")]
        memory_id: String,
        #[serde(default, deserialize_with = "crate::utils::nullable_string")]
        content: String,
    }

    let cats: CategoriesResp = client
        .execute_query(
            "getMemoryCategories",
            &serde_json::json!({ "memory_id": memory_id }),
        )
        .await
        .map_err(|e| TraversalError::Database(e.to_string()))?;

    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<(String, String)> = Vec::new();
    for cat in cats.categories {
        if cat.category_id.is_empty() {
            continue;
        }
        let mems: MemoriesResp = match client
            .execute_query(
                "getMemoriesByCategory",
                &serde_json::json!({
                    "category_id": cat.category_id,
                    "exclude_memory_id": memory_id,
                    "limit": cap,
                }),
            )
            .await
        {
            Ok(m) => m,
            Err(e) => {
                debug!(
                    "connect: getMemoriesByCategory failed for {}: {e}",
                    cat.category_id
                );
                continue;
            }
        };
        for m in mems.memories {
            if m.memory_id.is_empty() {
                continue;
            }
            if seen.insert(m.memory_id.clone()) {
                out.push((m.memory_id, m.content));
            }
        }
    }
    Ok(out)
}

fn build_path(
    wave_a: &Wave,
    wave_b: &Wave,
    meeting: &str,
    content_by_id: &HashMap<String, String>,
) -> ConnectionPath {
    // A-side arrives seed-first and ends at the meeting node.
    let a_chain = wave_a.chain_to(meeting);
    // B-side also arrives seed-first; we need it meeting-first to continue
    // the path towards B's anchor.
    let b_chain = wave_b.chain_to(meeting);

    let mut nodes: Vec<PathNode> = Vec::new();
    let mut edges: Vec<PathEdge> = Vec::new();

    for (memory_id, edge) in &a_chain {
        if let Some((_, edge_type, weight)) = edge {
            edges.push(PathEdge {
                edge_type: (*edge_type).to_string(),
                weight: *weight,
            });
        }
        nodes.push(PathNode {
            memory_id: memory_id.clone(),
            content: content_by_id.get(memory_id).cloned().unwrap_or_default(),
        });
    }

    // Walk the B chain backwards from the meeting node to B's seed; each
    // step's edge belongs between the previous node and this one.
    for window in b_chain.windows(2).rev() {
        let (child_id, child_edge) = &window[1];
        let (parent_id, _) = &window[0];
        let _ = child_id; // the meeting node itself is already in `nodes`
        if let Some((_, edge_type, weight)) = child_edge {
            edges.push(PathEdge {
                edge_type: (*edge_type).to_string(),
                weight: *weight,
            });
        }
        nodes.push(PathNode {
            memory_id: parent_id.clone(),
            content: content_by_id.get(parent_id).cloned().unwrap_or_default(),
        });
    }

    let confidence = edges.iter().map(|e| e.weight).product::<f64>().min(1.0);
    let hops = edges.len();
    ConnectionPath {
        nodes,
        edges,
        confidence,
        hops,
        shared_seed: false,
    }
}
