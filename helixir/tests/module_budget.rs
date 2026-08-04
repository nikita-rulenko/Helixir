//! Repository-wide guard against oversized Rust modules.

use std::fs;
use std::path::{Path, PathBuf};

const MAX_MODULE_LINES: usize = 500;

fn rust_files_below(root: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(root)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", root.display()));

    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!("failed to read an entry under {}: {error}", root.display())
        });
        let path = entry.path();
        if path.is_dir() {
            rust_files_below(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn maintained_rust_modules_stay_within_the_line_budget() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files_below(&source_root, &mut files);
    files.sort();

    let oversized = files
        .into_iter()
        .filter_map(|path| {
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let lines = source.lines().count();
            (lines > MAX_MODULE_LINES).then_some((path, lines))
        })
        .collect::<Vec<_>>();

    assert!(
        oversized.is_empty(),
        "Rust modules exceed the {MAX_MODULE_LINES}-line budget:\n{}",
        oversized
            .iter()
            .map(|(path, lines)| format!("{}: {lines}", path.display()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
