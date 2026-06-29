//! MCP tool input schemas. Free-choice parameters are real `enum`s (not free
//! strings) so the JSON Schema constrains callers and the agent can't pass an
//! invalid value; every field carries a description aimed at an agent calling
//! the tool cold.

use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

// ----------------------------------------------------------------------------
// Constrained value enums (serialise to the lowercase wire strings the engine
// expects; `as_str` converts back at the call site).
// ----------------------------------------------------------------------------

/// Recall breadth / time window for a search.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    /// Last ~4h — fastest, for "what was I just doing".
    Recent,
    /// Last ~30d — balanced default.
    Contextual,
    /// Last ~90d.
    Deep,
    /// Entire store, no time window — use when contextual returns nothing.
    Full,
}
impl SearchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::Contextual => "contextual",
            Self::Deep => "deep",
            Self::Full => "full",
        }
    }
}

/// Whose memories a search may see.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SearchScope {
    /// Only this user_id's own memories (default; safe).
    Personal,
    /// All users, ranked by consensus (collective tier only).
    Collective,
    /// Personal + collective combined, with controversy annotations.
    All,
}
impl SearchScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Collective => "collective",
            Self::All => "all",
        }
    }
}

/// The 8-type memory ontology (used both to classify and to filter).
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum OntologyType {
    Skill,
    Preference,
    Goal,
    Fact,
    Opinion,
    Experience,
    Achievement,
    Action,
}
impl OntologyType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Preference => "preference",
            Self::Goal => "goal",
            Self::Fact => "fact",
            Self::Opinion => "opinion",
            Self::Experience => "experience",
            Self::Achievement => "achievement",
            Self::Action => "action",
        }
    }
}

/// Direction to walk a reasoning chain.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChainMode {
    /// Backward along BECAUSE — "why is this so?".
    Causal,
    /// Forward along IMPLIES — "what does this lead to?".
    Forward,
    /// Both directions.
    Both,
    /// Both directions, walked deeper.
    Deep,
}
impl ChainMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Causal => "causal",
            Self::Forward => "forward",
            Self::Both => "both",
            Self::Deep => "deep",
        }
    }
}

/// Kind of thought node in a FastThink session.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ThoughtTypeArg {
    Reasoning,
    Hypothesis,
    Observation,
    Question,
}

// ----------------------------------------------------------------------------
// Tool parameter structs
// ----------------------------------------------------------------------------

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct AddMemoryParams {
    #[schemars(
        description = "Raw text to remember, in natural language. It is LLM-extracted into atomic typed facts (max 15 per call); pass a normal sentence or paragraph, not pre-formatted JSON. For very long input (>15 facts), split into chunks across calls."
    )]
    pub message: String,
    #[schemars(
        description = "Who this memory belongs to (e.g. 'claude', 'developer'). Use the SAME id consistently to build a coherent personal memory; it also scopes later searches."
    )]
    pub user_id: String,
    #[schemars(description = "Optional agent identifier that produced this memory.")]
    pub agent_id: Option<String>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct SearchMemoryParams {
    #[schemars(description = "Natural-language query; matched semantically + by keyword.")]
    pub query: String,
    #[schemars(
        description = "Whose memory to search (must match the user_id used on add_memory)."
    )]
    pub user_id: String,
    #[schemars(description = "Max results. Default depends on mode (~5–20).")]
    pub limit: Option<i32>,
    #[schemars(
        description = "Recall breadth. Default 'contextual' (~30d). If a query you expect to match returns nothing, retry with 'full'."
    )]
    pub mode: Option<SearchMode>,
    #[schemars(description = "Override the mode's time window, in days.")]
    pub temporal_days: Option<f64>,
    #[schemars(description = "Override graph-expansion depth (1–4).")]
    pub graph_depth: Option<i32>,
    #[schemars(
        description = "Whose memories to include. Default 'personal'. 'collective'/'all' require the collective tier and are silently downgraded to personal otherwise."
    )]
    pub scope: Option<SearchScope>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct UpdateMemoryParams {
    #[schemars(
        description = "Id of the memory to update (mem_… / raw_…), e.g. from a search result."
    )]
    pub memory_id: String,
    #[schemars(description = "Replacement content; the embedding and relations are regenerated.")]
    pub new_content: String,
    #[schemars(description = "Owner of the memory.")]
    pub user_id: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct GetMemoryGraphParams {
    #[schemars(description = "Whose graph to read.")]
    pub user_id: String,
    #[schemars(
        description = "Optional center node (mem_… / raw_…). Omit for the user's whole local graph; provide an id to get the ego-network around that memory."
    )]
    pub memory_id: Option<String>,
    #[schemars(description = "Hop radius around the center node. Default 2.")]
    pub depth: Option<i32>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct SearchByConceptParams {
    #[schemars(description = "Natural-language query (semantic matching).")]
    pub query: String,
    #[schemars(description = "Owner of the memories to search.")]
    pub user_id: String,
    #[schemars(
        description = "Restrict to one ontology type (e.g. only 'goal' or 'preference'). Omit to search all types."
    )]
    pub concept_type: Option<OntologyType>,
    #[schemars(description = "Comma-separated tags to additionally filter by.")]
    pub tags: Option<String>,
    #[schemars(description = "Recall breadth (see search_memory). Default 'contextual'.")]
    pub mode: Option<SearchMode>,
    #[schemars(description = "Max results. Default 10.")]
    pub limit: Option<i32>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct SearchReasoningChainParams {
    #[schemars(description = "What to find reasoning chains about (natural language).")]
    pub query: String,
    #[schemars(description = "Owner of the memories.")]
    pub user_id: String,
    #[schemars(description = "Which direction to walk the chain. Default 'both'.")]
    pub chain_mode: Option<ChainMode>,
    #[schemars(description = "Maximum chain depth (hops). Default 5.")]
    pub max_depth: Option<i32>,
    #[schemars(description = "How many seed memories to start chains from. Default ~5.")]
    pub limit: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct MemorySummaryArgs {
    #[schemars(description = "Owner of the memories to summarise.")]
    pub user_id: String,
    #[schemars(description = "Optional topic to focus the summary on.")]
    pub topic: Option<String>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct StartThinkingParams {
    #[schemars(
        description = "A unique id you choose for this reasoning session; reuse it on every think_* call until commit/discard."
    )]
    pub session_id: String,
    #[schemars(
        description = "The opening thought or question to reason about (becomes the root node)."
    )]
    pub initial_thought: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct AddThoughtParams {
    #[schemars(description = "The session_id from think_start.")]
    pub session_id: String,
    #[schemars(description = "The thought to add.")]
    pub content: String,
    #[schemars(description = "Kind of thought. Default 'reasoning'.")]
    pub thought_type: Option<ThoughtTypeArg>,
    #[schemars(
        description = "Index of the parent thought to attach under (from a previous response's thought_idx/root_thought_idx). Omit to attach to the root."
    )]
    pub parent_idx: Option<u32>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ThinkRecallParams {
    #[schemars(description = "The session_id from think_start.")]
    pub session_id: String,
    #[schemars(description = "Query to pull matching facts from MAIN memory into the session.")]
    pub query: String,
    #[schemars(description = "Index of the thought to attach the recalled facts under.")]
    pub parent_idx: u32,
    #[schemars(
        description = "Whose main memory to recall from. Omit to use the session's default scope."
    )]
    pub user_id: Option<String>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ThinkConcludeParams {
    #[schemars(description = "The session_id from think_start.")]
    pub session_id: String,
    #[schemars(description = "The conclusion of the reasoning (what to remember).")]
    pub conclusion: String,
    #[schemars(description = "Indices of the thoughts that support this conclusion.")]
    pub supporting_idx: Option<Vec<u32>>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ThinkCommitParams {
    #[schemars(description = "The session_id to commit.")]
    pub session_id: String,
    #[schemars(description = "Owner under whom the conclusion is stored in main memory.")]
    pub user_id: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ThinkDiscardParams {
    #[schemars(description = "The session_id to discard.")]
    pub session_id: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ThinkStatusParams {
    #[schemars(description = "The session_id to inspect.")]
    pub session_id: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct SearchIncompleteThoughtsParams {
    #[schemars(description = "Maximum number of results. Default 5.")]
    pub limit: Option<i32>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ListMemoriesParams {
    #[schemars(description = "Whose memories to list.")]
    pub user_id: String,
    #[schemars(description = "Max results. Default 100.")]
    pub limit: Option<i32>,
    #[schemars(description = "Optional: return only memories of this ontology type.")]
    pub memory_type: Option<OntologyType>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ListUsersParams {
    #[schemars(
        description = "Max identities to return, newest first. Default 50. The roster can be large, so this is a deliberately small window for orientation, not a full dump."
    )]
    pub limit: Option<i32>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ConnectMemoriesParams {
    #[schemars(
        description = "First anchor — a free-text description of concept A, OR an exact memory_id (mem_… / raw_…) to anchor precisely without searching."
    )]
    pub query_a: String,
    #[schemars(
        description = "Second anchor — a free-text description of concept B, OR an exact memory_id (mem_… / raw_…)."
    )]
    pub query_b: String,
    #[schemars(description = "Owner of the memories to route between.")]
    pub user_id: String,
    #[schemars(description = "Maximum total hops between the two anchors. Default 4.")]
    pub max_depth: Option<i32>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct GetAddStatusParams {
    #[schemars(description = "The pending_id returned by a buffered add_memory.")]
    pub pending_id: String,
}
