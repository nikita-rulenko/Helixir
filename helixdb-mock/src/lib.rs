//! Deterministic, bounded `HelixDB` v2 HTTP emulator used for differential tests.
//!
//! The emulator intentionally runs as a separate process. This lets a test
//! harness measure Helixir and database RSS independently without attributing
//! allocations from an in-process fake to the wrong component.

pub mod config;
mod fixture;
mod metrics;
pub mod profile;
mod registry;
mod response;
pub mod server;
mod state;
mod trace;
mod wire;

pub use config::Config;
pub use profile::{Profile, Scenario};
pub use server::{AppState, admin_router, data_router, run};
