use serde::{Deserialize, Deserializer};
use tracing::{info, debug};

use crate::utils::nullable_string;
use super::types::ToolingError;
use super::ToolingManager;

impl ToolingManager {
    pub async fn get_memory_graph(
        &self,
        user_id: &str,
        memory_id: Option<&str>,
        depth: usize,
    ) -> Result<(Vec<serde_json::Value>, Vec<serde_json::Value>), ToolingError> {
        info!("Getting memory graph for user={}, memory={:?}, depth={}", user_id, memory_id, depth);

        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut visited = std::collections::HashSet::new();

        let start_ids: Vec<String> = if let Some(mid) = memory_id {
            vec![mid.to_string()]
        } else {
            #[derive(serde::Deserialize)]
            struct UserMemoriesResult {
                #[serde(default)]
                memories: Vec<MemoryNode>,
            }
            #[derive(serde::Deserialize)]
            struct MemoryNode {
                #[serde(default, deserialize_with = "nullable_string")]
                memory_id: String,
                #[serde(default, deserialize_with = "nullable_string")]
                content: String,
            }

            match self.db.execute_query::<UserMemoriesResult, _>(
                "getUserMemories",
                &serde_json::json!({"user_id": user_id, "limit": 10i64}),
            ).await {
                Ok(result) => result.memories.into_iter().map(|m| m.memory_id).collect(),
                Err(_) => Vec::new(),
            }
        };

        if start_ids.is_empty() {
            return Ok((nodes, edges));
        }

        let mut current_ids = start_ids;
        let mut current_depth = 0;

        while current_depth < depth && !current_ids.is_empty() {
            let mut next_ids = Vec::new();

            for mid in &current_ids {
                if visited.contains(mid) {
                    continue;
                }
                visited.insert(mid.clone());

                #[derive(serde::Deserialize)]
                struct MemoryResult {
                    #[serde(default)]
                    memory: Option<MemoryData>,
                }
                #[derive(serde::Deserialize)]
                struct MemoryData {
                    #[serde(default, deserialize_with = "nullable_string")]
                    memory_id: String,
                    #[serde(default, deserialize_with = "nullable_string")]
                    content: String,
                    #[serde(default, deserialize_with = "nullable_string")]
                    memory_type: String,
                }

                if let Ok(result) = self.db.execute_query::<MemoryResult, _>(
                    "getMemory",
                    &serde_json::json!({"memory_id": mid}),
                ).await {
                    if let Some(mem) = result.memory {
                        nodes.push(serde_json::json!({
                            "id": mem.memory_id,
                            "content": mem.content,
                            "type": mem.memory_type,
                        }));
                    }
                }

                #[derive(serde::Deserialize, Default)]
                struct ConnectionsResult {
                    #[serde(default)]
                    implies_out: Vec<ConnectedMemory>,
                    #[serde(default)]
                    implies_in: Vec<ConnectedMemory>,
                    #[serde(default)]
                    because_out: Vec<ConnectedMemory>,
                    #[serde(default)]
                    because_in: Vec<ConnectedMemory>,
                    #[serde(default)]
                    contradicts_out: Vec<ConnectedMemory>,
                    #[serde(default)]
                    contradicts_in: Vec<ConnectedMemory>,
                    #[serde(default)]
                    relation_out: Vec<ConnectedMemory>,
                    #[serde(default)]
                    relation_in: Vec<ConnectedMemory>,
                }
                #[derive(serde::Deserialize)]
                struct ConnectedMemory {
                    #[serde(default, deserialize_with = "nullable_string")]
                    memory_id: String,
                    #[serde(default, deserialize_with = "nullable_string")]
                    content: String,
                }

                if let Ok(conns) = self.db.execute_query::<ConnectionsResult, _>(
                    "getMemoryLogicalConnections",
                    &serde_json::json!({"memory_id": mid}),
                ).await {
                    let edge_groups: &[(&Vec<ConnectedMemory>, &str, bool)] = &[
                        (&conns.implies_out, "IMPLIES", true),
                        (&conns.implies_in, "IMPLIES", false),
                        (&conns.because_out, "BECAUSE", true),
                        (&conns.because_in, "BECAUSE", false),
                        (&conns.contradicts_out, "CONTRADICTS", true),
                        (&conns.relation_out, "SUPPORTS", true),
                    ];

                    for (group, edge_type, is_outgoing) in edge_groups {
                        for conn in group.iter() {
                            let (source, target) = if *is_outgoing {
                                (mid.as_str(), conn.memory_id.as_str())
                            } else {
                                (conn.memory_id.as_str(), mid.as_str())
                            };
                            edges.push(serde_json::json!({
                                "source": source,
                                "target": target,
                                "type": edge_type,
                                "weight": 1.0,
                            }));
                            next_ids.push(conn.memory_id.clone());
                        }
                    }
                }
            }

            current_ids = next_ids;
            current_depth += 1;
        }

        info!("Graph built: {} nodes, {} edges", nodes.len(), edges.len());
        Ok((nodes, edges))
    }
}
