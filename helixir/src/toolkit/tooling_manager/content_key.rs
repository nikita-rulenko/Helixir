//! content_key helpers (#43/#55) — read/group/unify the fingerprint that ties
//! identical (and, via the NLI backstop, paraphrased) facts into one collective
//! consensus group. Thin wrappers over the deployed queries.

use serde::Deserialize;

use super::{ToolingError, ToolingManager};

/// The pure fingerprint hash (sha256 over normalized content + type) —
/// re-exported so agent-layer writers (e.g. Atropos insight persistence) can
/// compute keys without reaching into the private add_pipeline.
pub(crate) use super::add_pipeline::store::content_key_scoped as compute_content_key_scoped;

/// A lightweight memory view for the merge scan.
#[derive(Debug, Clone)]
pub struct MemoryBrief {
    pub memory_id: String,
    pub content: String,
    pub content_key: String,
    pub source: String,
    pub security_domain: Option<String>,
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
    pub async fn memories_in_group(&self, content_key: &str) -> Result<Vec<String>, ToolingError> {
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
            .map_err(|error| ToolingError::Database(error.to_string()))
    }

    /// Stamp a fingerprint onto a memory (used by backfill + unify).
    pub async fn set_content_key(
        &self,
        memory_id: &str,
        content_key: &str,
    ) -> Result<(), ToolingError> {
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
            restamped += self.restamp_content_key_group(key, canonical).await?;
        }
        Ok(restamped)
    }

    /// Move one complete non-unique fingerprint group to `canonical` in a
    /// single indexed mutation. Returns the number of updated Memory nodes.
    pub async fn restamp_content_key_group(
        &self,
        content_key: &str,
        canonical: &str,
    ) -> Result<usize, ToolingError> {
        #[derive(Deserialize)]
        struct Resp {
            #[serde(default)]
            updated_count: usize,
        }
        self.db
            .execute_query::<Resp, _>(
                "restampContentKeyGroup",
                &serde_json::json!({
                    "content_key": content_key,
                    "canonical": canonical,
                }),
            )
            .await
            .map(|response| response.updated_count)
            .map_err(|error| ToolingError::Database(error.to_string()))
    }

    /// A batch of memories as briefs (id + content + fingerprint) for the merge
    /// scan. Paraphrase merging is a COLLECTIVE pass over the whole store (facts
    /// are tied to a user by the node's `user_id` field, not a HAS_MEMORY edge),
    /// so this scans recent memories globally rather than one user's edges.
    pub async fn list_recent_briefs(&self, limit: i64) -> Result<Vec<MemoryBrief>, ToolingError> {
        #[derive(Deserialize)]
        struct Node {
            #[serde(default)]
            memory_id: String,
            #[serde(default)]
            content: String,
            #[serde(default)]
            content_key: Option<String>,
            #[serde(default)]
            source: String,
        }
        #[derive(Deserialize)]
        struct Resp {
            #[serde(default)]
            memories: Vec<Node>,
        }
        let nodes = self
            .db
            .execute_query::<Resp, _>("getRecentMemories", &serde_json::json!({ "limit": limit }))
            .await
            .map(|response| response.memories)
            .map_err(|error| ToolingError::Database(error.to_string()))?;
        let ids = nodes
            .iter()
            .map(|node| node.memory_id.clone())
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        let domains = crate::core::RbacManager::new(self.db.clone())
            .memory_security_domains(&ids)
            .await
            .map_err(|error| ToolingError::Database(error.to_string()))?;
        Ok(nodes
            .into_iter()
            .filter(|node| !node.memory_id.is_empty())
            .map(|node| MemoryBrief {
                security_domain: domains.get(&node.memory_id).cloned(),
                memory_id: node.memory_id,
                content: node.content,
                content_key: node.content_key.unwrap_or_default(),
                source: node.source,
            })
            .collect())
    }
}
