//! Longest-chain context reconstruction (#47): walk the deepest coherent
//! reasoning chain to replay how an understanding developed, hop by hop.
//!
//! `connect_memories` answers "how are A and B related?". This answers a
//! different question — "what is the longest single thread of reasoning running
//! through a topic?" — the elder-brain narrating cause → effect → supersession.
//! We expand a capped reasoning ego-network from topic seeds (reusing
//! `fetch_level`, so the same weighted IMPLIES/BECAUSE/CONTRADICTS/
//! MEMORY_RELATION edges), then extract THE single longest simple path, ranked
//! by hop count and, as a tiebreak, cumulative confidence.
//!
//! Confidence here finally uses the writer's PER-EDGE weight: `family_weight ×
//! strength_norm` (the LLM `strength`/`probability`), where the rest of the
//! traversal stack only uses the family constant. So a long thread held
//! together by weak edges scores below a shorter, firmer one — the seed of the
//! Lachesis coherence gate.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use tracing::{debug, info};

use super::batch_expansion::fetch_level;
use super::phases::TraversalError;
use crate::core::config::GraphConfig;
use crate::db::HelixClient;

#[derive(Debug, Clone, Serialize)]
pub struct ChainStep {
    pub memory_id: String,
    pub content: String,
    /// The reasoning edge from the PREVIOUS step to this one; `None` for the
    /// first node in the thread.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_type: Option<String>,
    /// Weight that edge contributed (`family_weight × per-edge strength`).
    pub edge_weight: f64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChainNarrative {
    /// Ordered thread, start → … → end. `steps[0]` has no incoming edge.
    pub steps: Vec<ChainStep>,
    pub hops: usize,
    /// Product of edge weights — how much to trust the whole thread.
    pub confidence: f64,
}

/// A directed reasoning edge in the ego-network (memory-id space).
struct Edge {
    to: String,
    edge_type: &'static str,
    /// `family_weight × strength_norm`.
    weight: f64,
}

/// Build the reasoning ego-network around `seed_ids` (capped) and return the
/// single longest simple path through it, ranked by hops then confidence.
/// `seed_ids` are `(memory_id, content)`. `max_hops` bounds expansion depth.
pub async fn longest_chain(
    client: &HelixClient,
    seed_ids: &[(String, String)],
    max_hops: usize,
    graph: &GraphConfig,
) -> Result<Option<ChainNarrative>, TraversalError> {
    let ew = graph.edge_weights;
    let ed = graph.edge_damping;
    let max_ego_nodes = graph.longest_chain_max_ego_nodes;
    if seed_ids.is_empty() {
        return Ok(None);
    }

    // 1) Grow the reasoning ego-network level by level via fetch_level.
    let mut adj: HashMap<String, Vec<Edge>> = HashMap::new();
    let mut content: HashMap<String, String> = HashMap::new();
    let mut created: HashMap<String, String> = HashMap::new();
    for (id, c) in seed_ids {
        content.entry(id.clone()).or_insert_with(|| c.clone());
    }

    let mut frontier: Vec<String> = seed_ids.iter().map(|(id, _)| id.clone()).collect();
    let mut visited: HashSet<String> = frontier.iter().cloned().collect();
    let mut seen_edges: HashSet<(String, String, &'static str)> = HashSet::new();

    for _ in 0..max_hops {
        if frontier.is_empty() || visited.len() >= max_ego_nodes {
            break;
        }
        let ids: Vec<&str> = frontier.iter().map(String::as_str).collect();
        let fetch = fetch_level(client, &ids, ew, ed).await?;

        let uuid_to_mid: HashMap<&str, &str> = fetch
            .nodes_by_uuid
            .iter()
            .map(|(u, n)| (u.as_str(), n.memory_id.as_str()))
            .collect();
        for n in fetch.nodes_by_uuid.values() {
            content
                .entry(n.memory_id.clone())
                .or_insert_with(|| n.content.clone());
            created
                .entry(n.memory_id.clone())
                .or_insert_with(|| n.created_at.clone());
        }

        let mut next: Vec<String> = Vec::new();
        for e in &fetch.edges {
            let (Some(parent), Some(child)) = (
                uuid_to_mid.get(e.parent_uuid.as_str()),
                uuid_to_mid.get(e.child_uuid.as_str()),
            ) else {
                continue;
            };
            if parent == child {
                continue;
            }
            if !seen_edges.insert(((*parent).to_string(), (*child).to_string(), e.edge_type)) {
                continue;
            }
            adj.entry((*parent).to_string()).or_default().push(Edge {
                to: (*child).to_string(),
                edge_type: e.edge_type,
                weight: e.weight * e.strength_norm,
            });
            if visited.len() < max_ego_nodes && visited.insert((*child).to_string()) {
                next.push((*child).to_string());
            }
        }
        frontier = next;
    }

    // 2) Longest simple path: DFS from every node, cycle-guarded, budget-capped.
    let mut best: Vec<(String, Option<(&'static str, f64)>)> = Vec::new();
    let mut best_key = (0usize, 0.0f64); // (hops+1 = nodes, confidence)
    let mut budget = graph.longest_chain_max_dfs_steps as u64;

    let starts: Vec<String> = visited.iter().cloned().collect();
    for start in &starts {
        let mut on_path: HashSet<String> = HashSet::new();
        on_path.insert(start.clone());
        let mut cur: Vec<(String, Option<(&'static str, f64)>)> = vec![(start.clone(), None)];
        dfs(
            start,
            &adj,
            &mut on_path,
            &mut cur,
            1.0,
            &mut best,
            &mut best_key,
            &mut budget,
        );
        if budget == 0 {
            debug!("longest_chain: DFS budget exhausted, returning best-so-far");
            break;
        }
    }

    if best.len() < 2 {
        info!("longest_chain: no multi-node thread in the ego-network");
        return Ok(None);
    }

    let steps: Vec<ChainStep> = best
        .into_iter()
        .map(|(mid, incoming)| ChainStep {
            content: content.get(&mid).cloned().unwrap_or_default(),
            created_at: created.get(&mid).cloned().unwrap_or_default(),
            edge_type: incoming.map(|(t, _)| t.to_string()),
            edge_weight: incoming.map(|(_, w)| w).unwrap_or(0.0),
            memory_id: mid,
        })
        .collect();

    let hops = steps.len() - 1;
    info!(
        "longest_chain: thread of {} hops, confidence {:.4}",
        hops, best_key.1
    );
    Ok(Some(ChainNarrative {
        hops,
        confidence: best_key.1,
        steps,
    }))
}

/// Depth-first longest simple path. Records the best `(nodes, confidence)` seen.
#[allow(clippy::too_many_arguments)]
fn dfs(
    node: &str,
    adj: &HashMap<String, Vec<Edge>>,
    on_path: &mut HashSet<String>,
    cur: &mut Vec<(String, Option<(&'static str, f64)>)>,
    cur_conf: f64,
    best: &mut Vec<(String, Option<(&'static str, f64)>)>,
    best_key: &mut (usize, f64),
    budget: &mut u64,
) {
    if *budget == 0 {
        return;
    }
    *budget -= 1;

    // Longer wins; ties broken by higher cumulative confidence.
    if cur.len() > best_key.0 || (cur.len() == best_key.0 && cur_conf > best_key.1) {
        *best_key = (cur.len(), cur_conf);
        *best = cur.clone();
    }

    if let Some(edges) = adj.get(node) {
        for e in edges {
            if on_path.contains(&e.to) {
                continue;
            }
            on_path.insert(e.to.clone());
            cur.push((e.to.clone(), Some((e.edge_type, e.weight))));
            dfs(
                &e.to,
                adj,
                on_path,
                cur,
                cur_conf * e.weight,
                best,
                best_key,
                budget,
            );
            cur.pop();
            on_path.remove(&e.to);
            if *budget == 0 {
                return;
            }
        }
    }
}
