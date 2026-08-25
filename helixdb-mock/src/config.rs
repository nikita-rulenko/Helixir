//! Command-line and environment configuration.

use crate::profile::{Profile, Scenario};
use anyhow::{Result, bail};
use clap::Parser;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

/// Standalone `HelixDB` protocol emulator configuration.
#[derive(Clone, Debug, Parser)]
#[command(version, about)]
pub struct Config {
    /// Data-plane listener used by Helixir's `HelixClient`.
    #[arg(long, env = "HELIXDB_MOCK_LISTEN", default_value = "127.0.0.1:16969")]
    pub listen: SocketAddr,

    /// Deterministic latency and fixture-density profile.
    #[arg(long, env = "HELIXDB_MOCK_PROFILE", value_enum, default_value_t)]
    pub profile: Profile,

    /// Deterministic dataset family, independent of latency profile.
    #[arg(long, env = "HELIXDB_MOCK_SCENARIO", value_enum, default_value_t)]
    pub scenario: Scenario,

    /// Seed mixed into deterministic latency and fixture generation.
    #[arg(long, env = "HELIXDB_MOCK_SEED", default_value_t = 17)]
    pub seed: u64,

    /// Hard ceiling for one JSON response.
    #[arg(
        long,
        env = "HELIXDB_MOCK_MAX_RESPONSE_BYTES",
        default_value_t = 262_144
    )]
    pub max_response_bytes: usize,

    /// Hard ceiling for records retained across all mock collections.
    #[arg(long, env = "HELIXDB_MOCK_MAX_RECORDS", default_value_t = 4096)]
    pub max_records: usize,

    /// Optional redacted JSONL request trace.
    #[arg(long, env = "HELIXDB_MOCK_TRACE_PATH")]
    pub trace_path: Option<PathBuf>,

    /// Optional admin listener. It must be loopback and is disabled by default.
    #[arg(long, env = "HELIXDB_MOCK_ADMIN_LISTEN")]
    pub admin_listen: Option<SocketAddr>,
}

impl Config {
    /// Reject configurations that defeat the emulator's safety bounds.
    ///
    /// # Errors
    ///
    /// Returns an error when bounds are invalid or the optional admin plane
    /// is not restricted to loopback.
    pub fn validate(&self) -> Result<()> {
        if self.max_response_bytes < 4096 {
            bail!("max-response-bytes must be at least 4096");
        }
        if self.max_records == 0 {
            bail!("max-records must be greater than zero");
        }
        if let Some(admin) = self.admin_listen
            && !is_loopback(admin.ip())
        {
            bail!("admin-listen must use a loopback address");
        }
        Ok(())
    }
}

fn is_loopback(address: IpAddr) -> bool {
    address.is_loopback()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            listen: "127.0.0.1:16969".parse().unwrap(),
            profile: Profile::Fast,
            scenario: Scenario::BootstrapEmpty,
            seed: 1,
            max_response_bytes: 4096,
            max_records: 1,
            trace_path: None,
            admin_listen: None,
        }
    }

    #[test]
    fn remote_admin_listener_is_rejected() {
        let mut value = config();
        value.admin_listen = Some("0.0.0.0:16970".parse().unwrap());
        assert!(value.validate().is_err());
    }

    #[test]
    fn disabled_or_loopback_admin_listener_is_accepted() {
        let mut value = config();
        assert!(value.validate().is_ok());
        value.admin_listen = Some("127.0.0.1:16970".parse().unwrap());
        assert!(value.validate().is_ok());
    }
}
