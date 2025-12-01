use petgraph::stable_graph::NodeIndex;
use std::time::Instant;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum ThoughtType {
    Initial,
    Reasoning,
    Recall,
    Hypothesis,
    Conclusion,
    Question,
    Observation,
}

impl std::fmt::Display for ThoughtType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThoughtType::Initial => write!(f, "initial"),
            ThoughtType::Reasoning => write!(f, "reasoning"),
            ThoughtType::Recall => write!(f, "recall"),
            ThoughtType::Hypothesis => write!(f, "hypothesis"),
            ThoughtType::Conclusion => write!(f, "conclusion"),
            ThoughtType::Question => write!(f, "question"),
            ThoughtType::Observation => write!(f, "observation"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Thought {
    pub id: String,
    pub content: String,
    pub thought_type: ThoughtType,
    pub certainty: f32,
    pub timestamp: Instant,
    pub depth: usize,
    pub source_memory_id: Option<String>,
}

impl Thought {
    pub fn new(content: &str, thought_type: ThoughtType, depth: usize) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            content: content.to_string(),
            thought_type,
            certainty: 0.5,
            timestamp: Instant::now(),
            depth,
            source_memory_id: None,
        }
    }

    pub fn with_certainty(mut self, certainty: f32) -> Self {
        self.certainty = certainty.clamp(0.0, 1.0);
        self
    }

    pub fn with_source(mut self, memory_id: &str) -> Self {
        self.source_memory_id = Some(memory_id.to_string());
        self
    }

    pub fn is_conclusion(&self) -> bool {
        self.thought_type == ThoughtType::Conclusion
    }

    pub fn is_recall(&self) -> bool {
        self.thought_type == ThoughtType::Recall
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ThoughtEdge {
    LeadsTo,
    Recalled,
    Supports,
    Contradicts,
    Implies,
    Because,
    Refines,
    Questions,
}

impl std::fmt::Display for ThoughtEdge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThoughtEdge::LeadsTo => write!(f, "leads_to"),
            ThoughtEdge::Recalled => write!(f, "recalled"),
            ThoughtEdge::Supports => write!(f, "supports"),
            ThoughtEdge::Contradicts => write!(f, "contradicts"),
            ThoughtEdge::Implies => write!(f, "implies"),
            ThoughtEdge::Because => write!(f, "because"),
            ThoughtEdge::Refines => write!(f, "refines"),
            ThoughtEdge::Questions => write!(f, "questions"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScratchEntityType {
    Person,
    Organization,
    Location,
    Concept,
    Object,
    Action,
    Event,
    Technology,
    Other,
}

impl std::fmt::Display for ScratchEntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScratchEntityType::Person => write!(f, "person"),
            ScratchEntityType::Organization => write!(f, "organization"),
            ScratchEntityType::Location => write!(f, "location"),
            ScratchEntityType::Concept => write!(f, "concept"),
            ScratchEntityType::Object => write!(f, "object"),
            ScratchEntityType::Action => write!(f, "action"),
            ScratchEntityType::Event => write!(f, "event"),
            ScratchEntityType::Technology => write!(f, "technology"),
            ScratchEntityType::Other => write!(f, "other"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScratchEntity {
    pub id: String,
    pub name: String,
    pub entity_type: ScratchEntityType,
    pub mentions: Vec<NodeIndex>,
    pub attributes: HashMap<String, String>,
}

impl ScratchEntity {
    pub fn new(name: &str, entity_type: ScratchEntityType) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            entity_type,
            mentions: Vec::new(),
            attributes: HashMap::new(),
        }
    }

    pub fn add_mention(&mut self, thought_idx: NodeIndex) {
        if !self.mentions.contains(&thought_idx) {
            self.mentions.push(thought_idx);
        }
    }

    pub fn set_attribute(&mut self, key: &str, value: &str) {
        self.attributes.insert(key.to_string(), value.to_string());
    }

    pub fn mention_count(&self) -> usize {
        self.mentions.len()
    }
}

#[derive(Debug, Clone)]
pub struct ScratchConcept {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    pub related_thoughts: Vec<NodeIndex>,
}

impl ScratchConcept {
    pub fn new(name: &str, parent: Option<&str>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            parent: parent.map(|s| s.to_string()),
            related_thoughts: Vec::new(),
        }
    }

    pub fn link_thought(&mut self, thought_idx: NodeIndex) {
        if !self.related_thoughts.contains(&thought_idx) {
            self.related_thoughts.push(thought_idx);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionStatus {
    Thinking,
    NeedsRecall,
    Decided,
    TimedOut,
    Overflow,
    Committed,
    Discarded,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionStatus::Thinking => write!(f, "thinking"),
            SessionStatus::NeedsRecall => write!(f, "needs_recall"),
            SessionStatus::Decided => write!(f, "decided"),
            SessionStatus::TimedOut => write!(f, "timed_out"),
            SessionStatus::Overflow => write!(f, "overflow"),
            SessionStatus::Committed => write!(f, "committed"),
            SessionStatus::Discarded => write!(f, "discarded"),
        }
    }
}

#[derive(Debug)]
pub enum FastThinkError {
    SessionNotFound,
    SessionAlreadyExists,
    Timeout,
    TooManyThoughts,
    TooManyEntities,
    TooManyConcepts,
    TooDeep,
    NoConclusion,
    InvalidState(String),
    RecallFailed(String),
    CommitFailed(String),
    ThoughtNotFound,
    EntityNotFound,
}

impl std::fmt::Display for FastThinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FastThinkError::SessionNotFound => write!(f, "Session not found"),
            FastThinkError::SessionAlreadyExists => write!(f, "Session already exists"),
            FastThinkError::Timeout => write!(f, "Thinking timeout exceeded"),
            FastThinkError::TooManyThoughts => write!(f, "Too many thoughts in session"),
            FastThinkError::TooManyEntities => write!(f, "Too many entities extracted"),
            FastThinkError::TooManyConcepts => write!(f, "Too many concepts mapped"),
            FastThinkError::TooDeep => write!(f, "Thought chain too deep"),
            FastThinkError::NoConclusion => write!(f, "No conclusion reached"),
            FastThinkError::InvalidState(s) => write!(f, "Invalid state: {}", s),
            FastThinkError::RecallFailed(s) => write!(f, "Recall failed: {}", s),
            FastThinkError::CommitFailed(s) => write!(f, "Commit failed: {}", s),
            FastThinkError::ThoughtNotFound => write!(f, "Thought not found"),
            FastThinkError::EntityNotFound => write!(f, "Entity not found"),
        }
    }
}

impl std::error::Error for FastThinkError {}

