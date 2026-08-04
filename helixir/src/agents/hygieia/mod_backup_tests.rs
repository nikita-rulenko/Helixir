use super::*;

#[test]
fn retention_keeps_newest_n() {
    let dir = std::env::temp_dir().join(format!("hyg_bak_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..5 {
        let p = dir.join(format!("helixir-data-2026010{i}-000000.tar.gz"));
        std::fs::write(&p, b"x").unwrap();
        // Distinct mtimes so ordering is deterministic.
        let t = filetime_from_secs(1_700_000_000 + i as i64 * 100);
        let _ = set_mtime(&p, t);
    }
    // A non-archive bystander must survive.
    std::fs::write(dir.join("notes.txt"), b"keep me").unwrap();

    let pruned = prune_backups(&dir, 2);
    assert_eq!(pruned, 3);
    let left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(left.contains(&"notes.txt".to_string()));
    assert_eq!(
        left.iter()
            .filter(|n| n.starts_with("helixir-data-"))
            .count(),
        2
    );
    assert!(newest_backup_age_hours(&dir).is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

fn filetime_from_secs(secs: i64) -> std::time::SystemTime {
    std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs as u64)
}
fn set_mtime(p: &std::path::Path, t: std::time::SystemTime) -> std::io::Result<()> {
    // Portable-enough mtime bump via File::set_times (Rust 1.75+).
    let f = std::fs::File::options().write(true).open(p)?;
    let times = std::fs::FileTimes::new().set_modified(t);
    f.set_times(times)
}
