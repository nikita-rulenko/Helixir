//! Swarm rendezvous (#39) — agent presence in the shared graph.
//!
//! The collective coordinates through ONE HelixDB, never CLI-to-CLI: every agent
//! on any host writes a heartbeat here, so any other agent reads the roster and
//! sees who is live. This is the data-plane rendezvous the multi-host topology
//! rests on — the per-host daemon/gateway (#42) layers on top, it does not
//! replace this.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use serde::Deserialize;

use super::ToolingManager;
use super::types::ToolingError;
use crate::utils::nullable_string;

const PRESENCE_CLAIM_LOCK_STRIPES: usize = 64;

fn presence_claim_lock(agent_id: &str) -> &'static tokio::sync::Mutex<()> {
    static LOCKS: OnceLock<Box<[tokio::sync::Mutex<()>]>> = OnceLock::new();
    let locks = LOCKS.get_or_init(|| {
        (0..PRESENCE_CLAIM_LOCK_STRIPES)
            .map(|_| tokio::sync::Mutex::new(()))
            .collect::<Vec<_>>()
            .into_boxed_slice()
    });
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    agent_id.hash(&mut hasher);
    &locks[(hasher.finish() as usize) % locks.len()]
}

/// One agent's presence as recorded in the shared graph.
#[derive(Debug, Clone)]
pub struct AgentPresence {
    pub agent_id: String,
    /// Stable RBAC principal that owns this execution instance.
    ///
    /// Old rows may not have this field; callers must then treat `agent_id`
    /// as the conservative fallback instead of inventing an authorization
    /// relationship from naming conventions.
    pub principal_id: String,
    pub name: String,
    pub role: String,
    pub host: String,
    pub last_seen: String,
    pub status: String,
}

/// One logical agent family and its concurrently active execution instances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFamilyPresence {
    pub principal_id: String,
    pub instance_ids: Vec<String>,
    pub active_instances: usize,
    pub total_instances: usize,
    pub active: bool,
}

impl AgentPresence {
    /// Seconds since `last_seen`, or `None` if it was never stamped / unparseable.
    pub fn age_seconds(&self, now: chrono::DateTime<chrono::Utc>) -> Option<i64> {
        let seen = chrono::DateTime::parse_from_rfc3339(self.last_seen.trim()).ok()?;
        Some((now - seen.with_timezone(&chrono::Utc)).num_seconds())
    }

    /// Live when the agent has not explicitly left and its last heartbeat is
    /// within `window` seconds (and not in the future). Explicit terminal
    /// states win immediately; the time window is only the crash fallback.
    pub fn is_active(&self, now: chrono::DateTime<chrono::Utc>, window: i64) -> bool {
        status_allows_activity(&self.status)
            && matches!(self.age_seconds(now), Some(age) if (0..=window).contains(&age))
    }
}

/// Whether a stored presence status represents a process that still claims
/// to be online. Unknown non-terminal statuses remain active for compatibility
/// with descriptive worker states such as `testing` or `reviewing`.
pub fn status_allows_activity(status: &str) -> bool {
    !matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "done" | "failed" | "offline" | "stopped" | "disconnected" | "farewell"
    )
}

/// The response from `getAgent`/`listAgents` may nest the node under its RETURN
/// name or hand it back directly — dig for a non-empty `agent_id` either way.
fn has_agent_id(v: &serde_json::Value) -> bool {
    let node = v.get("agent").unwrap_or(v);
    node.get("agent_id")
        .and_then(serde_json::Value::as_str)
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

fn stored_principal_id(v: &serde_json::Value) -> Option<&str> {
    let node = v.get("agent").unwrap_or(v);
    node.get("principal_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn is_missing_agent_lookup(error: &str) -> bool {
    error.to_ascii_lowercase().contains("no value found")
}

/// Collapse execution instances into their explicitly recorded logical
/// principals. This is a presentation/counting operation only; authorization
/// continues to use the graph-backed RBAC principal itself.
pub fn aggregate_agent_families(
    agents: &[AgentPresence],
    now: chrono::DateTime<chrono::Utc>,
    window: i64,
) -> Vec<AgentFamilyPresence> {
    let mut families = BTreeMap::<String, AgentFamilyPresence>::new();
    for agent in agents {
        let principal_id = if agent.principal_id.trim().is_empty() {
            agent.agent_id.clone()
        } else {
            agent.principal_id.clone()
        };
        let family = families
            .entry(principal_id.clone())
            .or_insert_with(|| AgentFamilyPresence {
                principal_id,
                instance_ids: Vec::new(),
                active_instances: 0,
                total_instances: 0,
                active: false,
            });
        family.instance_ids.push(agent.agent_id.clone());
        family.total_instances += 1;
        if agent.is_active(now, window) {
            family.active_instances += 1;
            family.active = true;
        }
    }
    let mut families = families.into_values().collect::<Vec<_>>();
    families.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then_with(|| left.principal_id.cmp(&right.principal_id))
    });
    families
}

/// Fill the display-only family identity for historical presence rows whose
/// schema predates `Agent.principal_id`.
///
/// Explicit identities are preserved. Prefix inference is restricted to this
/// projection step and therefore cannot grant access or mutate graph state.
pub fn normalize_legacy_agent_principals(
    agents: &mut [AgentPresence],
    known_principals: &BTreeSet<String>,
) {
    for agent in agents {
        agent.principal_id = crate::utils::resolve_agent_principal_for_display(
            &agent.principal_id,
            &agent.agent_id,
            known_principals,
        );
    }
}

impl ToolingManager {
    /// Register the agent if new, then stamp its presence (host, last_seen, status).
    /// Idempotent — safe to call on every daemon pass or session start.
    pub async fn register_or_heartbeat(
        &self,
        agent_id: &str,
        role: &str,
        host: &str,
        status: &str,
    ) -> Result<(), ToolingError> {
        let _claim = presence_claim_lock(agent_id).lock().await;
        let existing = self.existing_agent(agent_id).await?;
        let principal_id = existing
            .as_ref()
            .and_then(stored_principal_id)
            .unwrap_or(agent_id);
        self.register_or_heartbeat_inner(
            agent_id,
            principal_id,
            role,
            host,
            status,
            existing.as_ref(),
        )
        .await
    }

    /// Register or refresh one execution instance owned by a stable logical
    /// RBAC principal. New code should prefer this variant; the compatibility
    /// method above preserves an existing association and otherwise treats the
    /// instance as its own principal.
    pub async fn register_or_heartbeat_as(
        &self,
        agent_id: &str,
        principal_id: &str,
        role: &str,
        host: &str,
        status: &str,
    ) -> Result<(), ToolingError> {
        let principal_id = principal_id.trim();
        if principal_id.is_empty() {
            return Err(ToolingError::Memory(
                "agent principal_id must not be empty".to_string(),
            ));
        }
        let _claim = presence_claim_lock(agent_id).lock().await;
        let existing = self.existing_agent(agent_id).await?;
        if let Some(stored) = existing.as_ref().and_then(stored_principal_id)
            && stored != principal_id
        {
            return Err(ToolingError::Memory(format!(
                "agent instance '{agent_id}' already belongs to principal '{stored}'"
            )));
        }
        self.register_or_heartbeat_inner(
            agent_id,
            principal_id,
            role,
            host,
            status,
            existing.as_ref(),
        )
        .await
    }

    /// Mark an existing execution instance terminal without creating a new
    /// registry row when the supplied id has never announced presence.
    /// Repeated farewells for a known instance remain idempotent.
    pub async fn farewell_existing(
        &self,
        agent_id: &str,
        principal_id: &str,
        role: &str,
        host: &str,
    ) -> Result<bool, ToolingError> {
        let _claim = presence_claim_lock(agent_id).lock().await;
        let Some(existing) = self.existing_agent(agent_id).await? else {
            return Ok(false);
        };
        if !has_agent_id(&existing) {
            return Ok(false);
        }
        if let Some(stored) = stored_principal_id(&existing) {
            if stored != principal_id {
                return Err(ToolingError::Memory(format!(
                    "agent instance '{agent_id}' belongs to principal '{stored}', not '{principal_id}'"
                )));
            }
        } else if agent_id != principal_id {
            return Err(ToolingError::Memory(format!(
                "legacy agent instance '{agent_id}' has no explicit owner and cannot be terminated by '{principal_id}'"
            )));
        }
        self.register_or_heartbeat_inner(
            agent_id,
            principal_id,
            role,
            host,
            "done",
            Some(&existing),
        )
        .await?;
        Ok(true)
    }

    async fn existing_agent(
        &self,
        agent_id: &str,
    ) -> Result<Option<serde_json::Value>, ToolingError> {
        match self
            .db
            .execute_query::<serde_json::Value, _>(
                "getAgent",
                &serde_json::json!({ "agent_id": agent_id }),
            )
            .await
        {
            Ok(existing) => Ok(Some(existing)),
            Err(error) if is_missing_agent_lookup(&error.to_string()) => Ok(None),
            Err(error) => Err(ToolingError::Database(error.to_string())),
        }
    }

    async fn register_or_heartbeat_inner(
        &self,
        agent_id: &str,
        principal_id: &str,
        role: &str,
        host: &str,
        status: &str,
        existing: Option<&serde_json::Value>,
    ) -> Result<(), ToolingError> {
        let now = chrono::Utc::now().to_rfc3339();

        // Create the Agent node only if absent — guard with getAgent so a
        // re-register never duplicates (mirrors the getUser→addUser pattern).
        let exists = existing.map(has_agent_id).unwrap_or(false);
        if !exists {
            self.db
                .execute_query::<serde_json::Value, _>(
                    "addAgent",
                    &serde_json::json!({
                        "agent_id": agent_id,
                        "principal_id": principal_id,
                        "name": agent_id,
                        "role": role,
                        "capabilities": "",
                        "agent_version": env!("CARGO_PKG_VERSION"),
                        "created_at": now,
                    }),
                )
                .await
                .map_err(|e| ToolingError::Database(e.to_string()))?;
        }

        self.db
            .execute_query::<serde_json::Value, _>(
                "heartbeatAgent",
                &serde_json::json!({
                    "agent_id": agent_id,
                    "principal_id": principal_id,
                    "host": host,
                    "last_seen": now,
                    "status": status,
                }),
            )
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        Ok(())
    }

    /// The full roster — every agent the collective knows, with presence. Active
    /// vs stale is decided by the caller against a time window.
    pub async fn list_swarm(&self) -> Result<Vec<AgentPresence>, ToolingError> {
        #[derive(Deserialize, Default)]
        struct Resp {
            #[serde(default)]
            agents: Vec<Row>,
        }
        #[derive(Deserialize)]
        struct Row {
            #[serde(default, deserialize_with = "nullable_string")]
            agent_id: String,
            #[serde(default, deserialize_with = "nullable_string")]
            principal_id: String,
            #[serde(default, deserialize_with = "nullable_string")]
            name: String,
            #[serde(default, deserialize_with = "nullable_string")]
            role: String,
            #[serde(default, deserialize_with = "nullable_string")]
            host: String,
            #[serde(default, deserialize_with = "nullable_string")]
            last_seen: String,
            #[serde(default, deserialize_with = "nullable_string")]
            status: String,
        }
        let resp: Resp = self
            .db
            .execute_query("listAgents", &serde_json::json!({}))
            .await
            .map_err(|e| ToolingError::Database(e.to_string()))?;
        Ok(resp
            .agents
            .into_iter()
            .filter(|r| !r.agent_id.is_empty())
            .map(|r| AgentPresence {
                agent_id: r.agent_id,
                principal_id: r.principal_id,
                name: r.name,
                role: r.role,
                host: r.host,
                last_seen: r.last_seen,
                status: r.status,
            })
            .collect())
    }
}

#[cfg(test)]
#[path = "swarm_tests.rs"]
mod tests;
