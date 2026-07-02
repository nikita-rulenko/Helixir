use petgraph::Direction;
use petgraph::stable_graph::{EdgeIndex, NodeIndex, StableDiGraph};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;
use std::time::Instant;

use super::limits::FastThinkLimits;
use super::models::*;

pub struct ThinkingSession {
    pub id: String,
    pub graph: StableDiGraph<Thought, ThoughtEdge>,
    pub entities: HashMap<String, ScratchEntity>,
    pub concepts: HashMap<String, ScratchConcept>,
    pub thought_to_concepts: HashMap<NodeIndex, Vec<String>>,
    pub thought_to_entities: HashMap<NodeIndex, Vec<String>>,
    pub started_at: Instant,
    pub last_activity: Instant,
    pub current_depth: usize,
    pub status: SessionStatus,
    /// Last user_id seen by think_recall/commit on this session — the best
    /// guess of the owner when a shutdown auto-save must pick one.
    pub owner_hint: Option<String>,
    root_thought: Option<NodeIndex>,
}

impl ThinkingSession {
    pub fn new(session_id: &str) -> Self {
        Self {
            id: session_id.to_string(),
            graph: StableDiGraph::new(),
            entities: HashMap::new(),
            concepts: HashMap::new(),
            thought_to_concepts: HashMap::new(),
            thought_to_entities: HashMap::new(),
            started_at: Instant::now(),
            last_activity: Instant::now(),
            current_depth: 0,
            status: SessionStatus::Thinking,
            owner_hint: None,
            root_thought: None,
        }
    }

    pub fn add_thought(
        &mut self,
        content: &str,
        thought_type: ThoughtType,
        parent: Option<NodeIndex>,
        edge_type: Option<ThoughtEdge>,
        limits: &FastThinkLimits,
    ) -> Result<NodeIndex, FastThinkError> {
        if self.started_at.elapsed() > limits.thinking_timeout {
            self.status = SessionStatus::TimedOut;
            return Err(FastThinkError::Timeout);
        }

        if self.graph.node_count() >= limits.max_thoughts {
            self.status = SessionStatus::Overflow;
            return Err(FastThinkError::TooManyThoughts);
        }

        let depth = parent
            .and_then(|p| self.graph.node_weight(p))
            .map(|t| t.depth + 1)
            .unwrap_or(0);

        if depth > limits.max_depth {
            return Err(FastThinkError::TooDeep);
        }

        let thought = Thought::new(content, thought_type, depth);
        let node = self.graph.add_node(thought);

        if let Some(parent_idx) = parent {
            let edge = edge_type.unwrap_or(ThoughtEdge::LeadsTo);
            self.graph.add_edge(parent_idx, node, edge);
        }

        if self.root_thought.is_none() {
            self.root_thought = Some(node);
        }

        self.last_activity = Instant::now();
        self.current_depth = self.current_depth.max(depth);

        Ok(node)
    }

    pub fn add_recalled_thought(
        &mut self,
        content: &str,
        source_memory_id: &str,
        certainty: f32,
        parent: NodeIndex,
        limits: &FastThinkLimits,
    ) -> Result<NodeIndex, FastThinkError> {
        let node = self.add_thought(
            content,
            ThoughtType::Recall,
            Some(parent),
            Some(ThoughtEdge::Recalled),
            limits,
        )?;

        if let Some(thought) = self.graph.node_weight_mut(node) {
            thought.source_memory_id = Some(source_memory_id.to_string());
            thought.certainty = certainty;
        }

        Ok(node)
    }

    pub fn add_conclusion(
        &mut self,
        content: &str,
        supporting_thoughts: &[NodeIndex],
        limits: &FastThinkLimits,
    ) -> Result<NodeIndex, FastThinkError> {
        let parent = supporting_thoughts.first().copied();

        let node = self.add_thought(
            content,
            ThoughtType::Conclusion,
            parent,
            Some(ThoughtEdge::LeadsTo),
            limits,
        )?;

        for &supporting in supporting_thoughts.iter().skip(1) {
            self.graph.add_edge(supporting, node, ThoughtEdge::Supports);
        }

        self.status = SessionStatus::Decided;
        Ok(node)
    }

    pub fn link_thoughts(
        &mut self,
        from: NodeIndex,
        to: NodeIndex,
        edge_type: ThoughtEdge,
    ) -> Result<EdgeIndex, FastThinkError> {
        if self.graph.node_weight(from).is_none() {
            return Err(FastThinkError::ThoughtNotFound);
        }
        if self.graph.node_weight(to).is_none() {
            return Err(FastThinkError::ThoughtNotFound);
        }

        Ok(self.graph.add_edge(from, to, edge_type))
    }

    pub fn extract_entity(
        &mut self,
        thought_idx: NodeIndex,
        name: &str,
        entity_type: ScratchEntityType,
        limits: &FastThinkLimits,
    ) -> Result<String, FastThinkError> {
        if self.graph.node_weight(thought_idx).is_none() {
            return Err(FastThinkError::ThoughtNotFound);
        }

        let normalized_name = name.to_lowercase();

        if let Some(entity) = self.entities.get_mut(&normalized_name) {
            entity.add_mention(thought_idx);

            self.thought_to_entities
                .entry(thought_idx)
                .or_default()
                .push(normalized_name.clone());

            return Ok(entity.id.clone());
        }

        if self.entities.len() >= limits.max_entities {
            return Err(FastThinkError::TooManyEntities);
        }

        let mut entity = ScratchEntity::new(name, entity_type);
        entity.add_mention(thought_idx);
        let entity_id = entity.id.clone();

        self.entities.insert(normalized_name.clone(), entity);
        self.thought_to_entities
            .entry(thought_idx)
            .or_default()
            .push(normalized_name);

        Ok(entity_id)
    }

    pub fn map_to_concept(
        &mut self,
        thought_idx: NodeIndex,
        concept_name: &str,
        parent_concept: Option<&str>,
        limits: &FastThinkLimits,
    ) -> Result<String, FastThinkError> {
        if self.graph.node_weight(thought_idx).is_none() {
            return Err(FastThinkError::ThoughtNotFound);
        }

        let normalized_name = concept_name.to_lowercase();

        if let Some(concept) = self.concepts.get_mut(&normalized_name) {
            concept.link_thought(thought_idx);

            self.thought_to_concepts
                .entry(thought_idx)
                .or_default()
                .push(normalized_name.clone());

            return Ok(concept.id.clone());
        }

        if self.concepts.len() >= limits.max_concepts {
            return Err(FastThinkError::TooManyConcepts);
        }

        let mut concept = ScratchConcept::new(concept_name, parent_concept);
        concept.link_thought(thought_idx);
        let concept_id = concept.id.clone();

        self.concepts.insert(normalized_name.clone(), concept);
        self.thought_to_concepts
            .entry(thought_idx)
            .or_default()
            .push(normalized_name);

        Ok(concept_id)
    }

    pub fn get_thought(&self, idx: NodeIndex) -> Option<&Thought> {
        self.graph.node_weight(idx)
    }

    pub fn get_thought_mut(&mut self, idx: NodeIndex) -> Option<&mut Thought> {
        self.graph.node_weight_mut(idx)
    }

    pub fn get_conclusions(&self) -> Vec<(NodeIndex, &Thought)> {
        self.graph
            .node_indices()
            .filter_map(|idx| {
                self.graph.node_weight(idx).and_then(|t| {
                    if t.is_conclusion() {
                        Some((idx, t))
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    pub fn get_children(&self, idx: NodeIndex) -> Vec<(NodeIndex, &ThoughtEdge)> {
        self.graph
            .edges_directed(idx, Direction::Outgoing)
            .map(|e| (e.target(), e.weight()))
            .collect()
    }

    pub fn get_parents(&self, idx: NodeIndex) -> Vec<(NodeIndex, &ThoughtEdge)> {
        self.graph
            .edges_directed(idx, Direction::Incoming)
            .map(|e| (e.source(), e.weight()))
            .collect()
    }

    pub fn get_chain_to_root(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        let mut chain = vec![idx];
        let mut current = idx;

        while let Some((parent, _)) = self.get_parents(current).first() {
            if chain.contains(parent) {
                break;
            }
            chain.push(*parent);
            current = *parent;
        }

        chain.reverse();
        chain
    }

    pub fn get_entities_for_thought(&self, idx: NodeIndex) -> Vec<&ScratchEntity> {
        self.thought_to_entities
            .get(&idx)
            .map(|names| {
                names
                    .iter()
                    .filter_map(|name| self.entities.get(name))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn get_concepts_for_thought(&self, idx: NodeIndex) -> Vec<&ScratchConcept> {
        self.thought_to_concepts
            .get(&idx)
            .map(|names| {
                names
                    .iter()
                    .filter_map(|name| self.concepts.get(name))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn thought_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn concept_count(&self) -> usize {
        self.concepts.len()
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.status,
            SessionStatus::Thinking | SessionStatus::NeedsRecall
        )
    }

    pub fn root(&self) -> Option<NodeIndex> {
        self.root_thought
    }

    pub fn build_conclusion_content(&self) -> String {
        let conclusions = self.get_conclusions();

        if conclusions.is_empty() {
            return String::new();
        }

        conclusions
            .iter()
            .map(|(_, t)| t.content.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn get_supporting_evidence(&self) -> Vec<String> {
        self.graph
            .node_indices()
            .filter_map(|idx| {
                let thought = self.graph.node_weight(idx)?;
                if thought.is_recall() {
                    Some(format!(
                        "[{}] {}",
                        thought.source_memory_id.as_deref().unwrap_or("unknown"),
                        thought.content
                    ))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Memory ids of recall nodes the conclusion(s) actually REST on: direct
    /// supporters of a conclusion, their ancestors, and recalls hanging off
    /// those nodes. `get_supporting_memory_ids` (every recall in the session)
    /// over-attributed — broad exploratory recalls became SUPPORTS provenance
    /// for conclusions they never touched.
    pub fn get_conclusion_evidence_ids(&self) -> Vec<String> {
        use std::collections::HashSet;

        let mut evidence: Vec<String> = Vec::new();
        let mut pushed: HashSet<String> = HashSet::new();
        let mut seen: HashSet<NodeIndex> = HashSet::new();
        let push_recall = |t: &Thought, pushed: &mut HashSet<String>, out: &mut Vec<String>| {
            if t.is_recall() {
                if let Some(id) = &t.source_memory_id {
                    if pushed.insert(id.clone()) {
                        out.push(id.clone());
                    }
                }
            }
        };

        for (cidx, _) in self.get_conclusions() {
            let mut stack: Vec<NodeIndex> = self
                .graph
                .neighbors_directed(cidx, Direction::Incoming)
                .collect();
            while let Some(idx) = stack.pop() {
                if !seen.insert(idx) {
                    continue;
                }
                if let Some(t) = self.graph.node_weight(idx) {
                    push_recall(t, &mut pushed, &mut evidence);
                }
                // Recalls attached UNDER a supporting thought count as its
                // evidence (think_recall parents them to the thought).
                for child in self.graph.neighbors_directed(idx, Direction::Outgoing) {
                    if let Some(t) = self.graph.node_weight(child) {
                        push_recall(t, &mut pushed, &mut evidence);
                    }
                }
                stack.extend(self.graph.neighbors_directed(idx, Direction::Incoming));
            }
        }
        evidence
    }

    pub fn get_supporting_memory_ids(&self) -> Vec<String> {
        self.graph
            .node_indices()
            .filter_map(|idx| {
                let thought = self.graph.node_weight(idx)?;
                if thought.is_recall() {
                    thought.source_memory_id.clone()
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod evidence_tests {
    use super::*;
    use crate::toolkit::fast_think::limits::FastThinkLimits;
    use crate::toolkit::fast_think::models::{ThoughtEdge, ThoughtType};

    /// Only recalls in the conclusion's supporting subtree are evidence — a
    /// broad exploratory recall on an unrelated branch must NOT become
    /// SUPPORTS provenance (the inflation observed live: ~105 edges).
    #[test]
    fn evidence_excludes_unrelated_recalls() {
        let limits = FastThinkLimits::default();
        let mut s = ThinkingSession::new("t");
        let root = s
            .add_thought("pick a policy", ThoughtType::Initial, None, None, &limits)
            .unwrap();
        let obs = s
            .add_thought(
                "outages are short",
                ThoughtType::Observation,
                Some(root),
                Some(ThoughtEdge::LeadsTo),
                &limits,
            )
            .unwrap();
        let used = s
            .add_recalled_thought("queue fact", "mem_used", 0.9, obs, &limits)
            .unwrap();
        let _ = used;
        // Unrelated branch with its own recall.
        let side = s
            .add_thought(
                "tangent",
                ThoughtType::Question,
                Some(root),
                Some(ThoughtEdge::LeadsTo),
                &limits,
            )
            .unwrap();
        s.add_recalled_thought("noise fact", "mem_noise", 0.9, side, &limits)
            .unwrap();

        s.add_conclusion("backoff with jitter", &[obs], &limits)
            .unwrap();

        let ev = s.get_conclusion_evidence_ids();
        assert!(
            ev.contains(&"mem_used".to_string()),
            "supporting recall kept: {ev:?}"
        );
        assert!(
            !ev.contains(&"mem_noise".to_string()),
            "unrelated recall must be excluded: {ev:?}"
        );
    }
}
