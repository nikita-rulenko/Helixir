use std::time::Duration;

#[derive(Debug, Clone)]
pub struct FastThinkLimits {
    pub max_thoughts: usize,
    pub max_entities: usize,
    pub max_concepts: usize,
    pub max_depth: usize,
    pub thinking_timeout: Duration,
    pub session_ttl: Duration,
    pub max_recall_results: usize,
}

impl Default for FastThinkLimits {
    fn default() -> Self {
        Self {
            max_thoughts: 100,
            max_entities: 50,
            max_concepts: 30,
            max_depth: 10,
            thinking_timeout: Duration::from_secs(30),
            session_ttl: Duration::from_secs(300),
            max_recall_results: 5,
        }
    }
}

impl FastThinkLimits {
    pub fn relaxed() -> Self {
        Self {
            max_thoughts: 200,
            max_entities: 100,
            max_concepts: 50,
            max_depth: 15,
            thinking_timeout: Duration::from_secs(60),
            session_ttl: Duration::from_secs(600),
            max_recall_results: 10,
        }
    }

    pub fn strict() -> Self {
        Self {
            max_thoughts: 50,
            max_entities: 25,
            max_concepts: 15,
            max_depth: 5,
            thinking_timeout: Duration::from_secs(15),
            session_ttl: Duration::from_secs(120),
            max_recall_results: 3,
        }
    }

    pub fn with_max_thoughts(mut self, max: usize) -> Self {
        self.max_thoughts = max;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.thinking_timeout = timeout;
        self
    }

    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }
}

