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
        self.ensure_initialized().await?;

        let (nodes, edges) = self
            .tooling_manager
            .get_memory_graph(user_id, memory_id, depth.unwrap_or(2))
            .await
            .map_err(|e| HelixirClientError::Tooling(e.to_string()))?;

        Ok(GraphResult {
            nodes: nodes
                .into_iter()
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
                    metadata: HashMap::new(),
                })
                .collect(),
            edges: edges
                .into_iter()
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
