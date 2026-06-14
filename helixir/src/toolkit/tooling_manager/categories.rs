//! Clotho category dictionary operations (#33, Moira).
//!
//! Categories are the THIRD DIMENSION over the flat reasoning graph: a memory's
//! membership in a category lets it bridge to distant memories that share it,
//! and each memory carries several categories (several jump-planes). Tagging is
//! `Memory -TAGGED_AS-> Category`; the bridge is realised by routing THROUGH the
//! shared Category node (no pairwise edges — that would flatten the dimension
//! and explode hubs). Bridge strength ∝ category specificity (a "thin" axis is a
//! strong bridge; a "thick" one like raw-material is weak) — the future Lachesis
//! gate.

use serde::Deserialize;
use tracing::warn;

use super::ToolingManager;
use super::types::ToolingError;
use crate::utils::nullable_string;

impl ToolingManager {
    /// Canonical category_id for `name` (normalized trim+lowercase), creating it
    /// — and its embedding for later embedding-match tagging — if absent.
    /// Idempotent.
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

        // Embedding (name + description) for Clotho's embedding-match tagging.
        let embed_text = if description.trim().is_empty() {
            norm.clone()
        } else {
            format!("{norm}: {description}")
        };
        match self.embedder.generate(&embed_text, true).await {
            Ok(vec) => {
                let vector_data: Vec<f64> = vec.iter().map(|&x| x as f64).collect();
                if let Err(e) = self
                    .db
                    .execute_query::<serde_json::Value, _>(
                        "addCategoryEmbedding",
                        &serde_json::json!({
                            "category_id": category_id,
                            "vector_data": vector_data,
                            "content": norm,
                            "embedding_model": self.embedder.model(),
                        }),
                    )
                    .await
                {
                    warn!("ensure_category: embedding persist failed for {}: {}", norm, e);
                }
            }
            Err(e) => warn!("ensure_category: embed failed for {}: {}", norm, e),
        }

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

    /// Canonical id for a normalized category name, or None if absent.
    /// `getCategoryByName` uses `::FIRST`, which raises GRAPH_ERROR "No value
    /// found" when missing (same shape as #19) — mapped to None.
    async fn get_category_id(&self, normalized_name: &str) -> Result<Option<String>, ToolingError> {
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
