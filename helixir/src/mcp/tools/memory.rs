//! Long-term memory MCP tools.
//!
//! Covers the user-visible memory verbs: add, search (semantic + concept +
//! reasoning chain), list, update, graph, and the helper that finds
//! previously-timed-out FastThink commits.

use rmcp::{
    ErrorData as McpError, handler::server::wrapper::Parameters, model::*, tool, tool_router,
};
use serde_json::json;
use tracing::{debug, info, warn};

use crate::mcp::params::*;
use crate::mcp::server::{HelixirMcpServer, is_empty_user_graph_error};

#[tool_router(router = memory_router, vis = "pub(super)")]
impl HelixirMcpServer {
    #[tool(
        description = "Store raw natural-language text in long-term memory. An LLM splits it into atomic typed facts (max 15 per call — split bigger inputs), embeds them, and wires them into the reasoning graph with typed edges. Use whenever the user states a fact, decision, preference, goal or outcome worth keeping across sessions.\
        \nRESULT CONTRACT — read carefully:\
        \n- ok:true = SUCCESS. NEVER retry an ok:true result.\
        \n- ok:true + memory_ids = stored now.\
        \n- ok:true + status:'accepted' + pending_id = buffered write still finishing; searchable within seconds; optionally confirm via get_add_status(pending_id). Still SUCCESS.\
        \n- memories_added:0 with non-empty 'deduped' = this fact was ALREADY known and got linked ('saved' = memories_added + deduped). SUCCESS, not a failure.\
        \n- Only ok:false / status:'failed' is a real failure.\
        \n- 'pending_outcomes' = results of EARLIER buffered adds, delivered opportunistically.\
        \nneeds_clarification: if non-empty, the memory charter refused to silently resolve a conflict (e.g. a reversed preference). Ask the user each suggested_question (or apply a standing rule), then store the answer as a new memory. Never ignore it."
    )]
    async fn add_memory(
        &self,
        Parameters(params): Parameters<AddMemoryParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("Adding memory for user={}", params.user_id);

        // Rendezvous (#39): a writing agent announces its presence for free —
        // any agent that passes agent_id shows up in swarm_status with host +
        // "working" without a separate heartbeat call. Best-effort by design.
        if let Some(agent_id) = params.agent_id.as_deref() {
            if self.client.config().mode.collective_enabled() {
                let role = self.client.config().swarm.default_role.clone();
                if let Err(e) = self
                    .client
                    .tooling()
                    .register_or_heartbeat(agent_id, &role, machine_hostname(), "working")
                    .await
                {
                    debug!("swarm heartbeat for {agent_id} failed (non-fatal): {e}");
                }
            }
        }

        // Ingest buffer (#25): when HELIXIR_INGEST_BUFFER=1, the raw input is
        // persisted to a queue drained by ONE serial worker, so parallel
        // writers can't race the dedup check. Confirm-or-promise (#63): we then
        // briefly wait for THIS write to finish and return its real result, so
        // the agent gets memory_ids it can trust — never a bare "pending" it
        // misreads as failure (which made swarm agents retry or defect).
        if crate::toolkit::tooling_manager::ingest_buffer::buffer_enabled() {
            use crate::toolkit::tooling_manager::ingest_buffer::{STATUS_DONE, STATUS_FAILED};
            let enq = self
                .client
                .add_buffered(
                    &params.message,
                    &params.user_id,
                    params.agent_id.as_deref(),
                    None,
                )
                .await
                .map_err(Self::convert_error)?;
            info!("Queued {} for background processing", enq.pending_id);

            // Opportunistic outbox delivery FIRST: ride EARLIER write outcomes
            // back so the agent learns them without polling. Drain before the
            // await so we don't consume (and prune) THIS item's own outcome —
            // it is delivered inline below, and its tombstone stays pollable.
            let outcomes = self
                .client
                .drain_notices(&params.user_id, 20)
                .await
                .unwrap_or_default();

            // Wait (bounded, configurable) for the serial worker to finish this
            // exact item. Waiting does not parallelize processing, so the
            // dedup-race protection the buffer exists for is preserved.
            let ingest = &self.client.config().ingest;
            let confirmed = self
                .client
                .await_add(&enq.pending_id, ingest.ack_wait_ms, ingest.ack_poll_ms)
                .await;

            let mut json = match confirmed {
                // Finished in time -> return the real result, framed as success.
                Some(st) if st.status == STATUS_DONE => {
                    let mut v = st.result.unwrap_or_else(|| json!({}));
                    if !v.is_object() {
                        v = json!({ "result": v });
                    }
                    v["ok"] = json!(true);
                    v
                }
                // Genuinely failed -> say so honestly; never fake success.
                Some(st) => json!({
                    "ok": false,
                    "status": STATUS_FAILED,
                    "error": st.error.unwrap_or_else(|| "write failed".to_string()),
                }),
                // Still processing -> explicit ACCEPTED promise, never bare "pending".
                None => json!({
                    "ok": true,
                    "accepted": true,
                    "status": "accepted",
                    "message": "Saved to memory; still processing in the background and \
                                searchable within a few seconds. This is SUCCESS — do NOT retry. \
                                Optionally confirm later with get_add_status(pending_id).",
                }),
            };
            json["pending_id"] = json!(enq.pending_id);
            if !outcomes.is_empty() {
                json["pending_outcomes"] = serde_json::to_value(&outcomes).unwrap_or_default();
            }
            return Ok(CallToolResult::success(vec![Content::text(
                json.to_string(),
            )]));
        }

        let result = self
            .client
            .add(
                &params.message,
                &params.user_id,
                params.agent_id.as_deref(),
                None,
            )
            .await
            .map_err(Self::convert_error)?;

        info!(
            "Added {} memories ({} chunks)",
            result.memories_added, result.chunks_created
        );

        // Frame the synchronous result as an unambiguous success too (#63): a
        // dedup (memories_added=0 with a non-empty `deduped`) is "already
        // saved", not a failure — `ok:true` and a `saved` count say so plainly
        // so agents don't misread a no-op dedup as a failed write.
        let mut json = Self::result_to_value(&result)?;
        json["ok"] = json!(true);
        json["saved"] = json!(result.memories_added + result.deduped.len());
        Ok(CallToolResult::success(vec![Content::text(
            json.to_string(),
        )]))
    }

    #[tool(
        description = "Check the status of a buffered add_memory by its pending_id. Returns {status: pending|processing|done|failed|not_found, result?, error?}. Optional — outcomes are also delivered opportunistically as pending_outcomes on your next add_memory, so polling is not required."
    )]
    async fn get_add_status(
        &self,
        Parameters(params): Parameters<GetAddStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        let status = self
            .client
            .add_status(&params.pending_id)
            .await
            .map_err(Self::convert_error)?;
        let json = Self::result_to_json(&status)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Recall memories by meaning — the DEFAULT retrieval tool (hybrid dense + keyword + graph, no LLM call). Use it to answer 'what do I know about X'. Pick a sibling instead when: you want the WHY behind something -> search_reasoning_chain; to bridge two specific concepts -> connect_memories; to filter by ontology type/tags -> search_by_concept; to dump everything for a user -> list_memories. 'mode' sets recall breadth (recent ~4h / contextual ~30d default / deep ~90d / full = whole store; use full if a query you expect to match returns empty). 'time_from'/'time_to' (RFC3339 or YYYY-MM-DD) bound recall to an explicit EVENT-time window; memories outside the window that are linked to in-window results via the graph still return as FLASHBACKS — flagged metadata.flashback=true with their event_date, capped separately so they never crowd in-window rows. 'scope' defaults to personal; collective/all need the collective tier and are downgraded to personal otherwise. Returns ranked [{memory_id, content, score, metadata}] where metadata carries provenance (origin, edge, ppr, cosine). When a result's metadata has 'collapsed', those memory_ids are the same story folded under this row (a raw source and its extracted atoms never coexist in one window) — the content is NOT lost; fetch a folded id explicitly if you need its exact wording. A result with 'superseded: true' is OUTDATED (ranked down, kept for history) — 'superseded_by' names the current version; never act on a superseded row as current truth."
    )]
    async fn search_memory(
        &self,
        Parameters(params): Parameters<SearchMemoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let mode = params
            .mode
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| self.client.config().default_search_mode.clone());
        let limit = params.limit.map(|l| l as usize);
        // Default scope is intentionally personal (GH #40): collective memory
        // stays hidden unless explicitly requested, so weak models aren't
        // flooded with other users' facts. Not a config knob — a safety default.
        let requested_scope = params.scope.map(|s| s.as_str()).unwrap_or("personal");
        // Solo mode answers only from the user's own memory — a collective/all
        // request is downgraded to personal rather than leaking other users'.
        let scope = if self.client.config().mode.collective_enabled() {
            requested_scope
        } else {
            "personal"
        };

        // #87: explicit event-time window. A malformed bound is the caller's
        // error — reject loudly instead of silently searching unbounded.
        let mut window = crate::core::TimeWindow::default();
        if let Some(ref s) = params.time_from {
            window.from = Some(
                crate::core::time_window::parse_time_bound(s, false)
                    .map_err(|e| McpError::invalid_params(format!("time_from: {e}"), None))?,
            );
        }
        if let Some(ref s) = params.time_to {
            window.to = Some(
                crate::core::time_window::parse_time_bound(s, true)
                    .map_err(|e| McpError::invalid_params(format!("time_to: {e}"), None))?,
            );
        }
        if let (Some(f), Some(t)) = (&window.from, &window.to) {
            if f > t {
                return Err(McpError::invalid_params(
                    format!("empty window: time_from {f} is after time_to {t}"),
                    None,
                ));
            }
        }

        let query_preview: String = params.query.chars().take(50).collect();
        info!(
            "Searching: '{}' [mode={}, limit={:?}, scope={}, window={:?}..{:?}]",
            query_preview, mode, limit, scope, window.from, window.to
        );

        let results = self
            .client
            .search_windowed(
                &params.query,
                &params.user_id,
                limit,
                Some(&mode),
                params.temporal_days,
                params.graph_depth.map(|d| d as usize),
                Some(scope),
                window,
            )
            .await
            .map_err(Self::convert_error)?;

        info!("Found {} memories", results.len());

        // content[0] stays the ranked array (stable contract). When a PERSONAL
        // recall comes back thin and the collective tier is available, append a
        // second content block nudging the agent to the existing collective
        // escape hatch (#64) — a hint, not a roster dump, and never in Solo
        // (where collective would just downgrade back to personal).
        let mut contents = vec![Content::text(Self::result_to_json(&results)?)];
        let threshold = self.client.config().recall_thin_hint_threshold;
        if scope == "personal"
            && threshold > 0
            && results.len() < threshold
            && self.client.config().mode.collective_enabled()
        {
            contents.push(Content::text(format!(
                "Hint: personal scope returned {} result(s). If you expected more, retry search_memory with scope=\"collective\" to include the shared collective memory; or call list_users to check which user_id holds the knowledge.",
                results.len()
            )));
        }
        Ok(CallToolResult::success(contents))
    }

    #[tool(
        description = "Dump a user's memories in bulk (newest first), with NO ranking by relevance — use it for counting, auditing, or seeing everything; for 'what's relevant to X' use search_memory instead. Optionally restrict to one ontology type via memory_type. Capped by 'limit' (default 100) and truncated on large stores, so it is not a substitute for search. Returns [{memory_id, content, memory_type, created_at, importance, certainty}]."
    )]
    async fn list_memories(
        &self,
        Parameters(params): Parameters<ListMemoriesParams>,
    ) -> Result<CallToolResult, McpError> {
        let limit = params.limit.unwrap_or(100) as i64;
        info!(
            "Listing memories for user={}, limit={}",
            params.user_id, limit
        );

        #[derive(serde::Deserialize)]
        struct MemoriesResponse {
            #[serde(default)]
            memories: Vec<serde_json::Value>,
        }

        // HelixDB raises `Graph error: No value found` (also serialised with
        // the code `GRAPH_ERROR`) when the user has zero outgoing
        // `HAS_MEMORY` edges — i.e. either the user node is brand new or it
        // doesn't exist yet. Both states are semantically equivalent to "no
        // memories", so we map them to an empty Vec instead of bubbling an
        // MCP error to the caller. See issue #19.
        let result: MemoriesResponse = match self
            .client
            .db()
            .execute_query(
                "getUserMemories",
                &serde_json::json!({
                    "user_id": params.user_id,
                    "limit": limit
                }),
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let msg = e.to_string();
                if is_empty_user_graph_error(&msg) {
                    debug!(
                        "list_memories: user '{}' has no memories yet (HelixDB returned '{}')",
                        params.user_id, msg
                    );
                    MemoriesResponse {
                        memories: Vec::new(),
                    }
                } else {
                    return Err(McpError::internal_error(msg, None));
                }
            }
        };

        let mut memories = result.memories;

        if let Some(mem_type) = params.memory_type {
            memories.retain(|m| {
                m.get("memory_type")
                    .and_then(|v| v.as_str())
                    .map(|t| t == mem_type.as_str())
                    .unwrap_or(false)
            });
        }

        info!("Listed {} memories", memories.len());
        let json = serde_json::to_string_pretty(&memories)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "List the identities (user_ids) present in this Helixir, newest first — a deliberately small roster for ORIENTATION, not a full dump. Call it when you are unsure which user_id to use, want to find your OWN stable identity, or need a teammate's user_id to read their memories. It does NOT tell you which id is yours — pick one stable user_id and use it consistently on every call. Privacy: returns only {user_id, name, created_at}, never emails or content. GATED by the collective tier: in Solo mode it returns {available:false} with no roster (discovery is a shared-collective affordance). To read an identity's memories use list_memories(user_id); to search across everyone use search_memory(scope='collective')."
    )]
    async fn list_users(
        &self,
        Parameters(params): Parameters<ListUsersParams>,
    ) -> Result<CallToolResult, McpError> {
        // Discovery is gated by the collective tier — the same privilege as a
        // collective read (#40/#64). Solo keeps the roster private rather than
        // leaking who exists.
        if !self.client.config().mode.collective_enabled() {
            let payload = json!({
                "available": false,
                "users": [],
                "note": "User discovery requires the collective tier; this Helixir runs in Solo mode (private memory). Set HELIXIR_MODE=collective to enable a shared roster.",
            });
            return Ok(CallToolResult::success(vec![Content::text(
                payload.to_string(),
            )]));
        }

        let limit = params.limit.unwrap_or(50).max(1) as usize;
        info!("Listing users (limit={})", limit);

        #[derive(serde::Deserialize)]
        struct UsersResponse {
            #[serde(default)]
            users: Vec<serde_json::Value>,
        }

        // Reuses the already-deployed getAllUsers query (no schema change).
        let resp: UsersResponse = self
            .client
            .db()
            .execute_query("getAllUsers", &serde_json::json!({}))
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let total = resp.users.len();
        let mut users = resp.users;
        // Newest first so the window is the most relevant slice of a big roster.
        users.sort_by(|a, b| {
            let ca = b.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
            let cb = a.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
            ca.cmp(cb)
        });
        // Project to a privacy-safe roster — no email / metadata / internal id.
        let roster: Vec<serde_json::Value> = users
            .into_iter()
            .take(limit)
            .map(|u| {
                json!({
                    "user_id": u.get("user_id").and_then(|v| v.as_str()).unwrap_or(""),
                    "name": u.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "created_at": u.get("created_at").and_then(|v| v.as_str()).unwrap_or(""),
                })
            })
            .collect();

        info!("Listed {}/{} users", roster.len(), total);
        let payload = json!({
            "available": true,
            "total_users": total,
            "returned": roster.len(),
            "users": roster,
            "note": "Roster for orientation. Pick your OWN stable user_id and use it consistently. Read an identity's memories with list_memories(user_id); search across everyone with search_memory(scope='collective').",
        });
        Ok(CallToolResult::success(vec![Content::text(
            payload.to_string(),
        )]))
    }

    #[tool(
        description = "Who is in the swarm RIGHT NOW — the agent rendezvous. Returns the roster of agents known to this collective (live ones first): {agent_id, role, host, status, age_seconds, active}. An agent is ACTIVE if its last heartbeat is within active_window_secs (default from config, ~90s). Agents silent past presence_ttl_secs (default 30 min) are presumed gone and hidden from the roster (hidden_stale counts them); their authorship provenance on memories is untouched. Presence is stamped automatically when an agent passes agent_id to add_memory, so writing agents appear here without any extra call. Use it to see who else is working, from which host, and what they last reported as their status; read what an agent DID via list_memories/search_memory over its user_id. GATED by the collective tier: Solo returns {available:false} (a private memory has no swarm)."
    )]
    async fn swarm_status(
        &self,
        Parameters(params): Parameters<SwarmStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        if !self.client.config().mode.collective_enabled() {
            let payload = json!({
                "available": false,
                "agents": [],
                "note": "The swarm roster requires the collective tier; this Helixir runs in Solo mode (private memory). Set mode=Collective or Insights to join a swarm.",
            });
            return Ok(CallToolResult::success(vec![Content::text(
                payload.to_string(),
            )]));
        }

        let window = params
            .active_window_secs
            .unwrap_or(self.client.config().swarm.active_window_secs) as i64;
        let mut agents = self
            .client
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
        let ttl = self.client.config().swarm.presence_ttl_secs as i64;
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
            self.client
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
            .client
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
            self.client
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
        description = "Say goodbye to the swarm: stamp your presence status 'done' when your job is finished (one-shot agents especially — without a farewell your last status reads 'working' forever). Pass the same agent_id you used on add_memory. Cheap and idempotent; your authorship provenance is untouched. GATED by the collective tier: Solo returns {available:false}."
    )]
    async fn agent_farewell(
        &self,
        Parameters(params): Parameters<AgentFarewellParams>,
    ) -> Result<CallToolResult, McpError> {
        if !self.client.config().mode.collective_enabled() {
            return Ok(CallToolResult::success(vec![Content::text(
                json!({"available": false, "reason": "solo mode has no swarm"}).to_string(),
            )]));
        }
        let role = self.client.config().swarm.default_role.clone();
        self.client
            .tooling()
            .register_or_heartbeat(&params.agent_id, &role, machine_hostname(), "done")
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        info!("Farewell stamped for {}", params.agent_id);
        Ok(CallToolResult::success(vec![Content::text(
            json!({"available": true, "agent_id": params.agent_id, "status": "done"}).to_string(),
        )]))
    }

    #[tool(
        description = "Replace the content of an EXISTING memory (you must pass its memory_id, e.g. from a search result); the embedding and graph relations are regenerated. Use to correct or refine a specific known fact. Note: this edits in place and Helixir never deletes — to retire an OUTDATED fact, prefer add_memory with the corrected statement and let the charter supersede the old one (history is preserved). Returns {updated: bool, memory_id}."
    )]
    async fn update_memory(
        &self,
        Parameters(params): Parameters<UpdateMemoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let id_preview: String = params.memory_id.chars().take(12).collect();
        info!("Updating memory: {}...", id_preview);

        let result = self
            .client
            .update(&params.memory_id, &params.new_content, &params.user_id)
            .await
            .map_err(Self::convert_error)?;

        if result.updated {
            info!("Memory updated");
        } else {
            warn!("Memory update failed");
        }

        let json = Self::result_to_json(&result)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Return the user's knowledge graph as {nodes, edges}. Nodes are memories ({id, content, node_type}); edges are typed relations ({source, target, edge_type, weight}) where edge_type is BECAUSE/IMPLIES/SUPPORTS/CONTRADICTS. Pass memory_id to get the ego-network around one memory (radius = depth, default 2); omit it for the user's whole local graph. Use this to inspect structure — to WALK a reasoning chain use search_reasoning_chain, to find a PATH between two memories use connect_memories."
    )]
    async fn get_memory_graph(
        &self,
        Parameters(params): Parameters<GetMemoryGraphParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("Getting memory graph for user={}", params.user_id);

        let result = self
            .client
            .get_graph(
                &params.user_id,
                params.memory_id.as_deref(),
                params.depth.map(|d| d as usize),
            )
            .await
            .map_err(Self::convert_error)?;

        let json = Self::result_to_json(&result)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Semantic search restricted to ONE ontology type and/or tags — like search_memory but when you only want, say, the user's goals or preferences. Set concept_type to filter (one of skill/preference/goal/fact/opinion/experience/achievement/action; omit to search all types) and/or 'tags' (comma-separated). For unrestricted recall use search_memory. Returns [{memory_id, content, concept_score}]."
    )]
    async fn search_by_concept(
        &self,
        Parameters(params): Parameters<SearchByConceptParams>,
    ) -> Result<CallToolResult, McpError> {
        let query_preview: String = params.query.chars().take(30).collect();
        info!(
            "Concept search: '{}' type={:?}",
            query_preview, params.concept_type
        );

        let results = self
            .client
            .search_by_concept(
                &params.query,
                &params.user_id,
                params.concept_type.map(|c| c.as_str()),
                params.tags.as_deref(),
                params.mode.map(|m| m.as_str()),
                params.limit.map(|l| l as usize),
            )
            .await
            .map_err(Self::convert_error)?;

        info!("Found {} memories", results.len());

        let json = Self::result_to_json(&results)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Reconstruct chains of reasoning around a topic — the 'why / what-follows' tool, and Helixir's signature capability. It finds seed memories then walks typed reasoning edges (BECAUSE/IMPLIES/SUPPORTS/CONTRADICTS) to assemble cause->effect chains with a human-readable reasoning_trail. Use chain_mode 'causal' for 'why is X so', 'forward' for 'what does X lead to', 'both'/'deep' for full context. Can return a LARGE payload on a dense graph — keep max_depth (default 5) and limit modest. Returns {query, chains:[{seed, nodes, reasoning_trail}], total_memories, deepest_chain}."
    )]
    async fn search_reasoning_chain(
        &self,
        Parameters(params): Parameters<SearchReasoningChainParams>,
    ) -> Result<CallToolResult, McpError> {
        let chain_mode = params.chain_mode.map(|c| c.as_str()).unwrap_or("both");

        let query_preview: String = params.query.chars().take(30).collect();
        info!("Reasoning chain: '{}' mode={}", query_preview, chain_mode);

        let result = self
            .client
            .search_reasoning_chain(
                &params.query,
                &params.user_id,
                Some(chain_mode),
                params.max_depth.map(|d| d as usize),
                params.limit.map(|l| l as usize),
            )
            .await
            .map_err(Self::convert_error)?;

        info!("Found {} chains", result.chains.len());

        let json = Self::result_to_json(&result)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Discover how two concepts are related through the memory graph: bidirectional path search between anchors A and B. Each anchor may be a free-text query OR an exact memory_id (mem_… / raw_…) — pass an id to connect a memory you already know precisely, bypassing the search step. Returns the connecting chain with edge types (IMPLIES/BECAUSE/...) and cumulative confidence. The elder-brain primitive: sees connections that are several logical hops apart."
    )]
    async fn connect_memories(
        &self,
        Parameters(params): Parameters<ConnectMemoriesParams>,
    ) -> Result<CallToolResult, McpError> {
        info!(
            "Connect: '{}' <-> '{}'",
            params.query_a.chars().take(30).collect::<String>(),
            params.query_b.chars().take(30).collect::<String>()
        );

        let result = self
            .client
            .connect_memories(
                &params.query_a,
                &params.query_b,
                &params.user_id,
                params.max_depth.map(|d| d as usize),
            )
            .await
            .map_err(Self::convert_error)?;

        info!("Connection: found={} hops={}", result.found, result.hops);

        let json = Self::result_to_json(&result)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Find incomplete thoughts from previous sessions that timed out. Use at session start to continue unfinished research. Returns: [{memory_id, content, created_at}]"
    )]
    async fn search_incomplete_thoughts(
        &self,
        Parameters(params): Parameters<SearchIncompleteThoughtsParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("Searching for incomplete thoughts");

        let limit = params.limit.unwrap_or(5) as usize;

        let results = self
            .client
            .tooling()
            .search_by_tag("incomplete_thought", limit)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        if results.is_empty() {
            let json = Self::result_to_json(json!({
                "found": 0,
                "message": "No incomplete thoughts found"
            }))?;
            return Ok(CallToolResult::success(vec![Content::text(json)]));
        }

        let json = Self::result_to_json(json!({
            "found": results.len(),
            "incomplete_thoughts": results.iter().map(|r| {
                json!({
                    "memory_id": r.memory_id,
                    "content": r.content,
                    "created_at": r.created_at
                })
            }).collect::<Vec<_>>()
        }))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

/// Cached machine hostname for swarm presence — resolved once per process.
fn machine_hostname() -> &'static str {
    use std::sync::OnceLock;
    static HOST: OnceLock<String> = OnceLock::new();
    HOST.get_or_init(|| {
        std::process::Command::new("hostname")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    })
}
