//! Collective-scope enrichment: `user_count` and `ControversyInfo` lookups
//! that run in parallel for `scope=collective|all` searches.
//!
//! Used by [`super::dispatch::SearchEngine::search`]; kept as static methods
//! so they can be spawned without a `Self` borrow.

use crate::db::HelixClient;

use super::engine::SearchEngine;
use super::types::{ControversyInfo, SearchError};

impl SearchEngine {
    /// Cognitive layer (#33): distribution of stances toward a shared memory
    /// ("3 confirm, 1 disputes"). One query returns both the knowers and the
    /// attributed HAS_MEMORY edges.
    pub(super) async fn fetch_memory_stances_static(
        client: &HelixClient,
        memory_id: &str,
    ) -> Result<std::collections::HashMap<String, u32>, SearchError> {
        #[derive(serde::Deserialize)]
        struct StanceEdge {
            #[serde(default)]
            stance: Option<String>,
        }
        #[derive(serde::Deserialize)]
        struct StancesResult {
            #[serde(default)]
            stance_edges: Vec<StanceEdge>,
        }

        let result: StancesResult = client
            .execute_query(
                "getMemoryStances",
                &serde_json::json!({"memory_id": memory_id}),
            )
            .await
            .map_err(|e| SearchError::InvalidMode(e.to_string()))?;

        let mut distribution: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        for edge in result.stance_edges {
            // Edges created before the cognitive layer carry no stance —
            // count them as legacy "knows".
            let stance = edge
                .stance
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "knows".to_string());
            *distribution.entry(stance).or_insert(0) += 1;
        }
        Ok(distribution)
    }

    /// Collective consensus count (#43): the number of distinct holders across
    /// the whole fingerprint group, not just this one personal node. Each user
    /// keeps their own node; identical facts share a `content_key`, so we resolve
    /// the node's fingerprint and count the group. Legacy nodes (empty
    /// content_key) fall back to the per-node holder count.
    pub(super) async fn fetch_memory_user_count_static(
        client: &HelixClient,
        memory_id: &str,
    ) -> Result<u32, SearchError> {
        #[derive(serde::Deserialize)]
        struct MemNode {
            #[serde(default)]
            content_key: String,
        }
        #[derive(serde::Deserialize)]
        struct MemResult {
            #[serde(default)]
            memory: Option<MemNode>,
        }
        let content_key = client
            .execute_query::<MemResult, _>(
                "getMemory",
                &serde_json::json!({"memory_id": memory_id}),
            )
            .await
            .ok()
            .and_then(|r| r.memory)
            .map(|m| m.content_key)
            .unwrap_or_default();

        if !content_key.is_empty() {
            #[derive(serde::Deserialize)]
            struct CountResult {
                #[serde(default)]
                count: i64,
            }
            if let Ok(r) = client
                .execute_query::<CountResult, _>(
                    "getContentKeyGroupUserCount",
                    &serde_json::json!({"content_key": content_key}),
                )
                .await
            {
                return Ok((r.count.max(1)) as u32);
            }
        }

        // Fallback for legacy/empty content_key: holders of this node only.
        #[derive(serde::Deserialize)]
        struct UsersResult {
            #[serde(default)]
            users: Vec<serde_json::Value>,
        }
        let result: UsersResult = client
            .execute_query(
                "getMemoryUsers",
                &serde_json::json!({"memory_id": memory_id}),
            )
            .await
            .map_err(|e| SearchError::InvalidMode(e.to_string()))?;
        Ok(result.users.len().max(1) as u32)
    }

    pub(super) async fn fetch_controversy_static(
        client: &HelixClient,
        memory_id: &str,
        current_user_id: &str,
    ) -> Result<Option<ControversyInfo>, SearchError> {
        #[derive(serde::Deserialize)]
        struct ContradictionsResult {
            #[serde(default)]
            contradicts_out: Vec<ContradictedMemory>,
            #[serde(default)]
            contradicts_in: Vec<ContradictedMemory>,
        }

        #[derive(serde::Deserialize)]
        struct ContradictedMemory {
            #[serde(default)]
            memory_id: String,
            #[serde(default)]
            content: String,
            #[serde(default)]
            user_id: String,
        }

        let result: ContradictionsResult = client
            .execute_query(
                "getMemoryContradictions",
                &serde_json::json!({"memory_id": memory_id}),
            )
            .await
            .map_err(|e| SearchError::InvalidMode(e.to_string()))?;

        let all_contradictions: Vec<&ContradictedMemory> = result
            .contradicts_out
            .iter()
            .chain(result.contradicts_in.iter())
            .filter(|m| !m.memory_id.is_empty() && m.user_id != current_user_id)
            .collect();

        if let Some(conflict) = all_contradictions.first() {
            Ok(Some(ControversyInfo {
                conflicting_memory_id: conflict.memory_id.clone(),
                conflicting_content: conflict.content.clone(),
                conflicting_user_id: conflict.user_id.clone(),
                conflict_type: "preference_conflict".to_string(),
            }))
        } else {
            Ok(None)
        }
    }
}
