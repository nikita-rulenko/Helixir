use helix_rs::HelixDB;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, warn};

#[derive(Error, Debug)]
pub enum UserLinkError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Link failed: {0}")]
    LinkFailed(String),
}

impl From<helix_rs::HelixError> for UserLinkError {
    fn from(err: helix_rs::HelixError) -> Self {
        UserLinkError::Database(err.to_string())
    }
}

#[derive(Serialize, Deserialize)]
struct UserQuery {
    user_id: String,
}

#[derive(Serialize, Deserialize)]
struct UserCreate {
    user_id: String,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct MemoryLink {
    user_id: String,
    memory_id: String,
    context: String,
}

pub struct UserLinker {
    db: HelixDB,
}

impl UserLinker {
    pub fn new(db: HelixDB) -> Self {
        Self { db }
    }

    pub async fn ensure_user_exists(&self, user_id: &str) -> Result<bool, UserLinkError> {
        match self.db.query("getUser", serde_json::json!({"user_id": user_id})).await {
            Ok(_) => Ok(false),
            Err(_) => {
                debug!("Creating user {}", user_id);
                self.db.query("addUser", serde_json::json!({
                    "user_id": user_id,
                    "name": format!("User {}", user_id)
                })).await?;
                Ok(true)
            }
        }
    }

    pub async fn link_memory_to_user(
        &self,
        user_id: &str,
        memory_id: &str,
        context: &str,
    ) -> Result<(), UserLinkError> {
        debug!("Linking memory {} to user {}", memory_id, user_id);
        self.db.query("linkUserToMemory", serde_json::json!({
            "user_id": user_id,
            "memory_id": memory_id,
            "context": context
        })).await.map_err(|e| UserLinkError::LinkFailed(e.to_string()))?;
        Ok(())
    }
}