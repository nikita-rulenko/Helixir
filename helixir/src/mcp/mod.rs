//! MCP server surface — wraps [`crate::core::helixir_client::HelixirClient`]
//! and exposes the [`rmcp`] tool/prompt/resource protocol over stdio.
//!
//! Layout:
//! - [`server`]   — `HelixirMcpServer` struct, lifecycle, error mapping, runtime entry.
//! - [`tools`]    — tool routers grouped by domain (`memory`, `think`);
//!   merged into a single `ToolRouter` in `tools::mod`.
//! - [`handler`]  — `#[prompt_router]` block and the `ServerHandler` impl
//!   (`get_info`, `list_resources`, `read_resource`).
//! - [`params`]   — typed parameter structs for every tool/prompt.
//! - [`prompts`]  — instruction prompt text (cognitive protocol, tool guide).

mod handler;
mod params;
mod prompts;
mod server;
mod tools;

pub use server::{HelixirMcpServer, run_server};
