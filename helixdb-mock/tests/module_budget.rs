use std::path::{Path, PathBuf};

#[test]
fn every_rust_module_stays_within_budget() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut modules = Vec::new();
    collect_rust_modules(&root, &mut modules);
    assert!(!modules.is_empty());

    let oversized: Vec<_> = modules
        .into_iter()
        .filter_map(|path| {
            let source = std::fs::read_to_string(&path).unwrap();
            let lines = source.lines().count();
            (lines > 500).then_some((path, lines))
        })
        .collect();
    assert!(
        oversized.is_empty(),
        "Rust modules over 500 lines: {oversized:?}"
    );
}

fn collect_rust_modules(directory: &Path, modules: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().is_some_and(|name| name == "target") {
            continue;
        }
        if path.is_dir() {
            collect_rust_modules(&path, modules);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            modules.push(path);
        }
    }
}
