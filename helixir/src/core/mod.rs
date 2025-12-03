

pub mod cache;
pub mod config;
pub mod error;
pub mod events;
pub mod exceptions;
pub mod levels;
pub mod search_modes;
pub mod velocity;
pub mod helixir_client;

pub mod services;

pub use config::HelixirConfig;
pub use error::{HelixirError, Result};
pub use helixir_client::HelixirClient;
pub use search_modes::{SearchMode, SearchModeDefaults, estimate_token_cost};


pub use services::{
    IDResolutionService, BatchIDResolver, ResolutionStats,
    ChunkingService, ChunkingConfig, ChunkingStrategy,
    LinkBuilder, LinkBuilderStats,
};


pub use velocity::{
    VelocityController, VelocityEvent, VelocityMetrics,
    EventType, IssueStatus, ControllerStats,
};


pub use levels::{
    HelixirLevel, LevelDefinition, AccumulatedSchema,
    get_level_definition, get_all_levels, get_deployment_order,
    get_accumulated_schema, get_accumulated_queries,
};
