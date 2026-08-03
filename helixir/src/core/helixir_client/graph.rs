//! Graph-view method `get_graph` on [`HelixirClient`].

use std::collections::HashMap;

use super::client::HelixirClient;
use super::error::HelixirClientError;
use super::types::{GraphEdge, GraphNode, GraphResult};

impl HelixirClient {
    pub async fn get_graph(
        &self,
        user_id: &str,
        memory_id: Option<&str>,
        depth: Option<usize>,
    ) -> Result<GraphResult, HelixirClientError> {
        self.get_graph_as(user_id, user_id, memory_id, depth).await
    }

    pub async fn get_graph_as(
        &self,
        actor_id: &str,
        owner_id: &str,
        memory_id: Option<&str>,
        depth: Option<usize>,
    ) -> Result<GraphResult, HelixirClientError> {
        self.ensure_initialized().await?;

        let (nodes, edges) = self
            .tooling_manager
            .get_memory_graph(owner_id, memory_id, depth.unwrap_or(2))
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        let policy = self
            .rbac()
            .snapshot()
            .await
            .map_err(|e| HelixirClientError::Operation(e.to_string()))?;
        let allowed_ids = if policy.enabled {
            policy.readable_users(actor_id)
        } else {
            None
        };
        let visible_node_ids = nodes
            .iter()
            .filter(|node| {
                allowed_ids.as_ref().map_or(true, |allowed| {
                    node.get("user_id")
                        .and_then(|value| value.as_str())
                        .is_some_and(|owner| allowed.contains(owner))
                })
            })
            .filter_map(|node| {
                node.get("id")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            })
            .collect::<std::collections::HashSet<_>>();

        Ok(GraphResult {
            nodes: nodes
                .into_iter()
                .filter(|node| {
                    allowed_ids.as_ref().map_or(true, |_| {
                        node.get("id")
                            .and_then(|value| value.as_str())
                            .is_some_and(|id| visible_node_ids.contains(id))
                    })
                })
                .map(|n| GraphNode {
                    id: n
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    content: n
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    node_type: n
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("memory")
                        .to_string(),
                    metadata: n
                        .get("user_id")
                        .and_then(|value| value.as_str())
                        .map(|owner| {
                            HashMap::from([("user_id".to_string(), serde_json::json!(owner))])
                        })
                        .unwrap_or_default(),
                })
                .collect(),
            edges: edges
                .into_iter()
                .filter(|edge| {
                    allowed_ids.as_ref().map_or(true, |_| {
                        edge.get("source")
                            .and_then(|value| value.as_str())
                            .is_some_and(|source| visible_node_ids.contains(source))
                            && edge
                                .get("target")
                                .and_then(|value| value.as_str())
                                .is_some_and(|target| visible_node_ids.contains(target))
                    })
                })
                .map(|e| GraphEdge {
                    source: e
                        .get("source")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    target: e
                        .get("target")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    edge_type: e
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    weight: e.get("weight").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
                })
                .collect(),
        })
    }
}
