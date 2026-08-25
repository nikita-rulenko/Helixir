//! Memory ↔ context linking on the add path. Creates the `Context` node on
//! miss (so callers can use `context_tag` without first owning context
//! lifecycle), then writes a `VALID_IN` edge with priority 50.

use serde::{Deserialize, Serialize};
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

            #[derive(Deserialize)]
            struct ContextLookup {
                context: ContextId,
            }

            #[derive(Deserialize)]
            struct ContextId {
                context_id: String,
            }

            match self
                .db
                .execute_query::<ContextLookup, _>(
                    "getContextByName",
                    &GetByNameParams {
                        name: context_name.to_string(),
                    },
                )
                .await
            {
                Ok(existing) if !existing.context.context_id.is_empty() => {
                    Some(existing.context.context_id)
                }
                Ok(_) => {
                    return Err(ToolingError::Database(
                        "getContextByName returned an empty context_id".to_string(),
                    ));
                }
                Err(error) if error.is_graph_not_found() => None,
                Err(error) => return Err(ToolingError::Database(error.to_string())),
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

                self.db
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
                    .await
                    .map_err(|error| ToolingError::Database(error.to_string()))?;

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
                    priority: self.config.write.context_link_priority,
                    exclusive: 0,
                },
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;

        debug!("Linked memory {} to context '{}'", memory_id, context_name);
        Ok(())
    }
}
