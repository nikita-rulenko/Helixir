//! Top-level facade `HelixirClient` — the single object that consumers create.
//!
//! Layout:
//! - [`error`]   — [`HelixirClientError`] (one variant per failure boundary).
//! - [`types`]   — public DTOs returned by client methods.
//! - [`client`]  — [`HelixirClient`] struct, constructor, lifecycle, accessors.
//! - [`memory`]  — `add` / `search` / `update` / `delete` methods.
//! - [`graph`]   — `get_graph`.
//! - [`concepts`] — `search_by_concept` / `search_reasoning_chain`.
//!
//! Every method on `HelixirClient` lives in one of the four feature modules
//! (`memory`, `graph`, `concepts`) as `impl HelixirClient { ... }`; the public
//! API surface is identical to the pre-split file.

mod client;
mod concepts;
mod error;
mod graph;
mod memory;
mod types;

pub use client::HelixirClient;
pub use error::HelixirClientError;
pub use types::{
    AddMemoryResult, ChainNode, GraphEdge, GraphNode, GraphResult, ReasoningChain,
    ReasoningChainResult, SearchResult, UpdateResult,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::HelixirConfig;

    #[test]
    fn test_client_creation() {
        let config = HelixirConfig::default();
        let client = HelixirClient::new(config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_client_from_env() {
        unsafe {
            std::env::set_var("HELIX_HOST", "localhost");
        }
        unsafe {
            std::env::set_var("HELIX_PORT", "6969");
        }
        let client = HelixirClient::from_env();
        assert!(client.is_ok());
    }

    #[test]
    fn test_config_access() {
        let config = HelixirConfig::default();
        let client = HelixirClient::new(config).unwrap();

        assert_eq!(client.config().host, "localhost");
        assert_eq!(client.config().port, 6969);
    }
}
