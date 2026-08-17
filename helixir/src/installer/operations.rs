//! Durable, redaction-safe state for long-running installer operations.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, ensure};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{InstallEvent, InstallEventKind, InstallObserver, InstallPlan, InstallReport};

/// Durable lifecycle of one privileged installation attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Interrupted,
}

impl OperationStatus {
    #[must_use]
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Interrupted)
    }

    #[must_use]
    pub const fn resumable(self) -> bool {
        matches!(self, Self::Failed | Self::Interrupted)
    }
}

/// Browser-facing event categories. They deliberately contain no raw process output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationEventKind {
    Queued,
    Running,
    Progress,
    Succeeded,
    Failed,
    Rollback,
    Interrupted,
}

/// One replayable event. `sequence` is the SSE cursor and is stable on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationEvent {
    pub operation_id: String,
    pub sequence: u64,
    pub event_id: String,
    pub step_id: Option<String>,
    pub at: DateTime<Utc>,
    pub kind: OperationEventKind,
    pub install: Option<InstallEvent>,
    pub detail: Option<String>,
}

/// Complete durable snapshot returned by status endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationSnapshot {
    pub operation_id: String,
    pub plan_fingerprint: String,
    pub status: OperationStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub plan: InstallPlan,
    pub events: Vec<OperationEvent>,
    pub report: Option<InstallReport>,
    pub error: Option<String>,
    pub resumable: bool,
}

/// Incremental response used by an SSE proxy after a reconnect cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationEventBatch {
    pub operation_id: String,
    pub status: OperationStatus,
    pub events: Vec<OperationEvent>,
    pub next_cursor: u64,
    pub terminal: bool,
    pub resumable: bool,
}

/// Thread-safe operation registry backed by one atomically replaced JSON file per operation.
#[derive(Clone)]
pub struct OperationStore {
    root: Arc<PathBuf>,
    records: Arc<Mutex<HashMap<String, OperationSnapshot>>>,
}

impl OperationStore {
    /// Open the journal and mark operations abandoned by a prior process as interrupted.
    pub fn open(root: PathBuf) -> anyhow::Result<Self> {
        fs::create_dir_all(&root)
            .with_context(|| format!("create operation journal {}", root.display()))?;
        private_dir(&root)?;
        let mut records = HashMap::new();
        for entry in fs::read_dir(&root)? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let record: OperationSnapshot = serde_json::from_slice(&fs::read(&path)?)
                .with_context(|| format!("decode operation journal {}", path.display()))?;
            records.insert(record.operation_id.clone(), record);
        }
        let store = Self {
            root: Arc::new(root),
            records: Arc::new(Mutex::new(records)),
        };
        store.interrupt_inflight()?;
        Ok(store)
    }

    /// Create a queued operation. Only one install mutation may be active at a time.
    pub fn create(&self, plan: InstallPlan) -> anyhow::Result<OperationSnapshot> {
        let mut records = self.records.lock();
        ensure!(
            !records.values().any(|record| {
                matches!(
                    record.status,
                    OperationStatus::Queued | OperationStatus::Running
                )
            }),
            "another installation operation is already active"
        );
        let operation_id = format!("op_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now();
        let mut record = OperationSnapshot {
            operation_id: operation_id.clone(),
            plan_fingerprint: fingerprint(&plan)?,
            status: OperationStatus::Queued,
            created_at: now,
            updated_at: now,
            plan,
            events: Vec::new(),
            report: None,
            error: None,
            resumable: false,
        };
        push_event(&mut record, OperationEventKind::Queued, None, None);
        persist(&self.root, &record)?;
        records.insert(operation_id, record.clone());
        Ok(record)
    }

    /// Validate a replay against the original plan and queue another attempt.
    pub fn prepare_resume(
        &self,
        operation_id: &str,
        plan: &InstallPlan,
    ) -> anyhow::Result<OperationSnapshot> {
        let mut records = self.records.lock();
        let record = records
            .get_mut(operation_id)
            .with_context(|| format!("operation {operation_id} was not found"))?;
        ensure!(record.resumable, "operation is not resumable");
        ensure!(
            record.plan_fingerprint == fingerprint(plan)?,
            "the rebuilt plan differs from the interrupted operation"
        );
        record.status = OperationStatus::Queued;
        record.resumable = false;
        record.error = None;
        push_event(
            record,
            OperationEventKind::Queued,
            None,
            Some("Resume accepted after plan revalidation"),
        );
        persist(&self.root, record)?;
        Ok(record.clone())
    }

    pub fn mark_running(&self, operation_id: &str) -> anyhow::Result<()> {
        self.mutate(operation_id, |record| {
            record.status = OperationStatus::Running;
            push_event(record, OperationEventKind::Running, None, None);
        })
    }

    pub fn observe(&self, operation_id: &str, mut event: InstallEvent) -> anyhow::Result<()> {
        if let Some(detail) = event.detail.as_deref() {
            event.detail = Some(redact(detail));
        }
        self.mutate(operation_id, |record| {
            let kind = event_kind(event.kind);
            let step_id = event
                .step_index
                .map(|index| format!("step-{:04}", index + 1));
            push_event(record, kind, Some((event, step_id)), None);
        })
    }

    pub fn finish(
        &self,
        operation_id: &str,
        report: Option<InstallReport>,
        error: Option<&str>,
    ) -> anyhow::Result<()> {
        self.mutate(operation_id, |record| {
            let report = report.map(redact_report);
            let succeeded = report.as_ref().is_some_and(|value| value.ready) && error.is_none();
            record.status = if succeeded {
                OperationStatus::Succeeded
            } else {
                OperationStatus::Failed
            };
            record.resumable = !succeeded;
            record.report = report;
            record.error = error.map(redact);
            if !record.events.last().is_some_and(|event| {
                matches!(
                    event.kind,
                    OperationEventKind::Succeeded | OperationEventKind::Failed
                )
            }) {
                push_event(
                    record,
                    if succeeded {
                        OperationEventKind::Succeeded
                    } else {
                        OperationEventKind::Failed
                    },
                    None,
                    error,
                );
            }
        })
    }

    pub fn get(&self, operation_id: &str) -> anyhow::Result<OperationSnapshot> {
        self.records
            .lock()
            .get(operation_id)
            .cloned()
            .with_context(|| format!("operation {operation_id} was not found"))
    }

    pub fn events_after(
        &self,
        operation_id: &str,
        cursor: u64,
    ) -> anyhow::Result<OperationEventBatch> {
        let record = self.get(operation_id)?;
        let events: Vec<_> = record
            .events
            .iter()
            .filter(|event| event.sequence > cursor)
            .cloned()
            .collect();
        let next_cursor = events.last().map_or(cursor, |event| event.sequence);
        Ok(OperationEventBatch {
            operation_id: record.operation_id,
            status: record.status,
            events,
            next_cursor,
            terminal: record.status.terminal(),
            resumable: record.resumable,
        })
    }

    fn mutate(
        &self,
        operation_id: &str,
        mutate: impl FnOnce(&mut OperationSnapshot),
    ) -> anyhow::Result<()> {
        let mut records = self.records.lock();
        let record = records
            .get_mut(operation_id)
            .with_context(|| format!("operation {operation_id} was not found"))?;
        mutate(record);
        record.updated_at = Utc::now();
        persist(&self.root, record)
    }

    fn interrupt_inflight(&self) -> anyhow::Result<()> {
        let mut records = self.records.lock();
        for record in records.values_mut().filter(|record| {
            matches!(
                record.status,
                OperationStatus::Queued | OperationStatus::Running
            )
        }) {
            record.status = OperationStatus::Interrupted;
            record.resumable = true;
            record.error = Some("Supervisor stopped before the operation completed".to_string());
            push_event(
                record,
                OperationEventKind::Interrupted,
                None,
                Some("Supervisor restarted; revalidate and resume this operation"),
            );
            persist(&self.root, record)?;
        }
        Ok(())
    }
}

/// Observer adapter used by the worker protocol.
pub struct JournalObserver {
    store: OperationStore,
    operation_id: String,
}

impl JournalObserver {
    #[must_use]
    pub fn new(store: OperationStore, operation_id: String) -> Self {
        Self {
            store,
            operation_id,
        }
    }
}

impl InstallObserver for JournalObserver {
    fn observe(&self, event: InstallEvent) {
        if let Err(error) = self.store.observe(&self.operation_id, event) {
            tracing::error!(%error, operation_id = %self.operation_id, "persist install event");
        }
    }
}

fn event_kind(kind: InstallEventKind) -> OperationEventKind {
    match kind {
        InstallEventKind::PlanStarted | InstallEventKind::StepStarted => {
            OperationEventKind::Progress
        }
        InstallEventKind::StepSucceeded => OperationEventKind::Succeeded,
        InstallEventKind::StepFailed | InstallEventKind::RollbackFailed => {
            OperationEventKind::Failed
        }
        InstallEventKind::RollbackStarted => OperationEventKind::Rollback,
        InstallEventKind::PlanCompleted => OperationEventKind::Progress,
    }
}

fn push_event(
    record: &mut OperationSnapshot,
    kind: OperationEventKind,
    install: Option<(InstallEvent, Option<String>)>,
    detail: Option<&str>,
) {
    let sequence = record.events.last().map_or(1, |event| event.sequence + 1);
    let (install, step_id) = install.map_or((None, None), |(event, step)| (Some(event), step));
    record.updated_at = Utc::now();
    record.events.push(OperationEvent {
        operation_id: record.operation_id.clone(),
        sequence,
        event_id: format!("{}:{sequence}", record.operation_id),
        step_id,
        at: record.updated_at,
        kind,
        install,
        detail: detail.map(redact),
    });
}

fn fingerprint(plan: &InstallPlan) -> anyhow::Result<String> {
    let bytes = serde_json::to_vec(plan).context("encode install plan fingerprint")?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn redact(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if [
        "api_key",
        "api-key",
        "authorization",
        "bearer ",
        "password",
        "secret=",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return "[redacted sensitive detail]".to_string();
    }
    value.chars().take(4_096).collect()
}

fn redact_report(mut report: InstallReport) -> InstallReport {
    for step in &mut report.steps {
        if let Some(detail) = step.detail.as_deref() {
            step.detail = Some(redact(detail));
        }
    }
    if let Some(error) = report.rollback_error.as_deref() {
        report.rollback_error = Some(redact(error));
    }
    report
}

fn persist(root: &Path, record: &OperationSnapshot) -> anyhow::Result<()> {
    let path = root.join(format!("{}.json", record.operation_id));
    let temporary = root.join(format!(
        ".{}.{}.tmp",
        record.operation_id,
        std::process::id()
    ));
    fs::write(&temporary, serde_json::to_vec_pretty(record)?)?;
    private_file(&temporary)?;
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| format!("replace operation journal {}", path.display()));
    }
    Ok(())
}

#[cfg(unix)]
fn private_dir(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn private_dir(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn private_file(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn private_file(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

/// Default host-owned journal directory. It is never mounted into the web container.
#[must_use]
pub fn default_journal_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".helixir/run/operations")
}

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;
