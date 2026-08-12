//! One-way repair of the global-admin-only Moirai memory workspace.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde::Deserialize;

use crate::llm::EmbeddingGenerator;

use super::rbac::RbacManager;
use super::rbac_compat::MOIRAI_GROUP_ID;

impl RbacManager {
    /// Move historical generated memories out of user workspaces and into the
    /// explicit Moirai domain. Every operation is idempotent and safe to retry.
    pub async fn repair_moirai_memory_layer(&self, actor: &str) -> Result<usize> {
        self.authorize_moirai_repair(actor).await?;
        let legacy_stitches = self.migrate_legacy_lachesis_stitches(actor).await?;
        let mut memories = BTreeMap::<String, (String, String)>::new();
        for (tag, memory_type) in [
            ("moira-insight", "opinion"),
            ("moira-stitch", "opinion"),
            ("insight-retired", "fact"),
        ] {
            let response: TaggedMemoriesResponse = self
                .db
                .execute_query(
                    "searchByContextTag",
                    &serde_json::json!({"tag": tag, "limit": 100_000i64}),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            for memory in response.memories {
                if !memory.memory_id.is_empty() {
                    memories.insert(memory.memory_id, (memory.content, memory_type.to_string()));
                }
            }
        }

        let scope = format!("rbac:group:{MOIRAI_GROUP_ID}");
        for (memory_id, (content, memory_type)) in &memories {
            self.migrate_legacy_provenance(memory_id).await?;
            let current_groups = self
                .memory_group_map(std::slice::from_ref(memory_id))
                .await?
                .remove(memory_id)
                .unwrap_or_default();
            let content_key =
                super::memory_fingerprint::content_key_scoped(content, memory_type, Some(&scope));
            self.db
                .execute_query::<serde_json::Value, _>(
                    "setMemorySecurityDomain",
                    &serde_json::json!({
                        "memory_id": memory_id,
                        "content_key": content_key,
                        "rbac_scope": scope,
                    }),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            for group_id in current_groups {
                if group_id != MOIRAI_GROUP_ID {
                    self.unlink_memory_from_group(memory_id, &group_id).await?;
                }
            }
            self.link_memory_to_group(memory_id, Some(MOIRAI_GROUP_ID), actor)
                .await?;
        }
        Ok(memories.len() + legacy_stitches)
    }

    async fn migrate_legacy_lachesis_stitches(&self, actor: &str) -> Result<usize> {
        let response: LegacyLachesisEffectsResponse = self
            .db
            .execute_query("getLegacyLachesisEffects", &serde_json::json!({}))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let mut migrated = 0usize;
        for effect in response.effects {
            if effect.memory_id.is_empty() {
                continue;
            }
            let causes: LegacyLachesisCausesResponse = self
                .db
                .execute_query(
                    "getLegacyLachesisCauses",
                    &serde_json::json!({"effect_id": effect.memory_id}),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            for cause in causes.causes {
                if cause.memory_id.is_empty() {
                    continue;
                }
                let embedder = self.embedder.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Moirai stitch migration requires the configured embedding provider"
                    )
                })?;
                let text = format!(
                    "HYPOTHESIS (generated, requires verification): '{}' may occur because '{}'. Migrated from a pre-v0.14.2 Lachesis stitch; this is not an asserted fact.",
                    crate::safe_truncate(&effect.content, 320),
                    crate::safe_truncate(&cause.content, 320),
                );
                let insight_id = self.ensure_legacy_stitch_memory(&text, embedder).await?;
                for witness_id in [&effect.memory_id, &cause.memory_id] {
                    self.db
                        .execute_query::<serde_json::Value, _>(
                            "addMoiraiProvenance",
                            &serde_json::json!({
                                "insight_id": insight_id,
                                "witness_id": witness_id,
                                "source": "lachesis-stitch-legacy",
                                "created_at": chrono::Utc::now().to_rfc3339(),
                            }),
                        )
                        .await
                        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                }
                self.link_memory_to_group(&insight_id, Some(MOIRAI_GROUP_ID), actor)
                    .await?;
                migrated += 1;
            }
            self.db
                .execute_query::<serde_json::Value, _>(
                    "dropLegacyLachesisStitches",
                    &serde_json::json!({"effect_id": effect.memory_id}),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        Ok(migrated)
    }

    async fn ensure_legacy_stitch_memory(
        &self,
        content: &str,
        embedder: &EmbeddingGenerator,
    ) -> Result<String> {
        let scope = format!("rbac:group:{MOIRAI_GROUP_ID}");
        let content_key =
            super::memory_fingerprint::content_key_scoped(content, "opinion", Some(&scope));
        let existing: TaggedMemoriesResponse = self
            .db
            .execute_query(
                "getMemoriesByContentKey",
                &serde_json::json!({"content_key": content_key}),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        if let Some(memory_id) = existing
            .memories
            .into_iter()
            .map(|memory| memory.memory_id)
            .find(|memory_id| !memory_id.is_empty())
        {
            self.ensure_memory_embedding(&memory_id, content, embedder)
                .await?;
            return Ok(memory_id);
        }

        let now = chrono::Utc::now().to_rfc3339();
        let memory_id = format!("mem_moirai_legacy_{}", &content_key[..16]);
        self.db
            .execute_query::<serde_json::Value, _>(
                "addMemoryKeyedScoped",
                &serde_json::json!({
                    "memory_id": memory_id,
                    "content_key": content_key,
                    "rbac_scope": scope,
                    "user_id": "helixir",
                    "content": content,
                    "memory_type": "opinion",
                    "certainty": 70i64,
                    "importance": 65i64,
                    "created_at": now,
                    "updated_at": now,
                    "valid_from": now,
                    "context_tags": "moira-stitch",
                    "source": "moirai_migration",
                    "metadata": "{\"legacy\":true}",
                }),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let user: UserResponse = self
            .db
            .execute_query("getUser", &serde_json::json!({"user_id": "helixir"}))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        if user.user.is_none() {
            self.db
                .execute_query::<serde_json::Value, _>(
                    "addUser",
                    &serde_json::json!({"user_id": "helixir", "name": "helixir"}),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        self.db
            .execute_query::<serde_json::Value, _>(
                "linkUserToMemoryWithStance",
                &serde_json::json!({
                    "user_id": "helixir",
                    "memory_id": memory_id,
                    "context": "moirai_migration",
                    "stance": "asserts",
                    "certainty": 70i64,
                    "linked_at": chrono::Utc::now().to_rfc3339(),
                }),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        self.ensure_memory_embedding(&memory_id, content, embedder)
            .await?;
        Ok(memory_id)
    }

    async fn ensure_memory_embedding(
        &self,
        memory_id: &str,
        content: &str,
        embedder: &EmbeddingGenerator,
    ) -> Result<()> {
        let response: MemoryResponse = self
            .db
            .execute_query("getMemory", &serde_json::json!({"memory_id": memory_id}))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let internal_id = response
            .memory
            .filter(|memory| !memory.id.is_empty())
            .map(|memory| memory.id)
            .ok_or_else(|| {
                anyhow::anyhow!("Moirai migration memory '{memory_id}' was not found")
            })?;
        let existing = self
            .db
            .execute_query::<serde_json::Value, _>(
                "getMemoryEmbedding",
                &serde_json::json!({"memory_id": internal_id}),
            )
            .await;
        if existing.as_ref().is_ok_and(|value| {
            value
                .get("embedding")
                .is_some_and(|embedding| !embedding.is_null())
        }) {
            return Ok(());
        }
        let vector = embedder.generate(content, true).await?;
        self.db
            .execute_query::<serde_json::Value, _>(
                "addMemoryEmbedding",
                &serde_json::json!({
                    "memory_id": internal_id,
                    "vector_data": vector.iter().map(|value| f64::from(*value)).collect::<Vec<_>>(),
                    "embedding_model": embedder.model(),
                    "created_at": chrono::Utc::now().to_rfc3339(),
                }),
            )
            .await
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    async fn migrate_legacy_provenance(&self, memory_id: &str) -> Result<()> {
        let legacy: MoiraiWitnessesResponse = self
            .db
            .execute_query(
                "getLegacyMoiraiWitnesses",
                &serde_json::json!({"insight_id": memory_id}),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        for witness in legacy.witnesses {
            if witness.memory_id.is_empty() {
                continue;
            }
            self.db
                .execute_query::<serde_json::Value, _>(
                    "addMoiraiProvenance",
                    &serde_json::json!({
                        "insight_id": memory_id,
                        "witness_id": witness.memory_id,
                        "source": "atropos-legacy",
                        "created_at": chrono::Utc::now().to_rfc3339(),
                    }),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        self.db
            .execute_query::<serde_json::Value, _>(
                "dropLegacyMoiraiSupports",
                &serde_json::json!({"insight_id": memory_id}),
            )
            .await
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    async fn authorize_moirai_repair(&self, actor: &str) -> Result<()> {
        let policy = self.snapshot().await?;
        if policy.enabled && !policy.is_admin(actor) {
            bail!("Moirai memory repair requires a global admin");
        }
        if !policy.groups.contains_key(MOIRAI_GROUP_ID) {
            bail!("Moirai memory repair requires the reserved '{MOIRAI_GROUP_ID}' group");
        }
        Ok(())
    }
}

#[derive(Debug, Default, Deserialize)]
struct TaggedMemoriesResponse {
    #[serde(default)]
    memories: Vec<TaggedMemory>,
}

#[derive(Debug, Default, Deserialize)]
struct TaggedMemory {
    #[serde(default)]
    memory_id: String,
    #[serde(default)]
    content: String,
}

#[derive(Debug, Default, Deserialize)]
struct MoiraiWitnessesResponse {
    #[serde(default)]
    witnesses: Vec<MoiraiWitness>,
}

#[derive(Debug, Default, Deserialize)]
struct MoiraiWitness {
    #[serde(default)]
    memory_id: String,
    #[serde(default)]
    content: String,
}

#[derive(Debug, Default, Deserialize)]
struct UserResponse {
    #[serde(default)]
    user: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct MemoryResponse {
    #[serde(default)]
    memory: Option<MemoryIdentity>,
}

#[derive(Debug, Default, Deserialize)]
struct MemoryIdentity {
    #[serde(default)]
    id: String,
}

#[derive(Debug, Default, Deserialize)]
struct LegacyLachesisEffectsResponse {
    #[serde(default)]
    effects: Vec<MoiraiWitness>,
}

#[derive(Debug, Default, Deserialize)]
struct LegacyLachesisCausesResponse {
    #[serde(default)]
    causes: Vec<MoiraiWitness>,
}
