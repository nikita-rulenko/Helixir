//! Collective-scope enrichment: `user_count` and `ControversyInfo` lookups
//! that run in parallel for `scope=collective|all` searches.
//!
//! Used by [`super::dispatch::SearchEngine::search`]; kept as static methods
//! so they can be spawned without a `Self` borrow.

use crate::db::HelixClient;

use super::engine::SearchEngine;
use super::types::{ControversyInfo, SearchError};

impl SearchEngine {
    pub(super) async fn fetch_memory_user_count_static(
        client: &HelixClient,
        memory_id: &str,
    ) -> Result<u32, SearchError> {
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
