//! CLI compatibility facade over the shared native installer detector.

pub(crate) use helixir::installer::native::{
    backend_reachable, client_available, detect_local_backend_tcp, detect_ollama,
    probe_backend_schema_contract, schema_dir_for_install,
};

pub(crate) async fn detect_onboard_state() -> helixir::installer::SystemState {
    helixir::installer::native::detect_system_state().await
}

pub(crate) fn onboard_nli_installed() -> bool {
    helixir::installer::native::nli_installed()
}
