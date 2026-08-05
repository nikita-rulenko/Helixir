//! Memory write and buffered-status tools.
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
use crate::mcp::server::HelixirMcpServer;

use super::memory_support::machine_hostname;

#[tool_router(router = memory_write_router, vis = "pub(super)")]
impl HelixirMcpServer {
    #[tool(
        description = "Store raw natural-language text in long-term memory. An LLM splits it into atomic typed facts (max 15 per call — split bigger inputs), embeds them, and wires them into the reasoning graph with typed edges. Use whenever the user states a fact, decision, preference, goal or outcome worth keeping across sessions. Provide actor_id and the concrete access group_id; Helixir resolves any dedup federation automatically.\
        \nRESULT CONTRACT — read carefully:\
        \n- ok:true = SUCCESS. NEVER retry an ok:true result.\
        \n- ok:true + memory_ids = stored now.\
        \n- ok:true + memory_ids = stored as new; ok:true + updated = existing memories changed.\
        \n- ok:true + status:'accepted' + pending_id = buffered write still finishing; searchable within seconds; optionally confirm via get_add_status(pending_id). Still SUCCESS.\
        \n- memories_added:0 with non-empty 'deduped' = this fact was ALREADY known and got linked. 'saved' counts added + updated + deduped outcomes. SUCCESS, not a failure.\
        \n- Only ok:false / status:'failed' is a real failure.\
        \n- 'pending_outcomes' = results of EARLIER buffered adds, delivered opportunistically.\
        \nneeds_clarification: if non-empty, the memory charter refused to silently resolve a conflict (e.g. a reversed preference). Ask the user each suggested_question (or apply a standing rule), then store the answer as a new memory. Never ignore it."
    )]
    async fn add_memory(
        &self,
        Parameters(params): Parameters<AddMemoryParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("Adding memory for user={}", params.user_id);
        let actor_id = self
            .actor_id(params.actor_id.as_deref(), &params.user_id)
            .await?;

        // Rendezvous (#39): a writing agent announces its presence for free —
        // any agent that passes agent_id shows up in swarm_status with host +
        // "working" without a separate heartbeat call. Best-effort by design.
        if let Some(agent_id) = params.agent_id.as_deref()
            && self.client().config().mode.collective_enabled()
        {
            let role = self.client().config().swarm.default_role.clone();
            if let Err(e) = self
                .client()
                .tooling()
                .register_or_heartbeat(agent_id, &role, machine_hostname(), "working")
                .await
            {
                debug!("swarm heartbeat for {agent_id} failed (non-fatal): {e}");
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
                .client()
                .add_buffered_as_in_group(
                    &actor_id,
                    &params.message,
                    &params.user_id,
                    params.agent_id.as_deref(),
                    None,
                    params.group_id.as_deref(),
                )
                .await
                .map_err(Self::convert_error)?;
            info!("Queued {} for background processing", enq.pending_id);

            // Opportunistic outbox delivery FIRST: ride EARLIER write outcomes
            // back so the agent learns them without polling. Drain before the
            // await so we don't consume (and prune) THIS item's own outcome —
            // it is delivered inline below, and its tombstone stays pollable.
            let outcomes = self
                .client()
                .drain_notices_as(&actor_id, &params.user_id, 20)
                .await
                .unwrap_or_default();

            // Wait (bounded, configurable) for the serial worker to finish this
            // exact item. Waiting does not parallelize processing, so the
            // dedup-race protection the buffer exists for is preserved.
            let client = self.client();
            let ingest = &client.config().ingest;
            let confirmed = self
                .client()
                .await_add_as(
                    &actor_id,
                    &enq.pending_id,
                    ingest.ack_wait_ms,
                    ingest.ack_poll_ms,
                )
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
            .client()
            .add_as_in_group(
                &actor_id,
                &params.message,
                &params.user_id,
                params.agent_id.as_deref(),
                None,
                params.group_id.as_deref(),
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
        json["saved"] = json!(result.memories_added + result.updated.len() + result.deduped.len());
        Ok(CallToolResult::success(vec![Content::text(
            json.to_string(),
        )]))
    }
}
