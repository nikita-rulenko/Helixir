//! Projection helpers for graph-backed agent presence in the RBAC registry.

use std::collections::BTreeSet;

use serde::{Deserialize, Deserializer};

#[derive(Debug, Default, Deserialize)]
pub(super) struct AgentsResponse {
    #[serde(default)]
    pub(super) agents: Vec<AgentNode>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AgentNode {
    #[serde(default, deserialize_with = "nullable_string")]
    pub(super) agent_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub(super) principal_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub(super) status: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub(super) last_seen: String,
}

fn nullable_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Option::unwrap_or_default)
}

#[derive(Debug, Default)]
pub(super) struct PrincipalPresence {
    pub(super) status: String,
    pub(super) last_seen: String,
    pub(super) instances: usize,
    pub(super) subagents: usize,
}

/// Resolve historical rows for display only. New writes persist the explicit
/// principal and therefore never derive authorization identity from a prefix.
pub(super) fn resolve_presence_principal(
    explicit: &str,
    agent_id: &str,
    known_principals: &BTreeSet<String>,
) -> String {
    crate::utils::resolve_agent_principal_for_display(explicit, agent_id, known_principals)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::resolve_presence_principal;

    #[test]
    fn legacy_nullable_presence_fields_deserialize_as_empty_strings() {
        let response: super::AgentsResponse = serde_json::from_value(serde_json::json!({
            "agents": [{
                "agent_id": "legacy-worker",
                "principal_id": null,
                "status": null,
                "last_seen": null
            }]
        }))
        .expect("legacy nullable Agent rows must remain readable");

        let agent = &response.agents[0];
        assert_eq!(agent.agent_id, "legacy-worker");
        assert!(agent.principal_id.is_empty());
        assert!(agent.status.is_empty());
        assert!(agent.last_seen.is_empty());
    }

    #[test]
    fn explicit_family_wins_and_legacy_rows_use_longest_known_prefix() {
        let known = ["codex".to_string(), "codex-web".to_string()]
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert_eq!(
            resolve_presence_principal("claude", "codex-web-build", &known),
            "claude"
        );
        assert_eq!(
            resolve_presence_principal("", "codex-web-build", &known),
            "codex-web"
        );
    }
}
