use std::fs;
use std::io::Write;
use std::sync::Arc;
use std::thread;

use super::*;

fn temporary_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "helixir-embed-cache-{name}-{}-{}.jsonl",
        std::process::id(),
        uuid::Uuid::new_v4()
    ))
}

fn namespace(provider: &str, endpoint: &str, revision: &str, dimension: usize) -> CacheNamespace {
    CacheNamespace::new(
        provider,
        endpoint,
        "nomic-embed-text",
        revision,
        Some(dimension),
        "test",
    )
}

fn cleanup(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("lock"));
}

#[test]
fn namespace_separates_provider_endpoint_revision_and_dimension() {
    let base = namespace("ollama", "http://localhost:11434", "sha:a", 2);
    assert_eq!(
        base,
        namespace("ollama", "HTTP://LOCALHOST:11434/", "sha:a", 2)
    );
    assert_ne!(
        base,
        namespace("openai", "http://localhost:11434", "sha:a", 2)
    );
    assert_ne!(base, namespace("ollama", "http://remote:11434", "sha:a", 2));
    assert_ne!(
        base,
        namespace("ollama", "http://localhost:11434", "sha:b", 2)
    );
    assert_ne!(
        base,
        namespace("ollama", "http://localhost:11434", "sha:a", 3)
    );
    assert_ne!(
        base,
        CacheNamespace::new(
            "ollama",
            "http://localhost:11434",
            "nomic-embed-text",
            "sha:a",
            Some(2),
            "new-epoch",
        )
    );
}

#[test]
fn persistence_loads_newest_unique_bounded_entries() {
    let path = temporary_path("bounded");
    let ns = namespace("ollama", "http://localhost:11434", "sha:a", 2);
    {
        let cache = EmbeddingCache::with_persistence_limits(
            2,
            60,
            &path,
            std::slice::from_ref(&ns),
            1_000_000,
        );
        cache.set(&ns, "old", vec![1.0, 0.0]);
        cache.set(&ns, "middle", vec![0.5, 0.5]);
        cache.set(&ns, "new", vec![0.0, 1.0]);
    }
    let cache =
        EmbeddingCache::with_persistence_limits(2, 60, &path, std::slice::from_ref(&ns), 1_000_000);
    assert!(cache.get(&ns, "old").is_none());
    assert_eq!(cache.get(&ns, "middle"), Some(vec![0.5, 0.5]));
    assert_eq!(cache.get(&ns, "new"), Some(vec![0.0, 1.0]));
    cleanup(&path);
}

#[test]
fn foreign_namespace_and_wrong_dimension_fail_safe() {
    let path = temporary_path("namespace");
    let primary = namespace("ollama", "http://localhost:11434", "sha:a", 2);
    let foreign = namespace("ollama", "http://localhost:11434", "sha:b", 2);
    {
        let cache = EmbeddingCache::with_persistence_limits(
            10,
            60,
            &path,
            &[primary.clone(), foreign.clone()],
            1_000_000,
        );
        cache.set(&primary, "same", vec![1.0, 0.0]);
        cache.set(&foreign, "same", vec![0.0, 1.0]);
        cache.set(&primary, "wrong", vec![1.0, 2.0, 3.0]);
    }
    let cache = EmbeddingCache::with_persistence_limits(
        10,
        60,
        &path,
        std::slice::from_ref(&primary),
        1_000_000,
    );
    assert_eq!(cache.get(&primary, "same"), Some(vec![1.0, 0.0]));
    assert!(cache.get(&foreign, "same").is_none());
    assert!(cache.get(&primary, "wrong").is_none());
    cleanup(&path);
}

#[test]
fn malformed_truncated_foreign_and_wrong_dimension_rows_are_invalidated() {
    let path = temporary_path("malformed");
    let primary = namespace("ollama", "http://localhost:11434", "sha:a", 2);
    let foreign = namespace("ollama", "http://localhost:11434", "sha:b", 2);
    {
        let cache = EmbeddingCache::with_persistence_limits(
            10,
            60,
            &path,
            &[primary.clone(), foreign.clone()],
            1_000_000,
        );
        cache.set(&primary, "valid", vec![1.0, 0.0]);
    }
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    let mut wrong_dimension =
        disk::DiskRecord::new(primary.id.clone(), "a".repeat(64), vec![1.0, 0.0]);
    wrong_dimension.dimension = 3;
    serde_json::to_writer(&mut file, &wrong_dimension).unwrap();
    writeln!(file).unwrap();
    let foreign_record = disk::DiskRecord::new(foreign.id.clone(), "b".repeat(64), vec![0.0, 1.0]);
    serde_json::to_writer(&mut file, &foreign_record).unwrap();
    writeln!(file).unwrap();
    writeln!(file, "{{\"truncated\":").unwrap();
    drop(file);

    let cache = EmbeddingCache::with_persistence_limits(
        10,
        60,
        &path,
        std::slice::from_ref(&primary),
        1_000_000,
    );
    assert_eq!(cache.get(&primary, "valid"), Some(vec![1.0, 0.0]));
    assert!(cache.diagnostics().invalidations >= 3);
    for line in fs::read_to_string(&path).unwrap().lines() {
        let record = serde_json::from_str::<disk::DiskRecord>(line).unwrap();
        assert_eq!(record.namespace, primary.id);
        assert_eq!(record.dimension, 2);
    }
    cleanup(&path);
}

#[test]
fn clear_is_durable_and_compaction_bounds_file() {
    let path = temporary_path("clear");
    let ns = namespace("ollama", "http://localhost:11434", "sha:a", 2);
    let cache =
        EmbeddingCache::with_persistence_limits(3, 60, &path, std::slice::from_ref(&ns), 512);
    for index in 0..20 {
        cache.set(&ns, &format!("text-{index}"), vec![index as f32, 1.0]);
    }
    assert!(fs::metadata(&path).unwrap().len() <= 512);
    let diagnostics = cache.diagnostics();
    assert!(diagnostics.compactions > 0);
    assert!(diagnostics.bytes <= 512);
    cache.clear();
    drop(cache);
    let cache =
        EmbeddingCache::with_persistence_limits(3, 60, &path, std::slice::from_ref(&ns), 512);
    assert_eq!(cache.len(), 0);
    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
    cleanup(&path);
}

#[test]
fn record_larger_than_disk_budget_remains_ephemeral() {
    let path = temporary_path("oversized");
    let ns = namespace("ollama", "http://localhost:11434", "sha:a", 2);
    let cache = EmbeddingCache::with_persistence_limits(3, 0, &path, std::slice::from_ref(&ns), 64);
    cache.set(&ns, "too-large-for-the-durable-budget", vec![1.0, 0.0]);

    assert!(fs::metadata(&path).unwrap().len() <= 64);
    assert!(cache.get(&ns, "too-large-for-the-durable-budget").is_none());
    cleanup(&path);
}

#[test]
fn diagnostics_count_hits_misses_and_invalidations() {
    let path = temporary_path("diagnostics");
    let ns = namespace("ollama", "http://localhost:11434", "sha:a", 2);
    let cache =
        EmbeddingCache::with_persistence_limits(4, 60, &path, std::slice::from_ref(&ns), 1_000_000);
    assert!(cache.get(&ns, "missing").is_none());
    cache.set(&ns, "known", vec![1.0, 0.0]);
    assert_eq!(cache.get(&ns, "known"), Some(vec![1.0, 0.0]));
    cache.clear();

    let diagnostics = cache.diagnostics();
    assert_eq!(diagnostics.hits, 1);
    assert_eq!(diagnostics.misses, 1);
    assert_eq!(diagnostics.invalidations, 1);
    assert_eq!(diagnostics.entries, 0);
    cleanup(&path);
}

#[test]
fn orphaned_compaction_file_does_not_replace_last_valid_snapshot() {
    let path = temporary_path("interrupted");
    let ns = namespace("ollama", "http://localhost:11434", "sha:a", 2);
    {
        let cache = EmbeddingCache::with_persistence_limits(
            4,
            60,
            &path,
            std::slice::from_ref(&ns),
            1_000_000,
        );
        cache.set(&ns, "survives", vec![1.0, 0.0]);
    }
    let orphan = path.with_extension("tmp.interrupted");
    fs::write(&orphan, b"{truncated").unwrap();

    let cache =
        EmbeddingCache::with_persistence_limits(4, 60, &path, std::slice::from_ref(&ns), 1_000_000);
    assert_eq!(cache.get(&ns, "survives"), Some(vec![1.0, 0.0]));
    let _ = fs::remove_file(orphan);
    cleanup(&path);
}

#[test]
fn concurrent_process_equivalent_writers_keep_valid_jsonl() {
    let path = temporary_path("concurrent");
    let ns = namespace("ollama", "http://localhost:11434", "sha:a", 2);
    let caches: Vec<_> = (0..4)
        .map(|_| {
            Arc::new(EmbeddingCache::with_persistence_limits(
                16,
                60,
                &path,
                std::slice::from_ref(&ns),
                8_192,
            ))
        })
        .collect();
    let handles: Vec<_> = caches
        .into_iter()
        .enumerate()
        .map(|(worker, cache)| {
            let ns = ns.clone();
            thread::spawn(move || {
                for index in 0..25 {
                    cache.set(
                        &ns,
                        &format!("worker-{worker}-{index}"),
                        vec![worker as f32, index as f32],
                    );
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    for line in fs::read_to_string(&path).unwrap().lines() {
        serde_json::from_str::<disk::DiskRecord>(line).unwrap();
    }
    let reloaded =
        EmbeddingCache::with_persistence_limits(16, 60, &path, std::slice::from_ref(&ns), 8_192);
    assert!(reloaded.len() <= 16);
    cleanup(&path);
}
