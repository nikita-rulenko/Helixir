//! Bounded request metrics for the loopback admin plane.

use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::sync::RwLock;

#[derive(Clone, Debug, Default, Serialize)]
pub struct QueryMetrics {
    pub calls: u64,
    pub errors: u64,
    pub request_bytes: u64,
    pub response_bytes: u64,
    pub output_cardinality: u64,
    pub state_delta: i64,
    pub latency_micros: u64,
}

#[derive(Debug, Default)]
pub struct Metrics {
    total_calls: AtomicU64,
    total_errors: AtomicU64,
    total_request_bytes: AtomicU64,
    total_response_bytes: AtomicU64,
    total_output_cardinality: AtomicU64,
    total_state_delta: AtomicI64,
    by_query: RwLock<BTreeMap<String, QueryMetrics>>,
}

#[derive(Debug, Serialize)]
pub struct MetricsSnapshot {
    pub total_calls: u64,
    pub total_errors: u64,
    pub total_request_bytes: u64,
    pub total_response_bytes: u64,
    pub total_output_cardinality: u64,
    pub total_state_delta: i64,
    pub process_rss_bytes: u64,
    pub by_query: BTreeMap<String, QueryMetrics>,
}

#[derive(Clone, Copy, Debug)]
pub struct Observation {
    pub request_bytes: usize,
    pub response_bytes: usize,
    pub output_cardinality: usize,
    pub state_delta: i64,
    pub latency_micros: u64,
    pub error: bool,
}

impl Metrics {
    pub async fn record(&self, query: &str, observation: Observation) {
        self.total_calls.fetch_add(1, Ordering::Relaxed);
        self.total_request_bytes
            .fetch_add(observation.request_bytes as u64, Ordering::Relaxed);
        self.total_response_bytes
            .fetch_add(observation.response_bytes as u64, Ordering::Relaxed);
        self.total_output_cardinality
            .fetch_add(observation.output_cardinality as u64, Ordering::Relaxed);
        self.total_state_delta
            .fetch_add(observation.state_delta, Ordering::Relaxed);
        if observation.error {
            self.total_errors.fetch_add(1, Ordering::Relaxed);
        }
        let mut by_query = self.by_query.write().await;
        let metric = by_query.entry(query.to_owned()).or_default();
        metric.calls += 1;
        metric.errors += u64::from(observation.error);
        metric.request_bytes += observation.request_bytes as u64;
        metric.response_bytes += observation.response_bytes as u64;
        metric.output_cardinality += observation.output_cardinality as u64;
        metric.state_delta += observation.state_delta;
        metric.latency_micros += observation.latency_micros;
    }

    pub async fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            total_calls: self.total_calls.load(Ordering::Relaxed),
            total_errors: self.total_errors.load(Ordering::Relaxed),
            total_request_bytes: self.total_request_bytes.load(Ordering::Relaxed),
            total_response_bytes: self.total_response_bytes.load(Ordering::Relaxed),
            total_output_cardinality: self.total_output_cardinality.load(Ordering::Relaxed),
            total_state_delta: self.total_state_delta.load(Ordering::Relaxed),
            process_rss_bytes: process_rss_bytes(),
            by_query: self.by_query.read().await.clone(),
        }
    }

    pub async fn reset(&self) {
        self.total_calls.store(0, Ordering::Relaxed);
        self.total_errors.store(0, Ordering::Relaxed);
        self.total_request_bytes.store(0, Ordering::Relaxed);
        self.total_response_bytes.store(0, Ordering::Relaxed);
        self.total_output_cardinality.store(0, Ordering::Relaxed);
        self.total_state_delta.store(0, Ordering::Relaxed);
        self.by_query.write().await.clear();
    }
}

/// Snapshot resident memory without installing a custom allocator in the
/// faithful default binary.
pub fn process_rss_bytes() -> u64 {
    let pid = Pid::from_u32(std::process::id());
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).map_or(0, sysinfo::Process::memory)
}
