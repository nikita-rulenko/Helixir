//! Response-header helpers for the browser control-plane boundary.

use axum::http::{HeaderName, HeaderValue};
use tower_http::set_header::SetResponseHeaderLayer;

use super::ControlPlaneConfig;

pub(super) fn validate_bind(config: &ControlPlaneConfig) -> anyhow::Result<()> {
    anyhow::ensure!(
        config.bind.ip().is_loopback() || config.containerized,
        "native web mode is loopback-only; non-loopback binding is reserved for the isolated container"
    );
    Ok(())
}

pub(super) fn security_header(
    name: &'static str,
    value: &'static str,
) -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::if_not_present(
        HeaderName::from_static(name),
        HeaderValue::from_static(value),
    )
}
