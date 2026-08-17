use super::*;

pub(crate) fn resolve_program(name: &str) -> Option<PathBuf> {
    helixir::installer::clients::resolve_command(name)
}

pub(crate) fn current_sibling(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(name)))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(name))
}

// Doctor execution is implemented in the adjacent module.
