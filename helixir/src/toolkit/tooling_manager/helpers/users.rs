//! User lifecycle: ensure the `User` node exists, plus Hive cross-user linking.
//!
//! `link_user_to_existing_memory` is reserved for the Hive cross-user
//! linking path which is currently disabled. Kept until the feature lands.

use serde::Serialize;
use tracing::{debug, warn};

use super::super::ToolingManager;

impl ToolingManager {
    pub(crate) async fn ensure_user_exists(&self, user_id: &str) {
        #[derive(serde::Deserialize)]
        struct UserResponse {
            #[serde(default)]
            user: Option<serde_json::Value>,
        }

        let exists = self
            .db
            .execute_query::<UserResponse, _>("getUser", &serde_json::json!({"user_id": user_id}))
            .await
            .map(|r| r.user.is_some())
            .unwrap_or(false);

        if !exists {
            let _ = self
                .db
                .execute_query::<serde_json::Value, _>(
                    "addUser",
                    &serde_json::json!({"user_id": user_id, "name": user_id}),
                )
                .await;
            debug!("Created user node: {}", user_id);
        }
    }

    // Reserved for Hive cross-user linking; not invoked yet — guarded behind
    // the (currently disabled) `cross_user_dedup` path. Will land with the
    // Hive-feature ticket; remove `#[allow]` then.
    #[allow(dead_code)]
    pub(crate) async fn link_user_to_existing_memory(&self, user_id: &str, memory_id: &str) {
        self.ensure_user_exists(user_id).await;

        #[derive(Serialize)]
        struct LinkInput {
            user_id: String,
            memory_id: String,
            context: String,
        }

        if let Err(e) = self
            .db
            .execute_query::<serde_json::Value, _>(
                "linkUserToMemory",
                &LinkInput {
                    user_id: user_id.to_string(),
                    memory_id: memory_id.to_string(),
                    context: "cross_user_link".to_string(),
                },
            )
            .await
        {
            warn!(
                "Failed to cross-link user {} to memory {}: {}",
                user_id, memory_id, e
            );
            return;
        }

        #[derive(serde::Deserialize)]
        #[allow(dead_code)] // HelixDB response envelope.
        struct UsersResult {
            #[serde(default)]
            users: Vec<serde_json::Value>,
        }
        let user_count = match self
            .db
            .execute_query::<UsersResult, _>(
                "getMemoryUsers",
                &serde_json::json!({"memory_id": memory_id}),
            )
            .await
        {
            Ok(r) => r.users.len().max(1) as i64,
            Err(_) => 2,
        };

        #[derive(Serialize)]
        struct UpdateCountInput {
            memory_id: String,
            user_count: i64,
            updated_at: String,
        }
        let _ = self
            .db
            .execute_query::<serde_json::Value, _>(
                "updateMemoryUserCount",
                &UpdateCountInput {
                    memory_id: memory_id.to_string(),
                    user_count,
                    updated_at: chrono::Utc::now().to_rfc3339(),
                },
            )
            .await;

        debug!(
            "Cross-linked user {} to memory {} (user_count={})",
            user_id, memory_id, user_count
        );
    }
}
