

use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SearchMode {
    
    #[default]
    Recent,
    
    Contextual,
    
    Deep,
    
    Full,
}

impl SearchMode {
    
    #[must_use]
    pub fn get_defaults(&self) -> SearchModeDefaults {
        match self {
            Self::Recent => SearchModeDefaults {
                max_results: 10,
                graph_depth: 1,
                temporal_days: Some(0.167), 
                vector_weight: 0.7,
                bm25_weight: 0.3,
                include_relations: true,
                cost_estimate: 1.0,
                use_smart_traversal: true,
                vector_top_k: 5,
                min_vector_score: 0.6,
                min_combined_score: 0.4,
            },
            Self::Contextual => SearchModeDefaults {
                max_results: 20,
                graph_depth: 2,
                temporal_days: Some(30.0),
                vector_weight: 0.6,
                bm25_weight: 0.4,
                include_relations: true,
                cost_estimate: 2.5,
                use_smart_traversal: true,
                vector_top_k: 10,
                min_vector_score: 0.5,
                min_combined_score: 0.3,
            },
            Self::Deep => SearchModeDefaults {
                max_results: 50,
                graph_depth: 3,
                temporal_days: Some(90.0),
                vector_weight: 0.5,
                bm25_weight: 0.5,
                include_relations: true,
                cost_estimate: 5.0,
                use_smart_traversal: true,
                vector_top_k: 15,
                min_vector_score: 0.4,
                min_combined_score: 0.25,
            },
            Self::Full => SearchModeDefaults {
                max_results: 100,
                graph_depth: 4,
                temporal_days: None, 
                vector_weight: 0.5,
                bm25_weight: 0.5,
                include_relations: true,
                cost_estimate: 10.0,
                use_smart_traversal: false, 
                vector_top_k: 0,
                min_vector_score: 0.0,
                min_combined_score: 0.0,
            },
        }
    }

    
    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            Self::Recent => "Fast recent memories (4 hours) + nearest graph",
            Self::Contextual => "Balanced search (30 days) + moderate graph",
            Self::Deep => "Deep search (90 days) + extensive graph",
            Self::Full => "Complete history + full graph traversal",
        }
    }

    
    #[must_use]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "recent" => Self::Recent,
            "contextual" => Self::Contextual,
            "deep" => Self::Deep,
            "full" => Self::Full,
            _ => Self::Recent, 
        }
    }
}

impl From<&str> for SearchMode {
    fn from(s: &str) -> Self {
        Self::from_str(s)
    }
}

impl From<String> for SearchMode {
    fn from(s: String) -> Self {
        Self::from_str(&s)
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchModeDefaults {
    
    pub max_results: usize,
    
    pub graph_depth: usize,
    
    pub temporal_days: Option<f64>,
    
    pub vector_weight: f64,
    
    pub bm25_weight: f64,
    
    pub include_relations: bool,
    
    pub cost_estimate: f64,
    
    pub use_smart_traversal: bool,
    
    pub vector_top_k: usize,
    
    pub min_vector_score: f64,
    
    pub min_combined_score: f64,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCostEstimate {
    
    pub base_cost: f64,
    
    pub result_cost: f64,
    
    pub total_cost: usize,
    
    pub cost_tier: String,
    
    pub num_results: usize,
    
    pub graph_depth: usize,
    
    pub mode: String,
}


#[must_use]
pub fn estimate_token_cost(
    mode: SearchMode,
    num_results: Option<usize>,
    graph_depth: Option<usize>,
) -> TokenCostEstimate {
    let defaults = mode.get_defaults();

    let results = num_results.unwrap_or(defaults.max_results);
    let depth = graph_depth.unwrap_or(defaults.graph_depth);

    
    const BASE_COST_PER_MEMORY: f64 = 200.0;
    const RELATION_COST: f64 = 50.0;

    let graph_multiplier = 1.0 + (depth as f64 * 2.0);
    let result_cost = BASE_COST_PER_MEMORY + (RELATION_COST * depth as f64 * 2.0);
    let total_cost = result_cost * results as f64 * graph_multiplier;

    let tier = if total_cost < 5000.0 {
        "low"
    } else if total_cost < 15000.0 {
        "medium"
    } else if total_cost < 50000.0 {
        "high"
    } else {
        "very_high"
    };

    TokenCostEstimate {
        base_cost: result_cost,
        result_cost: result_cost * graph_multiplier,
        total_cost: total_cost as usize,
        cost_tier: tier.to_string(),
        num_results: results,
        graph_depth: depth,
        mode: format!("{:?}", mode),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_mode_from_str() {
        assert_eq!(SearchMode::from_str("recent"), SearchMode::Recent);
        assert_eq!(SearchMode::from_str("CONTEXTUAL"), SearchMode::Contextual);
        assert_eq!(SearchMode::from_str("Deep"), SearchMode::Deep);
        assert_eq!(SearchMode::from_str("full"), SearchMode::Full);
        assert_eq!(SearchMode::from_str("unknown"), SearchMode::Recent);
    }

    #[test]
    fn test_search_mode_defaults() {
        let recent = SearchMode::Recent.get_defaults();
        assert_eq!(recent.max_results, 10);
        assert_eq!(recent.graph_depth, 1);

        let full = SearchMode::Full.get_defaults();
        assert_eq!(full.max_results, 100);
        assert!(full.temporal_days.is_none());
    }

    #[test]
    fn test_token_cost_estimate() {
        let estimate = estimate_token_cost(SearchMode::Recent, None, None);
        assert_eq!(estimate.cost_tier, "low");

        let estimate = estimate_token_cost(SearchMode::Full, Some(100), Some(4));
        assert!(estimate.total_cost > 10000);
    }
}
