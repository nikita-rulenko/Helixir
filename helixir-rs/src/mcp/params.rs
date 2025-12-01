use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct AddMemoryParams {
    #[schemars(description = "Text to remember (will be extracted into atomic facts)")]
    pub message: String,
    #[schemars(description = "User identifier (e.g., 'claude', 'developer')")]
    pub user_id: String,
    #[schemars(description = "Optional agent identifier")]
    pub agent_id: Option<String>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct SearchMemoryParams {
    #[schemars(description = "Search query")]
    pub query: String,
    #[schemars(description = "User identifier")]
    pub user_id: String,
    #[schemars(description = "Max results (default: mode-based)")]
    pub limit: Option<i32>,
    #[schemars(description = "Search mode: 'recent' (4h), 'contextual' (30d), 'deep' (90d), 'full'")]
    pub mode: Option<String>,
    #[schemars(description = "Override time window in days")]
    pub temporal_days: Option<f64>,
    #[schemars(description = "Override graph depth")]
    pub graph_depth: Option<i32>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct UpdateMemoryParams {
    #[schemars(description = "Memory ID to update")]
    pub memory_id: String,
    #[schemars(description = "New content")]
    pub new_content: String,
    #[schemars(description = "User identifier")]
    pub user_id: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct GetMemoryGraphParams {
    #[schemars(description = "User identifier")]
    pub user_id: String,
    #[schemars(description = "Optional starting point memory ID")]
    pub memory_id: Option<String>,
    #[schemars(description = "Traversal depth (default: 2)")]
    pub depth: Option<i32>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct SearchByConceptParams {
    #[schemars(description = "Search query (semantic matching)")]
    pub query: String,
    #[schemars(description = "User identifier")]
    pub user_id: String,
    #[schemars(description = "Concept type: 'skill', 'preference', 'goal', 'fact', 'opinion', 'experience', 'achievement'")]
    pub concept_type: Option<String>,
    #[schemars(description = "Comma-separated tags to filter by")]
    pub tags: Option<String>,
    #[schemars(description = "Search mode")]
    pub mode: Option<String>,
    #[schemars(description = "Max results (default: 10)")]
    pub limit: Option<i32>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct SearchReasoningChainParams {
    #[schemars(description = "Search query")]
    pub query: String,
    #[schemars(description = "User identifier")]
    pub user_id: String,
    #[schemars(description = "Chain mode: 'causal' (BECAUSE), 'forward' (IMPLIES), 'both', 'deep'")]
    pub chain_mode: Option<String>,
    #[schemars(description = "Maximum chain depth (default: 5)")]
    pub max_depth: Option<i32>,
    #[schemars(description = "Number of seed memories")]
    pub limit: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct MemorySummaryArgs {
    #[schemars(description = "User identifier")]
    pub user_id: String,
    #[schemars(description = "Optional topic to focus on")]
    pub topic: Option<String>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct StartThinkingParams {
    #[schemars(description = "Unique session identifier")]
    pub session_id: String,
    #[schemars(description = "Initial thought or question to reason about")]
    pub initial_thought: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct AddThoughtParams {
    #[schemars(description = "Session identifier")]
    pub session_id: String,
    #[schemars(description = "Thought content")]
    pub content: String,
    #[schemars(description = "Thought type: 'reasoning', 'hypothesis', 'observation', 'question'")]
    pub thought_type: Option<String>,
    #[schemars(description = "Parent thought index (from previous response)")]
    pub parent_idx: Option<u32>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ThinkRecallParams {
    #[schemars(description = "Session identifier")]
    pub session_id: String,
    #[schemars(description = "Query to search in main memory")]
    pub query: String,
    #[schemars(description = "Parent thought index to attach recalled facts to")]
    pub parent_idx: u32,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ThinkConcludeParams {
    #[schemars(description = "Session identifier")]
    pub session_id: String,
    #[schemars(description = "Conclusion content")]
    pub conclusion: String,
    #[schemars(description = "Supporting thought indices")]
    pub supporting_idx: Option<Vec<u32>>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ThinkCommitParams {
    #[schemars(description = "Session identifier")]
    pub session_id: String,
    #[schemars(description = "User identifier for storing in main memory")]
    pub user_id: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ThinkDiscardParams {
    #[schemars(description = "Session identifier")]
    pub session_id: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ThinkStatusParams {
    #[schemars(description = "Session identifier")]
    pub session_id: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct SearchIncompleteThoughtsParams {
    #[schemars(description = "Maximum number of results (default: 5)")]
    pub limit: Option<i32>,
}

