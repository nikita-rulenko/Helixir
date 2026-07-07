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
    /// Score floor for recalls (see `FastThinkConfig::recall_min_score`).
    pub recall_min_score: f32,
    /// #90: relaxed floor for the one-shot fallback pass when the primary
    /// recall returns zero rows (see `FastThinkConfig::recall_fallback_min_score`).
    pub recall_fallback_min_score: f32,
    /// #90: hard cap on fallback rows; 0 disables the fallback.
    pub recall_fallback_max: usize,
    /// #78: recall stops this many slots short of the thought cap.
    pub conclude_reserve: usize,
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
            recall_min_score: 0.6,
            recall_fallback_min_score: 0.45,
            recall_fallback_max: 3,
            conclude_reserve: 2,
        }
    }
}

impl FastThinkLimits {
    /// Build from the layered [`crate::core::config::FastThinkConfig`].
    pub fn from_config(c: &crate::core::config::FastThinkConfig) -> Self {
        Self {
            max_thoughts: c.max_thoughts,
            max_entities: c.max_entities,
            max_concepts: c.max_concepts,
            max_depth: c.max_depth,
            thinking_timeout: Duration::from_secs(c.thinking_timeout_secs),
            session_ttl: Duration::from_secs(c.session_ttl_secs),
            max_recall_results: c.max_recall_results,
            recall_min_score: c.recall_min_score,
            recall_fallback_min_score: c.recall_fallback_min_score,
            recall_fallback_max: c.recall_fallback_max,
            conclude_reserve: c.conclude_reserve,
        }
    }

    pub fn relaxed() -> Self {
        Self {
            max_thoughts: 200,
            max_entities: 100,
            max_concepts: 50,
            max_depth: 15,
            thinking_timeout: Duration::from_secs(60),
            session_ttl: Duration::from_secs(600),
            max_recall_results: 10,
            recall_min_score: 0.6,
            recall_fallback_min_score: 0.45,
            recall_fallback_max: 3,
            conclude_reserve: 2,
        }
    }

    /// Limits tuned for MCP usage where inter-call latency eats into the
    /// thinking budget. Timeout is 90s (vs 30s default) because each tool call
    /// through MCP adds 3-8s of transport overhead.
    pub fn mcp() -> Self {
        Self {
            max_thoughts: 150,
            max_entities: 80,
            max_concepts: 40,
            max_depth: 12,
            thinking_timeout: Duration::from_secs(90),
            session_ttl: Duration::from_secs(600),
            max_recall_results: 8,
            recall_min_score: 0.6,
            recall_fallback_min_score: 0.45,
            recall_fallback_max: 3,
            conclude_reserve: 2,
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
            recall_min_score: 0.6,
            recall_fallback_min_score: 0.45,
            recall_fallback_max: 1,
            conclude_reserve: 2,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// #81: the recall floor must flow from the layered config into the
    /// limits the recall path actually consults — a silently-dropped mapping
    /// here would resurrect the unbounded-recall bug with green builds.
    #[test]
    fn from_config_carries_recall_knobs() {
        let mut c = crate::core::config::FastThinkConfig::default();
        c.max_recall_results = 3;
        c.recall_min_score = 0.42;
        c.recall_fallback_min_score = 0.33;
        c.recall_fallback_max = 2;
        let limits = FastThinkLimits::from_config(&c);
        assert_eq!(limits.max_recall_results, 3);
        assert!((limits.recall_min_score - 0.42).abs() < f32::EPSILON);
        assert!((limits.recall_fallback_min_score - 0.33).abs() < f32::EPSILON);
        assert_eq!(limits.recall_fallback_max, 2);
        assert_eq!(limits.conclude_reserve, 2, "#78 reserve flows from config");
    }

    /// #90: the fallback cap must stay SMALLER than the primary cap in every
    /// preset — weak evidence must never flood the tree the belt protects.
    #[test]
    fn fallback_cap_is_smaller_than_primary_everywhere() {
        for limits in [
            FastThinkLimits::default(),
            FastThinkLimits::relaxed(),
            FastThinkLimits::mcp(),
            FastThinkLimits::strict(),
        ] {
            assert!(limits.recall_fallback_max < limits.max_recall_results);
            assert!(limits.recall_fallback_min_score < limits.recall_min_score);
        }
    }
}
