//! content_key helpers (#43/#55) — read/group/unify the fingerprint that ties
//! identical (and, via the NLI backstop, paraphrased) facts into one collective
//! consensus group. Thin wrappers over the deployed queries.

use serde::Deserialize;

use super::{ToolingError, ToolingManager};

/// A lightweight memory view for the merge scan.
#[derive(Debug, Clone)]
pub struct MemoryBrief {
    pub memory_id: String,
    pub content: String,
    pub content_key: String,
}

impl ToolingManager {
    /// The fingerprint of one memory (empty if unset/legacy).
    pub async fn content_key_of(&self, memory_id: &str) -> String {
        #[derive(Deserialize)]
        struct Node {
            #[serde(default)]
            content_key: Option<String>,
        }
        #[derive(Deserialize)]
        struct Resp {
            #[serde(default)]
            memory: Option<Node>,
        }
        self.db
            .execute_query::<Resp, _>("getMemory", &serde_json::json!({ "memory_id": memory_id }))
            .await
            .ok()
            .and_then(|r| r.memory)
            .and_then(|n| n.content_key)
            .unwrap_or_default()
    }

    /// All memory_ids that share a fingerprint group.
    pub async fn memories_in_group(&self, content_key: &str) -> Vec<String> {
        #[derive(Deserialize)]
        struct Node {
            #[serde(default)]
            memory_id: String,
        }
        #[derive(Deserialize)]
        struct Resp {
            #[serde(default)]
            memories: Vec<Node>,
        }
        self.db
            .execute_query::<Resp, _>(
                "getMemoriesByContentKey",
                &serde_json::json!({ "content_key": content_key }),
            )
            .await
            .map(|r| {
                r.memories
                    .into_iter()
                    .map(|n| n.memory_id)
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Stamp a fingerprint onto a memory (used by backfill + unify).
    pub async fn set_content_key(&self, memory_id: &str, content_key: &str) -> Result<(), ToolingError> {
        self.db
            .execute_query::<serde_json::Value, _>(
                "setMemoryContentKey",
                &serde_json::json!({ "memory_id": memory_id, "content_key": content_key }),
            )
            .await
            .map(|_| ())
            .map_err(|e| ToolingError::Database(e.to_string()))
    }

    /// Unify two fingerprint groups onto one canonical key: every member of both
    /// groups ends up with `canonical`. Idempotent — members already on
    /// `canonical` are skipped. Returns how many nodes were re-stamped.
    pub async fn unify_content_keys(
        &self,
        key_a: &str,
        key_b: &str,
        canonical: &str,
    ) -> Result<usize, ToolingError> {
        let mut restamped = 0;
        for key in [key_a, key_b] {
            if key == canonical {
                continue;
            }
            for id in self.memories_in_group(key).await {
                if self.set_content_key(&id, canonical).await.is_ok() {
                    restamped += 1;
                }
            }
        }
        Ok(restamped)
    }

    /// A batch of memories as briefs (id + content + fingerprint) for the merge
    /// scan. Paraphrase merging is a COLLECTIVE pass over the whole store (facts
    /// are tied to a user by the node's `user_id` field, not a HAS_MEMORY edge),
    /// so this scans recent memories globally rather than one user's edges.
    pub async fn list_recent_briefs(&self, limit: i64) -> Vec<MemoryBrief> {
        #[derive(Deserialize)]
        struct Node {
            #[serde(default)]
            memory_id: String,
            #[serde(default)]
            content: String,
            #[serde(default)]
            content_key: Option<String>,
        }
        #[derive(Deserialize)]
        struct Resp {
            #[serde(default)]
            memories: Vec<Node>,
        }
        self.db
            .execute_query::<Resp, _>(
                "getRecentMemories",
                &serde_json::json!({ "limit": limit }),
            )
            .await
            .map(|r| {
                r.memories
                    .into_iter()
                    .filter(|n| !n.memory_id.is_empty())
                    .map(|n| MemoryBrief {
                        memory_id: n.memory_id,
                        content: n.content,
                        content_key: n.content_key.unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
