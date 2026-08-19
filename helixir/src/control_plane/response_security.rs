//! Static assets and response policy for the browser control-plane boundary.

use std::path::{Path, PathBuf};

use anyhow::Context;
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

pub(super) fn resolve_assets(explicit: Option<&Path>) -> anyhow::Result<PathBuf> {
    explicit
        .map(Path::to_path_buf)
        .into_iter()
        .chain(std::env::var_os("HELIXIR_WEB_DIST").map(PathBuf::from))
        .chain(
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(|parent| parent.join("web"))),
        )
        .chain([PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web/dist")])
        .find(|path| path.join("index.html").is_file())
        .context("web frontend assets were not found; run `npm run build` in helixir/web")
}
