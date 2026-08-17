//! Frontend-neutral progress events emitted by installation execution.

use serde::{Deserialize, Serialize};

use super::{InstallAction, InstallStep};

/// Stable operation lifecycle values used by journals, SSE, and CLI rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallEventKind {
    PlanStarted,
    StepStarted,
    StepSucceeded,
    StepFailed,
    RollbackStarted,
    RollbackFailed,
    PlanCompleted,
}

/// One redaction-safe event in an installation operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallEvent {
    pub kind: InstallEventKind,
    pub step_index: Option<usize>,
    pub total_steps: usize,
    pub action: Option<InstallAction>,
    pub detail: Option<String>,
    pub ready: Option<bool>,
}

impl InstallEvent {
    pub(crate) fn plan_started(total_steps: usize) -> Self {
        Self::new(InstallEventKind::PlanStarted, None, total_steps, None)
    }

    pub(crate) fn step_started(index: usize, total: usize, step: &InstallStep) -> Self {
        Self::new(
            InstallEventKind::StepStarted,
            Some(index),
            total,
            Some(step.action.clone()),
        )
    }

    pub(crate) fn step_succeeded(index: usize, total: usize, step: &InstallStep) -> Self {
        Self::new(
            InstallEventKind::StepSucceeded,
            Some(index),
            total,
            Some(step.action.clone()),
        )
    }

    pub(crate) fn step_failed(
        index: usize,
        total: usize,
        step: &InstallStep,
        detail: &str,
    ) -> Self {
        Self::new(
            InstallEventKind::StepFailed,
            Some(index),
            total,
            Some(step.action.clone()),
        )
        .with_detail(detail)
    }

    pub(crate) fn rollback_started(index: usize, total: usize, action: &InstallAction) -> Self {
        Self::new(
            InstallEventKind::RollbackStarted,
            Some(index),
            total,
            Some(action.clone()),
        )
    }

    pub(crate) fn rollback_failed(
        index: usize,
        total: usize,
        action: &InstallAction,
        detail: &str,
    ) -> Self {
        Self::new(
            InstallEventKind::RollbackFailed,
            Some(index),
            total,
            Some(action.clone()),
        )
        .with_detail(detail)
    }

    pub(crate) fn plan_completed(ready: bool, total_steps: usize) -> Self {
        let mut event = Self::new(InstallEventKind::PlanCompleted, None, total_steps, None);
        event.ready = Some(ready);
        event
    }

    fn new(
        kind: InstallEventKind,
        step_index: Option<usize>,
        total_steps: usize,
        action: Option<InstallAction>,
    ) -> Self {
        Self {
            kind,
            step_index,
            total_steps,
            action,
            detail: None,
            ready: None,
        }
    }

    fn with_detail(mut self, detail: &str) -> Self {
        self.detail = Some(detail.to_string());
        self
    }
}

/// Synchronous observer boundary; implementations may journal or broadcast events.
pub trait InstallObserver: Send + Sync {
    fn observe(&self, event: InstallEvent);
}

impl<F> InstallObserver for F
where
    F: Fn(InstallEvent) + Send + Sync,
{
    fn observe(&self, event: InstallEvent) {
        self(event);
    }
}

pub(crate) struct NoopObserver;

impl InstallObserver for NoopObserver {
    fn observe(&self, _event: InstallEvent) {}
}
