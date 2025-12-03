use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChainDirection {
    Forward,   
    Backward,  
    Both,      
}

impl Default for ChainDirection {
    fn default() -> Self {
        Self::Both
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryChainConfig {
    
    pub max_depth: u32,
    
    pub direction: ChainDirection,
    
    pub relation_types: Vec<String>,
    
    pub min_confidence: f64,
    
    pub include_contradictions: bool,
}

impl Default for MemoryChainConfig {
    fn default() -> Self {
        Self {
            max_depth: 5,
            direction: ChainDirection::Both,
            relation_types: vec![
                "IMPLIES".to_string(),
                "BECAUSE".to_string(),
                "CONTRADICTS".to_string(),
            ],
            min_confidence: 0.5,
            include_contradictions: true,
        }
    }
}

impl MemoryChainConfig {
    
    
    pub fn causal_only() -> Self {
        Self {
            max_depth: 5,
            direction: ChainDirection::Backward,
            relation_types: vec!["BECAUSE".to_string()],
            min_confidence: 0.5,
            include_contradictions: false,
        }
    }
    
    
    pub fn implications_only() -> Self {
        Self {
            max_depth: 5,
            direction: ChainDirection::Forward,
            relation_types: vec!["IMPLIES".to_string()],
            min_confidence: 0.5,
            include_contradictions: false,
        }
    }
    
    
    pub fn deep_context() -> Self {
        Self {
            max_depth: 7,
            direction: ChainDirection::Both,
            relation_types: vec![
                "IMPLIES".to_string(),
                "BECAUSE".to_string(),
                "CONTRADICTS".to_string(),
                "SUPPORTS".to_string(),
                "REFUTES".to_string(),
            ],
            min_confidence: 0.3,
            include_contradictions: true,
        }
    }
}