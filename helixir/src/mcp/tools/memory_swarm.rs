//! Swarm presence and contradiction-resolution tools.
//! Long-term memory MCP tools.
//!
//! Covers the user-visible memory verbs: add, search (semantic + concept +
//! reasoning chain), list, update, graph, and the helper that finds
//! previously-timed-out FastThink commits.

use std::collections::BTreeSet;

use rmcp::{
    ErrorData as McpError, handler::server::wrapper::Parameters, model::*, tool, tool_router,
};
use serde_json::json;
use tracing::info;

use crate::mcp::params::*;
use crate::mcp::server::HelixirMcpServer;

use super::memory_support::machine_hostname;

#[tool_router(router = memory_swarm_router, vis = "pub(super)")]
impl HelixirMcpServer {
    #[tool(
        description = "Register this remote agent principal through the reserved onboarding workspace. This deliberately narrow operation accepts only actor_id and can grant only worker in onboarding; it cannot choose another user, role, or group. Repeated calls are idempotent, and an existing default/onboarding assignment is preserved without downgrade. Returns {principal_id, group_id, roles, created}."
    )]
    async fn enroll_client(
        &self,
        Parameters(params): Parameters<EnrollClientParams>,
    ) -> Result<CallToolResult, McpError> {
        let enrollment = self
            .client()
            .rbac()
            .self_enroll_client(&params.actor_id)
            .await
            .map_err(|error| McpError::invalid_request(error.to_string(), None))?;
        let payload = Self::result_to_json(enrollment)?;
        Ok(CallToolResult::success(vec![Content::text(payload)]))
    }

    #[tool(
        description = "Announce or refresh one concrete execution instance without writing a memory. actor_id resolves the authenticated logical principal; agent_id identifies this sub-agent/process and is grouped under that principal. status is an optional non-terminal progress label (default 'working', max 64 characters). The operation is cheap, idempotent, changes no RBAC grants, and returns {available, principal_id, agent_id, status}. Call it when a sub-agent starts and at meaningful progress boundaries; call agent_farewell for the same agent_id when it finishes."
    )]
    async fn agent_heartbeat(
        &self,
        Parameters(params): Parameters<AgentHeartbeatParams>,
    ) -> Result<CallToolResult, McpError> {
        if !self.client().config().mode.collective_enabled() {
            return Ok(CallToolResult::success(vec![Content::text(
                json!({"available": false, "reason": "solo mode has no swarm"}).to_string(),
            )]));
        }
        let agent_id = validate_presence_value("agent_id", &params.agent_id, 128)?;
        let status = validate_heartbeat_status(params.status.as_deref().unwrap_or("working"))?;
        let actor_id = self
            .resolve_actor_id_without_presence(params.actor_id.as_deref(), agent_id)
            .await?;
        let role = self.client().config().swarm.default_role.clone();
        self.client()
            .tooling()
            .register_or_heartbeat_as(agent_id, &actor_id, &role, machine_hostname(), status)
            .await
            .map_err(|error| McpError::internal_error(error.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            json!({
                "available": true,
                "principal_id": actor_id,
                "agent_id": agent_id,
                "status": status,
            })
            .to_string(),
        )]))
    }

    #[tool(
        description = "Who is in the swarm RIGHT NOW — the agent rendezvous. `families` is the logical-agent registry grouped by explicitly stored RBAC principal; `subagents` contains only child execution instances where agent_id differs from principal_id; `agents` preserves the complete instance roster as a compatibility/diagnostic view. `active`/`total` and `active_principals`/`total_principals` count logical families, never raw processes. Separate instance and subagent counters expose concurrency. A family is online when its root or any child has a live non-terminal lease. agent_farewell ends only that instance; siblings keep the family live. Presence is published explicitly by agent_heartbeat and refreshed by add_memory(agent_id). GATED by the collective tier: Solo returns {available:false}."
    )]
    async fn swarm_status(
        &self,
        Parameters(params): Parameters<SwarmStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        if !self.client().config().mode.collective_enabled() {
            let payload = json!({
                "available": false,
                "agents": [],
                "families": [],
                "active_principals": 0,
                "active_instances": 0,
                "active_subagents": 0,
                "subagents": [],
                "note": "The swarm roster requires the collective tier; this Helixir runs in Solo mode (private memory). Set mode=Collective or Insights to join a swarm.",
            });
            return Ok(CallToolResult::success(vec![Content::text(
                payload.to_string(),
            )]));
        }

        let window = params
            .active_window_secs
            .unwrap_or(self.client().config().swarm.active_window_secs) as i64;
        let mut agents = self
            .client()
            .tooling()
            .list_swarm()
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let known_principals = self
            .client()
            .rbac()
            .snapshot()
            .await
            .map_err(|error| McpError::internal_error(error.to_string(), None))?
            .users
            .into_keys()
            .collect::<BTreeSet<_>>();
        crate::toolkit::tooling_manager::swarm::normalize_legacy_agent_principals(
            &mut agents,
            &known_principals,
        );

        let now = chrono::Utc::now();
        // #84: roster hygiene — an agent silent past the TTL is presumed gone
        // and HIDDEN from the roster (one-shot agents never say goodbye, so
        // their stored status lies "working" forever). The Agent NODE stays:
        // it anchors AGENT_CREATED provenance on every memory it wrote —
        // pruning the view must never orphan authorship.
        let ttl = self.client().config().swarm.presence_ttl_secs as i64;
        let total_known = agents.len();
        if ttl > 0 {
            agents.retain(|a| a.age_seconds(now).map(|s| s <= ttl).unwrap_or(false));
        }
        let hidden_stale = total_known - agents.len();
        // Live first, then most-recently-seen.
        agents.sort_by_key(|a| {
            let age = a.age_seconds(now).unwrap_or(i64::MAX);
            (!a.is_active(now, window), age)
        });
        let families =
            crate::toolkit::tooling_manager::swarm::aggregate_agent_families(&agents, now, window);
        let roster: Vec<serde_json::Value> = agents
            .iter()
            .map(|a| {
                {
                    // #84: derived honesty — a stored 'working' from an agent
                    // silent past the active window is a lie by omission; the
                    // roster says so instead of repeating it.
                    let active = a.is_active(now, window);
                    let derived = if active {
                        a.status.clone()
                    } else if a.status == "working" {
                        "stale (last reported: working)".to_string()
                    } else {
                        a.status.clone()
                    };
                    json!({
                        "agent_id": a.agent_id,
                        "principal_id": if a.principal_id.is_empty() { &a.agent_id } else { &a.principal_id },
                        "role": a.role,
                        "host": a.host,
                        "status": a.status,
                        "derived_status": derived,
                        "age_seconds": a.age_seconds(now),
                        "active": active,
                    })
                }
            })
            .collect();
        let active_instances = roster
            .iter()
            .filter(|a| a["active"].as_bool() == Some(true))
            .count();
        let active_principals = families.iter().filter(|family| family.active).count();
        let subagent_roster = roster
            .iter()
            .filter(|instance| instance["agent_id"] != instance["principal_id"])
            .cloned()
            .collect::<Vec<_>>();
        let active_subagents = subagent_roster
            .iter()
            .filter(|instance| instance["active"].as_bool() == Some(true))
            .count();
        let family_roster = families
            .iter()
            .map(|family| {
                json!({
                    "principal_id": family.principal_id,
                    "active": family.active,
                    "active_instances": family.active_instances,
                    "total_instances": family.total_instances,
                    "instance_ids": family.instance_ids,
                })
            })
            .collect::<Vec<_>>();
        info!(
            "Swarm roster: {} logical principals ({} active), {} instances ({} active)",
            families.len(),
            active_principals,
            roster.len(),
            active_instances
        );
        let payload = json!({
            "available": true,
            "active_window_secs": window,
            "presence_ttl_secs": ttl,
            "active": active_principals,
            "total": families.len(),
            "active_principals": active_principals,
            "total_principals": families.len(),
            "active_instances": active_instances,
            "total_instances": roster.len(),
            "active_subagents": active_subagents,
            "total_subagents": subagent_roster.len(),
            "hidden_stale": hidden_stale,
            "agents": roster,
            "families": family_roster,
            "subagents": subagent_roster,
        });
        Ok(CallToolResult::success(vec![Content::text(
            payload.to_string(),
        )]))
    }

    #[tool(
        description = "Answer a contradiction_review notice: settle a dispute between two memories. Pass the notice's from_id/to_id and your verdict — 'confirm' (my memory stands; both records stay, dispute retired), 'retract' (my memory is outdated; the disputing memory SUPERSEDES it — history preserved, nothing deleted), or 'preference' (both are valid viewpoints; they coexist). Non-destructive in every branch. Once resolved the dispute stops re-surfacing in reconcile passes. Every verdict is recorded as a charter PRECEDENT; after enough identical verdicts the result carries a 'rule_proposal' — a ready-to-adopt standing rule (adopt it verbatim via the add_memory call it dictates, or surface it to your human; adopted rules appear in memory://rules and silence further questions of that shape). Returns {resolved, from_id, to_id, strategy, rule_proposal?}."
    )]
    async fn resolve_contradiction(
        &self,
        Parameters(params): Parameters<ResolveContradictionParams>,
    ) -> Result<CallToolResult, McpError> {
        let verdict = params.resolution.trim().to_ascii_lowercase();
        let strategy = match verdict.as_str() {
            "confirm" | "confirmed" => "owner_confirmed",
            "retract" | "retracted" => "owner_retracted",
            "preference" | "coexist" => "coexist_preference",
            other => {
                return Err(McpError::invalid_params(
                    format!(
                        "resolution must be 'confirm', 'retract' or 'preference' (got '{other}')"
                    ),
                    None,
                ));
            }
        };
        info!(
            "Resolving contradiction {} -> {} as {strategy}",
            params.from_id, params.to_id
        );

        // Retract = the disputing memory wins: record the supersession FIRST
        // (if this fails the dispute must stay open), then retire the edge.
        if strategy == "owner_retracted" {
            self.client()
                .tooling()
                .record_supersession(
                    &params.from_id,
                    &params.to_id,
                    "owner retracted in contradiction review",
                )
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        }

        let resolved = match self
            .client()
            .tooling()
            .resolve_memory_contradictions(&params.from_id, strategy)
            .await
        {
            Ok(()) => true,
            // "No value found" = no open dispute (already resolved or bogus
            // ids) — graceful, not an error: the end state is what was asked.
            Err(e) if e.to_string().to_lowercase().contains("no value found") => false,
            Err(e) => return Err(McpError::internal_error(e.to_string(), None)),
        };

        // #34 2b: every settled dispute is a PRECEDENT. Record the episode
        // (best-effort) and, when enough identical verdicts accumulate,
        // hand the agent a ready-to-adopt rule proposal.
        let rule_proposal = if resolved {
            self.client()
                .tooling()
                .record_charter_precedent(&params.from_id, &params.to_id, strategy)
                .await
        } else {
            None
        };

        let mut payload = json!({
            "resolved": resolved,
            "from_id": params.from_id,
            "to_id": params.to_id,
            "strategy": strategy,
            "note": if resolved { "dispute retired; it will not re-surface" } else { "no open dispute found for from_id (already resolved?)" },
        });
        if let Some(p) = rule_proposal {
            payload["rule_proposal"] = json!({
                "shape": p.shape,
                "precedents": p.precedents,
                "proposal": p.proposal,
            });
        }
        let json = Self::result_to_json(payload)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Say goodbye to the swarm: stamp one existing execution instance 'done' and make it inactive immediately without affecting active siblings in the same logical-principal family. Pass the owning actor_id and the same agent_id used on agent_heartbeat and any add_memory calls. Permanent RBAC rejects cross-principal termination. Cheap and idempotent; the durable Agent node and authorship provenance remain intact, while an unknown id returns found=false and creates no registry row. GATED by the collective tier: Solo returns {available:false}."
    )]
    async fn agent_farewell(
        &self,
        Parameters(params): Parameters<AgentFarewellParams>,
    ) -> Result<CallToolResult, McpError> {
        if !self.client().config().mode.collective_enabled() {
            return Ok(CallToolResult::success(vec![Content::text(
                json!({"available": false, "reason": "solo mode has no swarm"}).to_string(),
            )]));
        }
        let actor_id = self
            .resolve_actor_id_without_presence(params.actor_id.as_deref(), &params.agent_id)
            .await?;
        let role = self.client().config().swarm.default_role.clone();
        let found = self
            .client()
            .tooling()
            .farewell_existing(&params.agent_id, &actor_id, &role, machine_hostname())
            .await
            .map_err(|error| match error {
                crate::toolkit::tooling_manager::types::ToolingError::Memory(message) => {
                    McpError::invalid_request(message, None)
                }
                other => McpError::internal_error(other.to_string(), None),
            })?;
        if found {
            info!("Farewell stamped for {}", params.agent_id);
        }
        Ok(CallToolResult::success(vec![Content::text(
            json!({
                "available": true,
                "principal_id": actor_id,
                "agent_id": params.agent_id,
                "found": found,
                "status": if found { "done" } else { "not_found" },
            })
            .to_string(),
        )]))
    }
}

fn validate_presence_value<'a>(
    field: &str,
    value: &'a str,
    max_chars: usize,
) -> Result<&'a str, McpError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max_chars {
        return Err(McpError::invalid_params(
            format!("{field} must contain 1..={max_chars} characters"),
            None,
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(McpError::invalid_params(
            format!("{field} must not contain control characters"),
            None,
        ));
    }
    Ok(value)
}

fn validate_heartbeat_status(value: &str) -> Result<&str, McpError> {
    let status = validate_presence_value("status", value, 64)?;
    if !crate::toolkit::tooling_manager::swarm::status_allows_activity(status) {
        return Err(McpError::invalid_params(
            "agent_heartbeat accepts only non-terminal status; use agent_farewell to finish",
            None,
        ));
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::{validate_heartbeat_status, validate_presence_value};

    #[test]
    fn presence_values_are_bounded_and_control_safe() {
        assert_eq!(
            validate_presence_value("status", " reviewing ", 64).unwrap(),
            "reviewing"
        );
        assert!(validate_presence_value("agent_id", "", 128).is_err());
        assert!(validate_presence_value("agent_id", "bad\nvalue", 128).is_err());
        assert!(validate_presence_value("status", &"x".repeat(65), 64).is_err());
        assert!(validate_heartbeat_status("working").is_ok());
        assert!(validate_heartbeat_status("done").is_err());
        assert!(validate_heartbeat_status("farewell").is_err());
    }
}
