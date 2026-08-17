use std::fs;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;

use super::*;
use crate::installer::{InstallAction, InstallStep, PlanExecutor, apply_plan_observed};

fn temp_store() -> (PathBuf, OperationStore) {
    let root = std::env::temp_dir().join(format!("helixir-operations-{}", uuid::Uuid::new_v4()));
    let store = OperationStore::open(root.clone()).unwrap();
    (root, store)
}

fn plan() -> InstallPlan {
    InstallPlan {
        steps: vec![InstallStep {
            action: InstallAction::VerifyBackend,
            required: true,
            reason: "verify".into(),
        }],
    }
}

#[test]
fn journal_replays_after_cursor_and_redacts_details() {
    let (root, store) = temp_store();
    let record = store.create(plan()).unwrap();
    store.mark_running(&record.operation_id).unwrap();
    let mut event = InstallEvent::plan_started(1);
    event.detail = Some("authorization: Bearer top-secret".into());
    store.observe(&record.operation_id, event).unwrap();
    let batch = store.events_after(&record.operation_id, 1).unwrap();
    assert_eq!(batch.events.len(), 2);
    assert_eq!(
        batch.events[1].install.as_ref().unwrap().detail.as_deref(),
        Some("[redacted sensitive detail]")
    );
    let persisted = fs::read_to_string(root.join(format!("{}.json", record.operation_id))).unwrap();
    assert!(!persisted.contains("top-secret"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn restart_marks_running_operation_resumable() {
    let (root, store) = temp_store();
    let record = store.create(plan()).unwrap();
    store.mark_running(&record.operation_id).unwrap();
    drop(store);
    let reopened = OperationStore::open(root.clone()).unwrap();
    let recovered = reopened.get(&record.operation_id).unwrap();
    assert_eq!(recovered.status, OperationStatus::Interrupted);
    assert!(recovered.resumable);
    reopened
        .prepare_resume(&record.operation_id, &plan())
        .unwrap();
    assert_eq!(
        reopened.get(&record.operation_id).unwrap().status,
        OperationStatus::Queued
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn resume_rejects_a_changed_plan() {
    let (root, store) = temp_store();
    let record = store.create(plan()).unwrap();
    store
        .finish(&record.operation_id, None, Some("failure"))
        .unwrap();
    let changed = InstallPlan { steps: Vec::new() };
    assert!(
        store
            .prepare_resume(&record.operation_id, &changed)
            .is_err()
    );
    fs::remove_dir_all(root).unwrap();
}

struct InjectedFailure {
    fail_once: AtomicBool,
    rolled_back: Mutex<Vec<InstallAction>>,
}

#[async_trait]
impl PlanExecutor for InjectedFailure {
    async fn apply(&self, action: &InstallAction) -> Result<(), String> {
        if action == &InstallAction::RunDoctor && self.fail_once.swap(false, Ordering::SeqCst) {
            return Err("injected doctor failure".into());
        }
        Ok(())
    }

    async fn rollback(&self, completed: &[InstallAction]) -> Result<(), String> {
        *self.rolled_back.lock().unwrap() = completed.to_vec();
        Ok(())
    }
}

#[tokio::test]
async fn failure_rolls_back_then_same_plan_resumes_to_success() {
    let (root, store) = temp_store();
    let plan = InstallPlan {
        steps: vec![
            InstallStep {
                action: InstallAction::VerifyBackend,
                required: true,
                reason: "verify".into(),
            },
            InstallStep {
                action: InstallAction::RunDoctor,
                required: true,
                reason: "doctor".into(),
            },
        ],
    };
    let record = store.create(plan.clone()).unwrap();
    let executor = InjectedFailure {
        fail_once: AtomicBool::new(true),
        rolled_back: Mutex::new(Vec::new()),
    };
    store.mark_running(&record.operation_id).unwrap();
    let observer = JournalObserver::new(store.clone(), record.operation_id.clone());
    let failed = apply_plan_observed(&executor, &plan, &observer).await;
    store
        .finish(&record.operation_id, Some(failed), None)
        .unwrap();
    assert_eq!(
        store.get(&record.operation_id).unwrap().status,
        OperationStatus::Failed
    );
    assert_eq!(
        *executor.rolled_back.lock().unwrap(),
        vec![InstallAction::VerifyBackend]
    );

    store.prepare_resume(&record.operation_id, &plan).unwrap();
    store.mark_running(&record.operation_id).unwrap();
    let succeeded = apply_plan_observed(&executor, &plan, &observer).await;
    store
        .finish(&record.operation_id, Some(succeeded), None)
        .unwrap();
    let final_state = store.get(&record.operation_id).unwrap();
    assert_eq!(final_state.status, OperationStatus::Succeeded);
    assert!(!final_state.resumable);
    fs::remove_dir_all(root).unwrap();
}
