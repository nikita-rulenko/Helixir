

use super::models::{HelixirLevel, LevelDefinition};
use lazy_static::lazy_static;
use std::collections::HashMap;

lazy_static! {
    
    pub static ref LEVEL_0: LevelDefinition = LevelDefinition::new(
        HelixirLevel::Level0,
        "User Management",
        "Base level: user management"
    )
    .with_nodes(&["User"])
    .with_queries(&["addUser", "getUser"])
    .with_notes("Foundation. Without User, no memory.");

    
    pub static ref LEVEL_1: LevelDefinition = LevelDefinition::new(
        HelixirLevel::Level1,
        "Memory CRUD",
        "CRUD operations for memory and entities"
    )
    .with_nodes(&["Memory", "Entity"])
    .with_edges(&["OWNS", "MENTIONS"])
    .with_queries(&[
        "addMemory", "getMemory", "addEntity", "getEntity",
        "getMemoriesByUser", "getEntitiesByMemory"
    ])
    .with_dependencies(&[HelixirLevel::Level0])
    .with_notes("Framework foundation. Memory linked to User via OWNS.");

    
    pub static ref LEVEL_2: LevelDefinition = LevelDefinition::new(
        HelixirLevel::Level2,
        "Context & Search",
        "Contexts and basic memory search"
    )
    .with_nodes(&["Context"])
    .with_edges(&["IN_CONTEXT"])
    .with_queries(&[
        "addContext", "getContext", "getMemoriesByContext",
        "searchMemories", "searchMemoriesByKeyword"
    ])
    .with_dependencies(&[HelixirLevel::Level0, HelixirLevel::Level1])
    .with_notes("Contexts for memory grouping. Search without vectors.");

    
    pub static ref LEVEL_3: LevelDefinition = LevelDefinition::new(
        HelixirLevel::Level3,
        "Temporal & Update",
        "Temporal queries and UPDATE operations"
    )
    .with_queries(&[
        "updateMemory", "getRecentMemories",
        "searchRecentMemories", "getMemoriesByDateRange"
    ])
    .with_dependencies(&[HelixirLevel::Level0, HelixirLevel::Level1, HelixirLevel::Level2])
    .with_notes(
        "ISSUE: UPDATE in HelixQL requires two-query pattern:\n\
         1) WHERE to get internal ID\n\
         2) UPDATE with internal ID\n\
         Parameters in WHERE must be constants!"
    );

    
    pub static ref LEVEL_4: LevelDefinition = LevelDefinition::new(
        HelixirLevel::Level4,
        "Relations & Reasoning",
        "Reasoning relations (causality and conflicts)"
    )
    .with_nodes(&["ReasoningRelation"])
    .with_edges(&[
        "IMPLIES",      
        "BECAUSE",      
        "CONTRADICTS",  
        "SUPERSEDES",   
        "DERIVED_FROM", 
        "SUPPORTS",     
        "REFUTES"       
    ])
    .with_queries(&[
        "addMemoryRelation", "getMemoryRelations",
        "getReasoningChain", "detectConflicts", "getRelatedMemories"
    ])
    .with_dependencies(&[HelixirLevel::Level1])
    .with_notes(
        "FRAMEWORK CORE! This is reasoning - understanding WHY.\n\
         Relations are built between Memory nodes."
    );

    
    pub static ref LEVEL_5: LevelDefinition = LevelDefinition::new(
        HelixirLevel::Level5,
        "Vectors & Embeddings",
        "Vector search and embeddings"
    )
    .with_extends(&["Memory"])
    .with_queries(&[
        "addMemoryWithVector", "searchVectorMemories",
        "searchMemoriesByText", "searchHybrid"
    ])
    .with_dependencies(&[HelixirLevel::Level1])
    .with_notes(
        "ISSUE: Embed() in queries was unstable.\n\
         SOLUTION: Client-side embeddings via LLM client.\n\
         Schema extends Memory, adding vector and embedding_model fields."
    );

    
    pub static ref LEVELS: HashMap<HelixirLevel, &'static LevelDefinition> = {
        let mut map = HashMap::new();
        map.insert(HelixirLevel::Level0, &*LEVEL_0);
        map.insert(HelixirLevel::Level1, &*LEVEL_1);
        map.insert(HelixirLevel::Level2, &*LEVEL_2);
        map.insert(HelixirLevel::Level3, &*LEVEL_3);
        map.insert(HelixirLevel::Level4, &*LEVEL_4);
        map.insert(HelixirLevel::Level5, &*LEVEL_5);
        map
    };
}


pub fn get_level_definition(level: HelixirLevel) -> &'static LevelDefinition {
    LEVELS.get(&level).expect("All levels should be defined")
}


pub fn get_all_levels() -> Vec<&'static LevelDefinition> {
    (0..=5)
        .filter_map(HelixirLevel::from_number)
        .filter_map(|l| LEVELS.get(&l).copied())
        .collect()
}

