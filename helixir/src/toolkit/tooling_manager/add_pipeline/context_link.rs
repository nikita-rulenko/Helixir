//! Memory ↔ context linking on the add path. Creates the `Context` node on
//! miss (so callers can use `context_tag` without first owning context
//! lifecycle), then writes a `VALID_IN` edge with priority 50.

use serde::Serialize;
use tracing::debug;

use super::super::{ToolingError, ToolingManager};

impl ToolingManager {
    pub(super) async fn link_memory_to_extracted_context(
        &self,
        memory_id: &str,
        context_tag: &str,
    ) -> Result<(), ToolingError> {
        let context_name = context_tag.trim();
        if context_name.is_empty() {
            return Ok(());
        }

        let context_type = if context_name.contains(':') {
            context_name
                .split(':')
                .next()
                .unwrap_or("general")
                .to_string()
        } else {
            "general".to_string()
        };

        let context_id = {
            #[derive(Serialize)]
            struct GetByNameParams {
                name: String,
            }

            let existing: Option<serde_json::Value> = self
                .db
                .execute_query(
                    "getContextByName",
                    &GetByNameParams {
                        name: context_name.to_string(),
                    },
                )
                .await
                .ok();

            if let Some(ref val) = existing {
                val.get("context_id")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            } else {
                None
            }
        };

        let resolved_id = match context_id {
            Some(id) => id,
            None => {
                let new_id = format!(
                    "ctx_{}",
                    uuid::Uuid::new_v4()
                        .to_string()
                        .replace("-", "")
                        .chars()
                        .take(12)
                        .collect::<String>()
                );

                #[derive(Serialize)]
                struct AddContextParams {
                    context_id: String,
                    name: String,
                    context_type: String,
                    properties: String,
                    parent_context: String,
                }

                let _ = self
                    .db
                    .execute_query::<serde_json::Value, _>(
                        "addContext",
                        &AddContextParams {
                            context_id: new_id.clone(),
                            name: context_name.to_string(),
                            context_type,
                            properties: "{}".to_string(),
                            parent_context: "".to_string(),
                        },
                    )
                    .await;

                debug!("Created new context '{}' ({})", context_name, new_id);
                new_id
            }
        };

        #[derive(Serialize)]
        struct ValidInParams {
            memory_id: String,
            context_id: String,
            priority: i64,
            exclusive: i64,
        }

        self.db
            .execute_query::<serde_json::Value, _>(
                "addMemoryValidIn",
                &ValidInParams {
                    memory_id: memory_id.to_string(),
                    context_id: resolved_id.clone(),
                    priority: 50,
                    exclusive: 0,
                },
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        debug!("Linked memory {} to context '{}'", memory_id, context_name);
        Ok(())
    }
}
