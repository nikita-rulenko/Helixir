//! Swarm presence and contradiction-resolution tools.
//! Long-term memory MCP tools.
//!
//! Covers the user-visible memory verbs: add, search (semantic + concept +
//! reasoning chain), list, update, graph, and the helper that finds
//! previously-timed-out FastThink commits.

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
        description = "Who is in the swarm RIGHT NOW — the agent rendezvous. Returns the roster of agents known to this collective (live ones first): {agent_id, role, host, status, age_seconds, active}. An agent is ACTIVE only when its status is non-terminal and its last heartbeat is within active_window_secs (default from config, ~90s). agent_farewell makes it inactive immediately; the time window is the crash fallback. Agents silent past presence_ttl_secs (default 30 min) are presumed gone and hidden from the roster (hidden_stale counts them); their authorship provenance on memories is untouched. Presence is stamped automatically when an agent passes agent_id to add_memory, so writing agents appear here without any extra call. Use it to see who else is working, from which host, and what they last reported as their status; read what an agent DID via list_memories/search_memory over its user_id. GATED by the collective tier: Solo returns {available:false} (a private memory has no swarm)."
    )]
    async fn swarm_status(
        &self,
        Parameters(params): Parameters<SwarmStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        if !self.client().config().mode.collective_enabled() {
            let payload = json!({
                "available": false,
                "agents": [],
                "note": "The swarm roster requires the collective tier; this Helixir runs in Solo mode (private memory). Set mode=Collective or Insights to join a swarm.",
            });
            return Ok(CallToolResult::success(vec![Content::text(
                payload.to_string(),
            )]));
        }

        self.touch_configured_presence("working").await;

        let window = params
            .active_window_secs
            .unwrap_or(self.client().config().swarm.active_window_secs) as i64;
        let mut agents = self
            .client()
            .tooling()
            .list_swarm()
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

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
        let active = roster
            .iter()
            .filter(|a| a["active"].as_bool() == Some(true))
            .count();
        info!("Swarm roster: {} agents, {} active", roster.len(), active);
        let payload = json!({
            "available": true,
            "active_window_secs": window,
            "presence_ttl_secs": ttl,
            "active": active,
            "total": roster.len(),
            "hidden_stale": hidden_stale,
            "agents": roster,
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
        description = "Say goodbye to the swarm: stamp your presence status 'done' and become inactive immediately when your job is finished. Pass the same agent_id you used on add_memory. Cheap and idempotent; your durable Agent node and authorship provenance remain intact. GATED by the collective tier: Solo returns {available:false}."
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
        let role = self.client().config().swarm.default_role.clone();
        self.client()
            .tooling()
            .register_or_heartbeat(&params.agent_id, &role, machine_hostname(), "done")
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        info!("Farewell stamped for {}", params.agent_id);
        Ok(CallToolResult::success(vec![Content::text(
            json!({"available": true, "agent_id": params.agent_id, "status": "done"}).to_string(),
        )]))
    }
}
