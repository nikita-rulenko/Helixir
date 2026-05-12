//! Entity subsystem — typed actors / objects extracted from memories.
//!
//! Layout:
//! - [`types`] — pure data types ([`Entity`], [`EntityType`], [`EntityEdgeType`],
//!   [`ExtractedEntity`]) plus the persistence DTO.
//! - [`error`] — [`EntityError`] for all I/O and validation failures.
//! - [`manager`] — [`EntityManager`], the cache-fronted CRUD + linking API.
//!
//! Public surface kept stable: every type previously exported from `entity::*`
//! is still reachable through this `mod.rs`.

mod error;
mod manager;
mod types;

pub use error::EntityError;
pub use manager::EntityManager;
pub use types::{Entity, EntityEdgeType, EntityType, ExtractedEntity};

pub use EntityEdgeType as EdgeType;
