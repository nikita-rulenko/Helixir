pub mod cache;
pub mod config;
pub mod error;
pub mod events;
pub mod exceptions;
pub mod helixir_client;
pub mod levels;
pub mod retrieval_profile;
pub mod search_modes;
pub mod velocity;

pub mod services;

pub use config::HelixirConfig;
pub use error::{HelixirError, Result};
pub use helixir_client::HelixirClient;
pub use retrieval_profile::RetrievalProfile;
pub use search_modes::{SearchMode, SearchModeDefaults, estimate_token_cost};

pub use services::{
    BatchIDResolver, ChunkingConfig, ChunkingService, ChunkingStrategy, IDResolutionService,
    LinkBuilder, LinkBuilderStats, ResolutionStats,
};

pub use velocity::{
    ControllerStats, EventType, IssueStatus, VelocityController, VelocityEvent, VelocityMetrics,
};

pub use levels::{
    AccumulatedSchema, HelixirLevel, LevelDefinition, get_accumulated_queries,
    get_accumulated_schema, get_all_levels, get_deployment_order, get_level_definition,
};
