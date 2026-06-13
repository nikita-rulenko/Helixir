pub mod chunking;
pub mod entity;
pub mod memory;
pub mod memory_chain;
pub mod ontology;
pub mod ranking;
pub mod reasoning;
pub mod search;

// <unused reason="The `integrator/` subsystem (MemoryIntegrator + SimilarMemoryFinder + EdgeCreator + RelationInferrer + cosine_similarity)
//                is declared but never instantiated — no `MemoryIntegrator::new(` call site exists outside the module itself.
//                It is the dead twin of the live add pipeline (see toolkit/tooling_manager/add_pipeline/).
//                Kept on disk as a historical reference; closes issues #25 (D1 cosine_similarity duplication) and #28 (D4 ReasoningEngine
//                trait/struct naming collision) by removing the only consumer of those duplicates from the live compilation unit.
//                See helixir/doc/duplication-audit.md §3.">
// pub mod integrator;
// </unused>

pub use chunking::ChunkingManager;
pub use entity::{Entity, EntityEdgeType, EntityError, EntityManager, EntityType};
pub use memory::{CrudError, Memory, MemoryCrud, MemoryManager};
pub use ontology::{Concept, ConceptMapper, ConceptMatch, OntologyManager};
