//! Local browser control plane for installation and ongoing administration.

mod admin;
mod auth;
mod dto;
mod graph;
mod graph_project;
mod graph_snapshot;
mod moirai;
mod operations;
mod response_security;
mod server;
pub mod session;
mod stats;
mod supervisor;

use std::net::SocketAddr;
use std::path::PathBuf;

pub use dto::{
    AccessProjection, AgentProjection, CategoryBreadcrumbProjection, CategoryEdgeProjection,
    CategoryNodeProjection, ContributorProjection, ControlPlaneMeta, DiscoveryResponse,
    GraphEdgeProjection, GroupMutation, GroupProjection, GroupRoleProjection,
    MemoryFieldProjection, MemoryGroupProjection, MemoryProjection, MoiraiInsightProjection,
    MoiraiProjection, MoiraiStageProjection, MutationReceipt, OverviewStats, PrincipalProjection,
    RoleMutation, SystemProjection,
};

/// Runtime options for the local web control plane.
#[derive(Debug, Clone)]
pub struct ControlPlaneConfig {
    /// Address to listen on. Native mode remains loopback-only.
    pub bind: SocketAddr,
    /// Optional built frontend directory. Autodetected when omitted.
    pub assets: Option<PathBuf>,
    /// Optional private browser-token file. Native mode creates it; container
    /// mode requires it to be mounted before startup.
    pub token_file: Option<PathBuf>,
    /// Open the authenticated admin URL in the system browser after binding.
    pub open_browser: bool,
    /// Run inside the isolated control-plane container.
    ///
    /// Container mode may bind its network namespace, but host mutations stay
    /// unavailable until the narrow native supervisor is connected.
    pub containerized: bool,
}

impl Default for ControlPlaneConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 6971)),
            assets: None,
            token_file: None,
            open_browser: true,
            containerized: false,
        }
    }
}

/// Serve the HTML application and versioned local API until interrupted.
pub async fn serve(config: ControlPlaneConfig) -> anyhow::Result<()> {
    server::serve(config).await
}
