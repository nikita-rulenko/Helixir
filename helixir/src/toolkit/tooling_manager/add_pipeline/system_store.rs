//! Persistence helpers for the admin-only Moirai memory layer.

use crate::llm::extractor::ExtractedMemory;

use super::super::{ToolingError, ToolingManager};

impl ToolingManager {
    /// Store a first-class Moirai artifact in the explicit admin-only system
    /// workspace and materialize its visibility edge before reporting success.
    pub(crate) async fn store_moirai_memory(
        &self,
        memory: &ExtractedMemory,
        vector: &[f32],
        context_tags: &str,
    ) -> Result<(String, usize), ToolingError> {
        let scope = format!("rbac:group:{}", crate::core::rbac_compat::MOIRAI_GROUP_ID);
        let stored = self
            .store_new_memory(memory, "helixir", vector, context_tags, Some(&scope))
            .await?;
        self.link_moirai_memory(&stored.0).await?;
        Ok(stored)
    }

    /// Idempotently repair/materialize the group edge for a Moirai memory.
    pub(crate) async fn link_moirai_memory(&self, memory_id: &str) -> Result<(), ToolingError> {
        crate::core::rbac::RbacManager::new(self.db.clone())
            .link_memory_to_group(
                memory_id,
                Some(crate::core::rbac_compat::MOIRAI_GROUP_ID),
                "helixir-moirai",
            )
            .await
            .map_err(|error| ToolingError::Database(error.to_string()))
    }

    /// Attach non-traversable provenance from an admin-only Moirai artifact to
    /// one source memory.
    pub(crate) async fn link_moirai_provenance(
        &self,
        insight_id: &str,
        witness_id: &str,
        source: &str,
    ) -> Result<(), ToolingError> {
        self.db
            .execute_query::<serde_json::Value, _>(
                "addMoiraiProvenance",
                &serde_json::json!({
                    "insight_id": insight_id,
                    "witness_id": witness_id,
                    "source": source,
                    "created_at": chrono::Utc::now().to_rfc3339(),
                }),
            )
            .await
            .map(|_| ())
            .map_err(|error| ToolingError::Database(error.to_string()))
    }
}
