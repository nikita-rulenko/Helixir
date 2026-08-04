//! Memory search, listing, and registry tools.
//! Long-term memory MCP tools.
//!
//! Covers the user-visible memory verbs: add, search (semantic + concept +
//! reasoning chain), list, update, graph, and the helper that finds
//! previously-timed-out FastThink commits.

use rmcp::{
    ErrorData as McpError, handler::server::wrapper::Parameters, model::*, tool, tool_router,
};
use serde_json::json;
use tracing::{debug, info};

use crate::mcp::params::*;
use crate::mcp::server::{HelixirMcpServer, is_empty_user_graph_error};

#[tool_router(router = memory_read_router, vis = "pub(super)")]
impl HelixirMcpServer {
    #[tool(
        description = "Check the status of a buffered add_memory by its pending_id. With RBAC enabled, pass actor_id; only the write owner, creator, or a global admin may inspect it. Returns {status: pending|processing|done|failed|not_found, result?, error?}. Optional — outcomes are also delivered opportunistically as pending_outcomes on your next add_memory, so polling is not required."
    )]
    async fn get_add_status(
        &self,
        Parameters(params): Parameters<GetAddStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        let actor_id = self.actor_id(params.actor_id.as_deref(), "default").await?;
        let status = self
            .client()
            .add_status_as(&actor_id, &params.pending_id)
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
        let actor_id = self
            .actor_id(params.actor_id.as_deref(), &params.user_id)
            .await?;
        let mode = params
            .mode
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| self.client().config().default_search_mode.clone());
        let limit = params.limit.map(|l| l as usize);
        // Default scope is intentionally personal (GH #40): collective memory
        // stays hidden unless explicitly requested, so weak models aren't
        // flooded with other users' facts. Not a config knob — a safety default.
        let requested_scope = params.scope.map(|s| s.as_str()).unwrap_or("personal");
        // Solo mode answers only from the user's own memory — a collective/all
        // request is downgraded to personal rather than leaking other users'.
        let scope = if self.client().config().mode.collective_enabled() {
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
        if let (Some(f), Some(t)) = (&window.from, &window.to)
            && f > t
        {
            return Err(McpError::invalid_params(
                format!("empty window: time_from {f} is after time_to {t}"),
                None,
            ));
        }

        let query_preview: String = params.query.chars().take(50).collect();
        info!(
            "Searching: '{}' [mode={}, limit={:?}, scope={}, window={:?}..{:?}]",
            query_preview, mode, limit, scope, window.from, window.to
        );

        let results = self
            .client()
            .search_as(
                &actor_id,
                &params.query,
                &params.user_id,
                crate::core::helixir_client::SearchParams {
                    limit,
                    search_mode: Some(mode.clone()),
                    temporal_days: params.temporal_days,
                    graph_depth: params.graph_depth.map(|d| d as usize),
                    scope: Some(scope.to_string()),
                    window,
                },
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
        let threshold = self.client().config().recall_thin_hint_threshold;
        if scope == "personal"
            && threshold > 0
            && results.len() < threshold
            && self.client().config().mode.collective_enabled()
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
        let actor_id = self
            .actor_id(params.actor_id.as_deref(), &params.user_id)
            .await?;
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
            .client()
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

        let memory_ids = memories
            .iter()
            .filter_map(|memory| memory.get("memory_id").and_then(serde_json::Value::as_str))
            .map(str::to_string)
            .collect::<Vec<_>>();
        let visible = self
            .client()
            .rbac()
            .visible_memory_ids(&actor_id, &memory_ids)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        if let Some(visible) = visible {
            memories.retain(|memory| {
                memory
                    .get("memory_id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| visible.contains(id))
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
        if !self.client().config().mode.collective_enabled() {
            let payload = json!({
                "available": false,
                "users": [],
                "note": "User discovery requires the collective tier; this Helixir runs in Solo mode (private memory). Set HELIXIR_MODE=collective to enable a shared roster.",
            });
            return Ok(CallToolResult::success(vec![Content::text(
                payload.to_string(),
            )]));
        }

        let policy = self
            .client()
            .rbac()
            .snapshot()
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        if policy.enabled {
            let Some(actor_id) = params.actor_id.as_deref() else {
                return Err(McpError::invalid_request(
                    "RBAC-enabled roster discovery requires actor_id",
                    None,
                ));
            };
            if !policy.is_admin(actor_id) {
                return Err(McpError::invalid_request(
                    "RBAC denied roster discovery; global admin role required",
                    None,
                ));
            }
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
            .client()
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
}
