use super::*;

pub(crate) fn resolve_program(name: &str) -> Option<PathBuf> {
    helixir::installer::clients::resolve_command(name)
}

pub(crate) fn backend_reachable(host: &str, port: u16) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    (host, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.next())
        .and_then(|address| TcpStream::connect_timeout(&address, Duration::from_millis(500)).ok())
        .is_some()
}

pub(crate) fn current_sibling(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(name)))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(name))
}

pub(crate) fn schema_dir_for_install() -> PathBuf {
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("schema")));
    sibling
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("helixir/schema"))
}

// Doctor execution is implemented in the adjacent module.
