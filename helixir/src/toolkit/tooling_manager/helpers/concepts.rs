//! Memory ↔ concept linking on the live add path.

use tracing::debug;

use super::super::{ToolingError, ToolingManager};

impl ToolingManager {
    pub(crate) async fn link_memory_to_concept(
        &self,
        memory_id: &str,
        concept_id: &str,
        confidence: i32,
    ) -> Result<(), ToolingError> {
        #[derive(serde::Deserialize)]
        #[allow(dead_code)] // HelixDB link ack envelope.
        struct LinkResponse {
            #[serde(default)]
            link: serde_json::Value,
        }

        self.db
            .execute_query::<LinkResponse, _>(
                "linkMemoryToInstanceOf",
                &serde_json::json!({
                    "memory_id": memory_id,
                    "concept_id": concept_id,
                    "confidence": confidence as i64,
                }),
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        debug!("Linked memory {} to concept {}", memory_id, concept_id);
        Ok(())
    }
}
