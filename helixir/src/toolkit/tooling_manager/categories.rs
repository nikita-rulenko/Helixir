//! Category primitives (#33, Moira) — the capability layer the Clotho agent
//! composes over. These are the "hands": create/link/tag categories and match
//! a memory's text against the dictionary by embedding. The *policy* (which
//! dictionary, what threshold, ancestor propagation, charter escalation) lives
//! in the Clotho agent (`crate::agents::clotho`), not here.
//!
//! Categories are the THIRD DIMENSION over the flat reasoning graph: a memory's
//! membership in a category lets it bridge to distant memories that share it.
//! Tagging is `Memory -TAGGED_AS-> Category`; the bridge routes THROUGH the
//! shared Category node (no pairwise edges — that would flatten the dimension
//! and explode hubs).

use std::collections::HashSet;

use serde::Deserialize;

use super::ToolingManager;
use super::types::ToolingError;
use crate::utils::nullable_string;

impl ToolingManager {
    /// Canonical category_id for `name` (normalized trim+lowercase), creating the
    /// `Category` node if absent. Idempotent.
    ///
    /// Note: this does NOT persist a `CategoryEmbedding`. Clotho's auto-tagging
    /// matches via in-memory cosine over the dictionary (SearchV exposes no
    /// readable score — see `helixdb-hql-gotchas`), so a DB-side category vector
    /// index is unnecessary until the dictionary is large enough to need ANN.
    pub async fn ensure_category(
        &self,
        name: &str,
        kind: &str,
        description: &str,
    ) -> Result<String, ToolingError> {
        let norm = name.trim().to_lowercase();
        if let Some(id) = self.get_category_id(&norm).await? {
            return Ok(id);
        }

        let category_id = format!(
            "cat_{}",
            uuid::Uuid::new_v4()
                .to_string()
                .replace('-', "")
                .chars()
                .take(12)
                .collect::<String>()
        );
        let now = chrono::Utc::now().to_rfc3339();
        self.db
            .execute_query::<serde_json::Value, _>(
                "addCategory",
                &serde_json::json!({
                    "category_id": category_id,
                    "name": norm,
                    "kind": kind,
                    "description": description,
                    "created_at": now,
                }),
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        Ok(category_id)
    }

    /// Link `child` SUBCATEGORY_OF `parent` (both category_ids).
    pub async fn link_subcategory(
        &self,
        child_id: &str,
        parent_id: &str,
    ) -> Result<(), ToolingError> {
        self.db
            .execute_query::<serde_json::Value, _>(
                "linkSubcategory",
                &serde_json::json!({ "child_id": child_id, "parent_id": parent_id }),
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        Ok(())
    }

    /// Tag a memory with a category (`Memory -TAGGED_AS-> Category`).
    pub async fn tag_memory(
        &self,
        memory_id: &str,
        category_id: &str,
        confidence: i64,
        source: &str,
    ) -> Result<(), ToolingError> {
        self.db
            .execute_query::<serde_json::Value, _>(
                "tagMemoryWithCategory",
                &serde_json::json!({
                    "memory_id": memory_id,
                    "category_id": category_id,
                    "confidence": confidence,
                    "source": source,
                }),
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        Ok(())
    }

    /// Embed arbitrary text with the active model (cached). The primitive the
    /// agents use to compute similarity in-memory: SearchV exposes no readable
    /// score (see `helixdb-hql-gotchas`), so thresholded matching over a small
    /// controlled vocabulary is done in Rust, not in the DB.
    pub async fn embed_text(&self, text: &str) -> Result<Vec<f32>, ToolingError> {
        self.embedder
            .generate(text, true)
            .await
            .map_err(|e| ToolingError::Embedding(e.to_string()))
    }

    /// The member set of a category subset — every memory_id tagged with it.
    /// Deploy-free PMI input for Lachesis subset routing; `limit` is high so it
    /// counts the whole subset on today's corpora (a `CO_OCCURS`-edge cache
    /// replaces this fetch once the dictionary is large).
    pub async fn category_member_ids(
        &self,
        category_id: &str,
    ) -> Result<HashSet<String>, ToolingError> {
        #[derive(Deserialize, Default)]
        struct Resp {
            #[serde(default)]
            memories: Vec<Row>,
        }
        #[derive(Deserialize)]
        struct Row {
            #[serde(default, deserialize_with = "nullable_string")]
            memory_id: String,
        }
        let resp: Resp = self
            .db
            .execute_query(
                "getMemoriesByCategory",
                &serde_json::json!({
                    "category_id": category_id,
                    "exclude_memory_id": "",
                    "limit": 1_000_000,
                }),
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        Ok(resp
            .memories
            .into_iter()
            .map(|m| m.memory_id)
            .filter(|s| !s.is_empty())
            .collect())
    }

    /// Canonical id for a normalized category name, or None if absent.
    /// `getCategoryByName` uses `::FIRST`, which raises GRAPH_ERROR "No value
    /// found" when missing (same shape as #19) — mapped to None.
    pub(crate) async fn get_category_id(
        &self,
        normalized_name: &str,
    ) -> Result<Option<String>, ToolingError> {
        #[derive(Deserialize)]
        struct Resp {
            #[serde(default)]
            category: Option<CatRow>,
        }
        #[derive(Deserialize)]
        struct CatRow {
            #[serde(default, deserialize_with = "nullable_string")]
            category_id: String,
        }

        match self
            .db
            .execute_query::<Resp, _>(
                "getCategoryByName",
                &serde_json::json!({ "name": normalized_name }),
            )
            .await
        {
            Ok(r) => Ok(r
                .category
                .map(|c| c.category_id)
                .filter(|id| !id.is_empty())),
            Err(e) if e.to_string().to_lowercase().contains("no value found") => Ok(None),
            Err(e) => Err(ToolingError::Database(e.to_string())),
        }
    }
}
