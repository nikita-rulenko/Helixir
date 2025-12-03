

use serde::{Deserialize, Serialize};
use strum::{EnumIter, IntoEnumIterator};


#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, EnumIter)]
#[repr(u8)]
pub enum HelixirLevel {
    
    Level0 = 0,
    
    Level1 = 1,
    
    Level2 = 2,
    
    Level3 = 3,
    
    Level4 = 4,
    
    Level5 = 5,
}

impl HelixirLevel {
    
    pub fn number(&self) -> u8 {
        *self as u8
    }

    
    pub fn from_number(n: u8) -> Option<Self> {
        match n {
            0 => Some(Self::Level0),
            1 => Some(Self::Level1),
            2 => Some(Self::Level2),
            3 => Some(Self::Level3),
            4 => Some(Self::Level4),
            5 => Some(Self::Level5),
            _ => None,
        }
    }

    
    pub fn levels_up_to(&self) -> Vec<Self> {
        (0..=self.number())
            .filter_map(Self::from_number)
            .collect()
    }

    
    pub fn depends_on(&self, other: &Self) -> bool {
        other.number() < self.number()
    }
}

impl std::fmt::Display for HelixirLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Level {}", self.number())
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelDefinition {
    
    pub level: HelixirLevel,
    
    pub name: String,
    
    pub description: String,
    
    pub schema_nodes: Vec<String>,
    
    pub schema_edges: Vec<String>,
    
    pub schema_extends: Vec<String>,
    
    pub queries: Vec<String>,
    
    pub dependencies: Vec<HelixirLevel>,
    
    pub notes: String,
}

impl LevelDefinition {
    
    pub fn new(level: HelixirLevel, name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            level,
            name: name.into(),
            description: description.into(),
            schema_nodes: Vec::new(),
            schema_edges: Vec::new(),
            schema_extends: Vec::new(),
            queries: Vec::new(),
            dependencies: Vec::new(),
            notes: String::new(),
        }
    }

    
    pub fn with_nodes(mut self, nodes: &[&str]) -> Self {
        self.schema_nodes = nodes.iter().map(|s| s.to_string()).collect();
        self
    }

    
    pub fn with_edges(mut self, edges: &[&str]) -> Self {
        self.schema_edges = edges.iter().map(|s| s.to_string()).collect();
        self
    }

    
    pub fn with_extends(mut self, extends: &[&str]) -> Self {
        self.schema_extends = extends.iter().map(|s| s.to_string()).collect();
        self
    }

    
    pub fn with_queries(mut self, queries: &[&str]) -> Self {
        self.queries = queries.iter().map(|s| s.to_string()).collect();
        self
    }

    
    pub fn with_dependencies(mut self, deps: &[HelixirLevel]) -> Self {
        self.dependencies = deps.to_vec();
        self
    }

    
    pub fn with_notes(mut self, notes: impl Into<String>) -> Self {
        self.notes = notes.into();
        self
    }
}


#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccumulatedSchema {
    pub nodes: Vec<String>,
    pub edges: Vec<String>,
    pub extends: Vec<String>,
}

impl AccumulatedSchema {
    
    pub fn add_level(&mut self, definition: &LevelDefinition) {
        self.nodes.extend(definition.schema_nodes.clone());
        self.edges.extend(definition.schema_edges.clone());
        self.extends.extend(definition.schema_extends.clone());
    }
}

