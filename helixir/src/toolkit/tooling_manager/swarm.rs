//! Swarm rendezvous (#39) — agent presence in the shared graph.
//!
//! The collective coordinates through ONE HelixDB, never CLI-to-CLI: every agent
//! on any host writes a heartbeat here, so any other agent reads the roster and
//! sees who is live. This is the data-plane rendezvous the multi-host topology
//! rests on — the per-host daemon/gateway (#42) layers on top, it does not
//! replace this.

use serde::Deserialize;

use super::ToolingManager;
use super::types::ToolingError;
use crate::utils::nullable_string;

/// One agent's presence as recorded in the shared graph.
#[derive(Debug, Clone)]
pub struct AgentPresence {
    pub agent_id: String,
    pub name: String,
    pub role: String,
    pub host: String,
    pub last_seen: String,
    pub status: String,
}

impl AgentPresence {
    /// Seconds since `last_seen`, or `None` if it was never stamped / unparseable.
    pub fn age_seconds(&self, now: chrono::DateTime<chrono::Utc>) -> Option<i64> {
        let seen = chrono::DateTime::parse_from_rfc3339(self.last_seen.trim()).ok()?;
        Some((now - seen.with_timezone(&chrono::Utc)).num_seconds())
    }

    /// Live if the last heartbeat is within `window` seconds (and not in the future).
    pub fn is_active(&self, now: chrono::DateTime<chrono::Utc>, window: i64) -> bool {
        matches!(self.age_seconds(now), Some(age) if (0..=window).contains(&age))
    }
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
        let now = chrono::Utc::now().to_rfc3339();

        // Create the Agent node only if absent — guard with getAgent so a
        // re-register never duplicates (mirrors the getUser→addUser pattern).
        let existing = self
            .db
            .execute_query::<serde_json::Value, _>(
                "getAgent",
                &serde_json::json!({ "agent_id": agent_id }),
            )
            .await
            .ok();
        let exists = existing.as_ref().map(has_agent_id).unwrap_or(false);
        if !exists {
            self.db
                .execute_query::<serde_json::Value, _>(
                    "addAgent",
                    &serde_json::json!({
                        "agent_id": agent_id,
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
mod tests {
    use super::*;

    fn at(ts: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(ts)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    fn presence(last_seen: &str) -> AgentPresence {
        AgentPresence {
            agent_id: "a".into(),
            name: "a".into(),
            role: "developer".into(),
            host: "h".into(),
            last_seen: last_seen.into(),
            status: "idle".into(),
        }
    }

    #[test]
    fn active_within_window_stale_outside() {
        let now = at("2026-06-16T12:00:00Z");
        let p = presence("2026-06-16T11:59:30Z"); // 30s ago
        assert_eq!(p.age_seconds(now), Some(30));
        assert!(p.is_active(now, 90));
        assert!(!p.is_active(now, 10));
    }

    #[test]
    fn never_seen_is_not_active() {
        let now = at("2026-06-16T12:00:00Z");
        let p = presence("");
        assert_eq!(p.age_seconds(now), None);
        assert!(!p.is_active(now, 90));
    }

    #[test]
    fn future_heartbeat_is_not_active() {
        let now = at("2026-06-16T12:00:00Z");
        let p = presence("2026-06-16T12:05:00Z"); // clock skew, 5m ahead
        assert!(!p.is_active(now, 90));
    }

    #[test]
    fn has_agent_id_handles_both_shapes() {
        assert!(has_agent_id(&serde_json::json!({"agent": {"agent_id": "x"}})));
        assert!(has_agent_id(&serde_json::json!({"agent_id": "x"})));
        assert!(!has_agent_id(&serde_json::json!({"agent": {"agent_id": ""}})));
        assert!(!has_agent_id(&serde_json::json!({"agent": null})));
        assert!(!has_agent_id(&serde_json::json!({})));
    }
}
