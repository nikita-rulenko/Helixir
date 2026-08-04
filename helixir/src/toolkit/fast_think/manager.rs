use parking_lot::RwLock;
use petgraph::stable_graph::NodeIndex;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::limits::FastThinkLimits;
use super::models::*;
use super::session::ThinkingSession;
use crate::core::HelixirClient;

struct FastThinkRuntime {
    limits: FastThinkLimits,
    main_memory: Arc<HelixirClient>,
}

struct ManagedSession {
    state: ThinkingSession,
    runtime: Arc<FastThinkRuntime>,
}

impl std::ops::Deref for ManagedSession {
    type Target = ThinkingSession;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl std::ops::DerefMut for ManagedSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}

pub struct FastThinkManager {
    sessions: RwLock<HashMap<String, ManagedSession>>,
    current: arc_swap::ArcSwap<FastThinkRuntime>,
}

mod persistence;
mod session_ops;

#[derive(Debug, Clone)]
pub struct CommitResult {
    pub memory_id: String,
    pub thoughts_processed: usize,
    pub entities_extracted: usize,
    pub concepts_mapped: usize,
    pub elapsed: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct DiscardResult {
    pub thoughts_discarded: usize,
    pub elapsed: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub status: SessionStatus,
    pub thought_count: usize,
    pub entity_count: usize,
    pub concept_count: usize,
    pub current_depth: usize,
    pub elapsed: std::time::Duration,
    pub has_conclusion: bool,
}

#[derive(Debug, Clone)]
pub struct ThoughtInfo {
    pub id: String,
    pub content: String,
    pub thought_type: ThoughtType,
    pub certainty: f32,
    pub depth: usize,
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
