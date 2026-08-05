//! Serial background worker for persisted ingest jobs.

use super::*;

/// The single serial worker. Spawned once by the process runtime when the
/// buffer is on. `tooling` is swapped on SIGHUP only between queue batches,
/// so an in-flight item finishes on its original generation while the next
/// item receives the new configuration.
/// Drains `pending` items oldest-first, one at a time — serialization is the
/// whole point (dedup-race closure), so this never parallelizes.
pub async fn run_ingest_worker(tooling: Arc<ArcSwap<ToolingManager>>) {
    let initial = tooling.load_full();
    info!(
        "Ingest worker started (poll {}ms); add_memory now returns pending_id",
        initial.config.ingest.poll_interval_ms.clamp(50, 60_000)
    );

    // Recover orphans: a `processing` item whose worker process was killed
    // mid-flight would otherwise be stuck forever (the worker only fetches
    // `pending`). Reset them to `pending` so they get retried.
    initial.recover_stuck_processing().await;
    drop(initial);

    loop {
        let tm = tooling.load_full();
        let interval = Duration::from_millis(tm.config.ingest.poll_interval_ms.clamp(50, 60_000));
        match tm.fetch_pending_batch(32).await {
            Ok(mut batch) if !batch.is_empty() => {
                // Oldest first — fairness and causal order for dedup.
                batch.sort_by(|a, b| a.created_at.cmp(&b.created_at));
                for node in batch {
                    tm.process_one_pending(&node).await;
                }
            }
            Ok(_) => {
                tokio::time::sleep(interval).await;
            }
            Err(e) => {
                warn!("Ingest worker: queue poll failed ({e}); backing off");
                tokio::time::sleep(interval.max(Duration::from_secs(2))).await;
            }
        }
    }
}
