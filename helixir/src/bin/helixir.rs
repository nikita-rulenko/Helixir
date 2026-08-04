//! `helixir` — the agent control & monitoring CLI (Moirai).
//!
//! The Moirai are background agents; at this stage the CLI is the dashboard that
//! both *drives* them (seed/tag/route) and *observes* them — live `tracing` logs
//! on stderr plus a persistent **activity journal** (append-only JSONL). When the
//! daemon (#42) lands and the agents run continuously, it writes to the same
//! journal and `helixir journal` becomes the monitor for the background fleet.
//!
//! Config comes from the same `HELIX_*` env as the MCP server (`from_env`).
//! Journal path: `$HELIXIR_AGENT_LOG` or `./helixir-agent-activity.jsonl`.

use std::fs::OpenOptions;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use dialoguer::{Confirm, Input, MultiSelect, Password, Select};
use helixir::agents::atropos::Insight;
use helixir::agents::daemon::DaemonConfig;
use helixir::agents::orchestrator::PassConfig;
use helixir::core::HelixirClient;
use helixir::core::config::MemoryMode;
use helixir::core::rbac::Role;
use tracing_subscriber::EnvFilter;

#[path = "helixir/cli.rs"]
mod cli;
pub(crate) use cli::*;
#[path = "helixir/cli_commands.rs"]
mod cli_commands;
pub(crate) use cli_commands::*;
#[path = "helixir/config_cmd.rs"]
mod config_cmd;
pub(crate) use config_cmd::*;
#[path = "helixir/rbac_cmd.rs"]
mod rbac_cmd;
pub(crate) use rbac_cmd::*;
#[path = "helixir/agents_cmd.rs"]
mod agents_cmd;
#[path = "helixir/app.rs"]
mod app;
pub(crate) use agents_cmd::*;
#[path = "helixir/setup.rs"]
mod setup;
pub(crate) use setup::*;
#[path = "helixir/setup_flow.rs"]
mod setup_flow;
pub(crate) use setup_flow::*;
#[path = "helixir/onboard_detect.rs"]
mod onboard_detect;
pub(crate) use onboard_detect::*;
#[path = "helixir/onboard_options.rs"]
mod onboard_options;
pub(crate) use onboard_options::*;
#[path = "helixir/onboard_run.rs"]
mod onboard_run;
pub(crate) use onboard_run::*;
#[path = "helixir/onboard_executor.rs"]
mod onboard_executor;
pub(crate) use onboard_executor::*;
#[path = "helixir/embedding_recovery.rs"]
mod embedding_recovery;
pub(crate) use embedding_recovery::*;
#[path = "helixir/onboard_utils.rs"]
mod onboard_utils;
pub(crate) use onboard_utils::*;
#[path = "helixir/onboard_skills.rs"]
mod onboard_skills;
pub(crate) use onboard_skills::*;
#[path = "helixir/onboard_clients.rs"]
mod onboard_clients;
pub(crate) use onboard_clients::*;
#[path = "helixir/doctor.rs"]
mod doctor;
pub(crate) use doctor::*;
#[path = "helixir/wire.rs"]
mod wire;
pub(crate) use wire::*;
#[path = "helixir/model.rs"]
mod model;
pub(crate) use model::*;
#[path = "helixir/maintenance.rs"]
mod maintenance;
pub(crate) use maintenance::*;
#[path = "helixir/process.rs"]
mod process;
pub(crate) use process::*;
#[path = "helixir/watch.rs"]
mod watch;
pub(crate) use watch::*;
#[path = "helixir/gateway.rs"]
mod gateway;
pub(crate) use gateway::*;
#[path = "helixir/journal.rs"]
mod journal;
pub(crate) use journal::*;

#[tokio::main]
async fn main() -> Result<()> {
    app::run().await
}
