//! FastThink MCP tools — ephemeral working-memory sessions.
//!
//! These tools never touch HelixDB directly. They drive the in-process
//! `petgraph` scratchpad in [`FastThinkManager`]. Only `think_commit` (and
//! the automatic timeout commit inside `think_add`) persist anything.

use rmcp::{
    ErrorData as McpError, handler::server::wrapper::Parameters, model::*, tool, tool_router,
};
use serde_json::json;
use tracing::{info, warn};

use crate::mcp::params::*;
use crate::mcp::server::HelixirMcpServer;
use crate::toolkit::fast_think::{FastThinkError, ThoughtType};

#[tool_router(router = think_router, vis = "pub(super)")]
impl HelixirMcpServer {
    #[tool(
        description = "Start a FastThink session for reasoning. Creates an isolated working memory graph. Returns: {session_id, root_thought_idx}"
    )]
    async fn think_start(
        &self,
        Parameters(params): Parameters<StartThinkingParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("🧠 Starting thinking session: {}", params.session_id);

        let result = self
            .fast_think
            .start_thinking(&params.session_id, &params.initial_thought)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let json = Self::result_to_json(json!({
            "session_id": params.session_id,
            "root_thought_idx": result.index(),
            "status": "thinking"
        }))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Add a thought to an active FastThink session. Returns: {thought_idx, thought_count, depth}"
    )]
    async fn think_add(
        &self,
        Parameters(params): Parameters<AddThoughtParams>,
    ) -> Result<CallToolResult, McpError> {
        let thought_type = match params.thought_type.as_deref() {
            Some("hypothesis") => ThoughtType::Hypothesis,
            Some("observation") => ThoughtType::Observation,
            Some("question") => ThoughtType::Question,
            _ => ThoughtType::Reasoning,
        };

        let parent = params
            .parent_idx
            .map(|idx| petgraph::stable_graph::NodeIndex::new(idx as usize));

        let result = self.fast_think.add_thought(
            &params.session_id,
            &params.content,
            thought_type,
            parent,
            None,
        );

        match result {
            Ok(node) => {
                let status = self
                    .fast_think
                    .get_session_status(&params.session_id)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;

                let json = Self::result_to_json(json!({
                    "thought_idx": node.index(),
                    "thought_count": status.thought_count,
                    "depth": status.current_depth
                }))?;
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
            Err(FastThinkError::Timeout) => {
                warn!("⏰ FastThink timeout - committing partial results");
                let commit_result = self
                    .fast_think
                    .commit_partial(&params.session_id, "claude", "timeout")
                    .await;

                match commit_result {
                    Ok(cr) => {
                        let json = Self::result_to_json(json!({
                            "status": "timeout_committed",
                            "memory_id": cr.memory_id,
                            "thoughts_saved": cr.thoughts_processed,
                            "message": "⚠️ Thinking timed out. Partial thoughts saved to memory for future research."
                        }))?;
                        Ok(CallToolResult::success(vec![Content::text(json)]))
                    }
                    Err(e) => Err(McpError::internal_error(
                        format!("Timeout and commit failed: {}", e),
                        None,
                    )),
                }
            }
            Err(e) => Err(McpError::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Recall facts from main memory into FastThink session. READ-ONLY access to main memory. Returns: {recalled_count, thought_indices}"
    )]
    async fn think_recall(
        &self,
        Parameters(params): Parameters<ThinkRecallParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("💭 Recalling from main memory: '{}'", params.query);

        let parent = petgraph::stable_graph::NodeIndex::new(params.parent_idx as usize);
        let user_id = params.user_id.as_deref().unwrap_or("default");

        let results = self
            .fast_think
            .recall(&params.session_id, &params.query, parent, user_id)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let indices: Vec<usize> = results.iter().map(|n| n.index()).collect();

        info!("✅ Recalled {} facts", results.len());

        let json = Self::result_to_json(json!({
            "recalled_count": results.len(),
            "thought_indices": indices
        }))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Mark a conclusion in FastThink session. Required before commit. Returns: {conclusion_idx, status}"
    )]
    async fn think_conclude(
        &self,
        Parameters(params): Parameters<ThinkConcludeParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("✨ Concluding thinking session: {}", params.session_id);

        let supporting: Vec<petgraph::stable_graph::NodeIndex> = params
            .supporting_idx
            .unwrap_or_default()
            .iter()
            .map(|&idx| petgraph::stable_graph::NodeIndex::new(idx as usize))
            .collect();

        let result = self
            .fast_think
            .conclude(&params.session_id, &params.conclusion, &supporting)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let json = Self::result_to_json(json!({
            "conclusion_idx": result.index(),
            "status": "decided"
        }))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Commit FastThink session to main memory. Writes conclusion with supporting evidence. Returns: {memory_id, thoughts_processed, elapsed_ms}"
    )]
    async fn think_commit(
        &self,
        Parameters(params): Parameters<ThinkCommitParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("📝 Committing thinking session: {}", params.session_id);

        let result = self
            .fast_think
            .commit(&params.session_id, &params.user_id)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        info!(
            "✅ Committed: {} thoughts → memory {}",
            result.thoughts_processed, result.memory_id
        );

        let json = Self::result_to_json(json!({
            "memory_id": result.memory_id,
            "thoughts_processed": result.thoughts_processed,
            "entities_extracted": result.entities_extracted,
            "concepts_mapped": result.concepts_mapped,
            "elapsed_ms": result.elapsed.as_millis()
        }))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Discard FastThink session without saving. Clears working memory. Returns: {discarded_thoughts, elapsed_ms}"
    )]
    async fn think_discard(
        &self,
        Parameters(params): Parameters<ThinkDiscardParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("🗑️ Discarding thinking session: {}", params.session_id);

        let result = self
            .fast_think
            .discard(&params.session_id)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let json = Self::result_to_json(json!({
            "discarded_thoughts": result.thoughts_discarded,
            "elapsed_ms": result.elapsed.as_millis()
        }))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get status of a FastThink session. Returns: {status, thought_count, depth, has_conclusion, elapsed_ms}"
    )]
    async fn think_status(
        &self,
        Parameters(params): Parameters<ThinkStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        let status = self
            .fast_think
            .get_session_status(&params.session_id)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let json = Self::result_to_json(json!({
            "session_id": status.id,
            "status": status.status.to_string(),
            "thought_count": status.thought_count,
            "entity_count": status.entity_count,
            "concept_count": status.concept_count,
            "current_depth": status.current_depth,
            "has_conclusion": status.has_conclusion,
            "elapsed_ms": status.elapsed.as_millis()
        }))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}
