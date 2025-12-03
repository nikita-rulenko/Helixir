

use serde::{Deserialize, Serialize};
use strum::{EnumString, IntoStaticStr};


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, IntoStaticStr)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum MemoryOperation {
    
    Add,
    
    Update,
    
    Delete,
    
    Noop,
    
    Supersede,
    
    Contradict,
}

impl Default for MemoryOperation {
    fn default() -> Self {
        Self::Add
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryDecision {
    
    pub operation: MemoryOperation,

    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_memory_id: Option<String>,

    
    pub confidence: u8,

    
    pub reasoning: String,

    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merged_content: Option<String>,

    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes_memory_id: Option<String>,

    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contradicts_memory_id: Option<String>,

    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relates_to: Option<Vec<(String, String)>>,
}

impl MemoryDecision {
    
    pub fn add(confidence: u8, reasoning: impl Into<String>) -> Self {
        Self {
            operation: MemoryOperation::Add,
            target_memory_id: None,
            confidence,
            reasoning: reasoning.into(),
            merged_content: None,
            supersedes_memory_id: None,
            contradicts_memory_id: None,
            relates_to: None,
        }
    }

    
    pub fn noop(confidence: u8, reasoning: impl Into<String>) -> Self {
        Self {
            operation: MemoryOperation::Noop,
            target_memory_id: None,
            confidence,
            reasoning: reasoning.into(),
            merged_content: None,
            supersedes_memory_id: None,
            contradicts_memory_id: None,
            relates_to: None,
        }
    }

    
    pub fn update(
        target_id: impl Into<String>,
        merged_content: impl Into<String>,
        confidence: u8,
        reasoning: impl Into<String>,
    ) -> Self {
        Self {
            operation: MemoryOperation::Update,
            target_memory_id: Some(target_id.into()),
            confidence,
            reasoning: reasoning.into(),
            merged_content: Some(merged_content.into()),
            supersedes_memory_id: None,
            contradicts_memory_id: None,
            relates_to: None,
        }
    }

    
    pub fn supersede(
        supersedes_id: impl Into<String>,
        confidence: u8,
        reasoning: impl Into<String>,
    ) -> Self {
        Self {
            operation: MemoryOperation::Supersede,
            target_memory_id: None,
            confidence,
            reasoning: reasoning.into(),
            merged_content: None,
            supersedes_memory_id: Some(supersedes_id.into()),
            contradicts_memory_id: None,
            relates_to: None,
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarMemory {
    pub id: String,
    pub content: String,
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

