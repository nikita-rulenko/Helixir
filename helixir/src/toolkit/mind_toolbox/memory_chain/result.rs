use serde::{Deserialize, Serialize};
use std::collections::HashSet;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainNode {
    pub memory_id: String,
    pub content: String,
    pub memory_type: Option<String>,
    pub depth: u32,
    pub relation_type: Option<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryChain {
    pub seed_memory_id: String,
    pub chain_type: String, 
    pub nodes: Vec<ChainNode>,
    pub total_depth: u32,
}

impl MemoryChain {
    pub fn new(seed_memory_id: String, chain_type: String) -> Self {
        Self {
            seed_memory_id,
            chain_type,
            nodes: Vec::new(),
            total_depth: 0,
        }
    }
    
    pub fn add_node(&mut self, node: ChainNode) {
        self.total_depth = self.total_depth.max(node.depth);
        self.nodes.push(node);
    }
    
    pub fn get_all_memories(&self) -> &[ChainNode] {
        &self.nodes
    }
    
    
    pub fn get_reasoning_trail(&self) -> String {
        let mut trail = String::new();
        for node in &self.nodes {
            let relation = node.relation_type.as_deref().unwrap_or("ROOT");
            let content_preview = if node.content.len() > 80 {
                format!("{}...", crate::safe_truncate(&node.content, 80))
            } else {
                node.content.clone()
            };
            trail.push_str(&format!("[{}] {} -> {}\n", node.depth, relation, content_preview));
        }
        trail
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainSearchResult {
    pub query: String,
    pub chains: Vec<MemoryChain>,
    
    
    pub total_memories: usize,
    pub total_chains: usize,
    pub deepest_chain: u32,
    
    
    pub memories: Vec<serde_json::Value>,
}

impl ChainSearchResult {
    pub fn new(query: String, chains: Vec<MemoryChain>) -> Self {
        let mut result = Self {
            query,
            chains,
            total_memories: 0,
            total_chains: 0,
            deepest_chain: 0,
            memories: Vec::new(),
        };
        result.calculate_stats();
        result
    }
    
    pub fn empty(query: String) -> Self {
        Self {
            query,
            chains: Vec::new(),
            total_memories: 0,
            total_chains: 0,
            deepest_chain: 0,
            memories: Vec::new(),
        }
    }
    
    fn calculate_stats(&mut self) {
        self.total_chains = self.chains.len();
        self.total_memories = self.chains.iter().map(|c| c.nodes.len()).sum();
        self.deepest_chain = self.chains.iter().map(|c| c.total_depth).max().unwrap_or(0);
        
        
        let mut seen = HashSet::new();
        for chain in &self.chains {
            for node in &chain.nodes {
                if seen.insert(node.memory_id.clone()) {
                    self.memories.push(serde_json::json!({
                        "memory_id": node.memory_id,
                        "content": node.content,
                        "memory_type": node.memory_type,
                        "chain_depth": node.depth,
                        "relation_type": node.relation_type,
                    }));
                }
            }
        }
    }
    
    
    pub fn get_reasoning_trails(&self) -> String {
        let mut all_trails = String::new();
        for (i, chain) in self.chains.iter().enumerate() {
            all_trails.push_str(&format!("=== Chain {} (Type: {}) ===\n", i + 1, chain.chain_type));
            all_trails.push_str(&chain.get_reasoning_trail());
            if i < self.chains.len() - 1 {
                all_trails.push('\n');
            }
        }
        all_trails
    }
}