// Library interface for helix-cli to enable testing
use clap::{Subcommand, ValueEnum};

pub mod cleanup;
pub mod commands;
pub mod config;
pub mod docker;
pub mod errors;
pub mod github_issue;
pub mod metrics_sender;
pub mod output;
pub mod port;
pub mod project;
pub mod prompts;
pub mod sse_client;
pub mod update;
pub mod utils;

#[derive(Subcommand)]
pub enum AuthAction {
    /// Login to Helix cloud
    Login,
    /// Logout from Helix cloud
    Logout,
    /// Rotate a cluster API key
    CreateKey {
        /// Cluster ID
        cluster: String,
    },
}

#[derive(Subcommand)]
pub enum MetricsAction {
    /// Enable metrics collection
    Full,
    /// Disable metrics collection
    Basic,
    /// Disable metrics collection
    Off,
    /// Show metrics status
    Status,
}

#[derive(Subcommand)]
pub enum DashboardAction {
    /// Start the dashboard
    Start {
        /// Instance to connect to (from helix.toml)
        instance: Option<String>,

        /// Port to run dashboard on
        #[arg(short, long, default_value = "3000")]
        port: u16,

        /// Helix host to connect to (e.g., localhost). Bypasses project config.
        #[arg(long)]
        host: Option<String>,

        /// Helix port to connect to. Used with --host.
        #[arg(long, default_value = "6969")]
        helix_port: u16,

        /// Run dashboard in foreground with logs
        #[arg(long)]
        attach: bool,

        /// Restart if dashboard is already running
        #[arg(long)]
        restart: bool,
    },
    /// Stop the dashboard
    Stop,
    /// Show dashboard status
    Status,
}

#[derive(Subcommand, Clone)]
pub enum CloudDeploymentTypeCommand {
    /// Initialize Helix Cloud deployment
    #[command(name = "cloud")]
    Helix {
        /// Region for Helix cloud instance (default: us-east-1)
        #[arg(long, default_value = "us-east-1")]
        region: Option<String>,

        /// Instance name
        #[arg(short, long)]
        name: Option<String>,
    },
    /// Initialize ECR deployment
    Ecr {
        /// Instance name
        #[arg(short, long)]
        name: Option<String>,
    },
    /// Initialize Fly.io deployment
    Fly {
        /// Authentication type
        #[arg(long, default_value = "cli")]
        auth: String,

        /// volume size
        #[arg(long, default_value = "20")]
        volume_size: u16,

        /// vm size
        #[arg(long, default_value = "shared-cpu-4x")]
        vm_size: String,

        /// privacy
        #[arg(long, default_value = "false")]
        private: bool,

        /// Instance name
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Initialize Local deployment
    Local {
        /// Instance name
        #[arg(short, long)]
        name: Option<String>,
    },
}

impl CloudDeploymentTypeCommand {
    pub fn name(&self) -> Option<String> {
        match self {
            CloudDeploymentTypeCommand::Helix { name, .. } => name.clone(),
            CloudDeploymentTypeCommand::Ecr { name } => name.clone(),
            CloudDeploymentTypeCommand::Fly { name, .. } => name.clone(),
            CloudDeploymentTypeCommand::Local { name } => name.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum ConfigOutputFormat {
    #[default]
    Human,
    Json,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Manage the active workspace selection
    Workspace {
        #[command(subcommand)]
        action: WorkspaceConfigAction,
    },
    /// Manage the project linked in helix.toml
    Project {
        #[command(subcommand)]
        action: ProjectConfigAction,
    },
    /// List local and remote clusters for a workspace or project
    Cluster {
        #[command(subcommand)]
        action: ClusterConfigAction,
    },
}

#[derive(Subcommand)]
pub enum WorkspaceConfigAction {
    /// List accessible workspaces
    List {
        /// Include workspace members
        #[arg(long)]
        members: bool,

        /// Output format
        #[arg(long, value_enum, default_value_t = ConfigOutputFormat::Human)]
        format: ConfigOutputFormat,
    },
    /// Show the currently selected workspace
    Show {
        /// Output format
        #[arg(long, value_enum, default_value_t = ConfigOutputFormat::Human)]
        format: ConfigOutputFormat,
    },
    /// Switch the active workspace
    Switch {
        /// Workspace slug by default, or workspace ID with --id
        workspace: Option<String>,

        /// Treat the selector as a workspace ID instead of a slug
        #[arg(long)]
        id: bool,
    },
}

#[derive(Subcommand)]
pub enum ProjectConfigAction {
    /// List projects in the selected workspace
    List {
        /// Workspace slug by default, or workspace ID with --id
        workspace: Option<String>,

        /// Treat the workspace selector as a workspace ID instead of a slug
        #[arg(long)]
        id: bool,

        /// Output format
        #[arg(long, value_enum, default_value_t = ConfigOutputFormat::Human)]
        format: ConfigOutputFormat,
    },
    /// Show the project linked in helix.toml
    Show {
        /// Output format
        #[arg(long, value_enum, default_value_t = ConfigOutputFormat::Human)]
        format: ConfigOutputFormat,
    },
    /// Switch the project linked in helix.toml
    Switch {
        /// Project name by default, or project ID with --id
        project: Option<String>,

        /// Treat the selector as a project ID instead of a project name
        #[arg(long)]
        id: bool,
    },
}

#[derive(Subcommand)]
pub enum ClusterConfigAction {
    /// List local instances plus live workspace/project clusters
    List {
        /// Workspace slug to inspect
        #[arg(long, conflicts_with = "workspace_id")]
        workspace: Option<String>,

        /// Workspace ID to inspect
        #[arg(long = "workspace-id", conflicts_with = "workspace")]
        workspace_id: Option<String>,

        /// Project name to narrow the remote results within the selected workspace
        #[arg(long, conflicts_with = "project_id")]
        project: Option<String>,

        /// Project ID to narrow the remote results
        #[arg(long = "project-id", conflicts_with = "project")]
        project_id: Option<String>,

        /// Output format
        #[arg(long, value_enum, default_value_t = ConfigOutputFormat::Human)]
        format: ConfigOutputFormat,
    },
}

#[cfg(test)]
mod tests;
