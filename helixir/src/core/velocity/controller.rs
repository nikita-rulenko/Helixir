

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

use super::metrics::calculate_metrics;
use super::models::{
    EventType, IssueState, IssueStatus, IssueTransition, VelocityEvent, VelocityMetrics,
};


pub struct VelocityController {
    
    project_id: String,
    
    events: RwLock<Vec<VelocityEvent>>,
    
    issue_states: RwLock<HashMap<String, IssueState>>,
}

impl VelocityController {
    
    pub fn new(project_id: impl Into<String>) -> Self {
        let project_id = project_id.into();
        info!("VelocityController initialized for project: {}", project_id);

        Self {
            project_id,
            events: RwLock::new(Vec::new()),
            issue_states: RwLock::new(HashMap::new()),
        }
    }

    
    pub async fn track_event(&self, event: VelocityEvent) {
        debug!(
            "Tracking event: {:?} for entity: {}",
            event.event_type, event.entity_id
        );

        
        match event.event_type {
            EventType::IssueCreated => {
                let mut states = self.issue_states.write().await;
                states.insert(
                    event.entity_id.clone(),
                    IssueState {
                        status: IssueStatus::Created,
                        created_at: event.timestamp,
                        resolved_at: None,
                        transitions: vec![IssueTransition {
                            from: None,
                            to: IssueStatus::Created,
                            at: event.timestamp,
                        }],
                        metadata: event.metadata.clone(),
                    },
                );
            }

            EventType::IssueStatusChanged => {
                let mut states = self.issue_states.write().await;
                if let Some(state) = states.get_mut(&event.entity_id) {
                    if let Some(new_status) = event
                        .metadata
                        .get("new_status")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse::<IssueStatus>().ok())
                    {
                        let old_status = state.status;
                        state.status = new_status;
                        state.transitions.push(IssueTransition {
                            from: Some(old_status),
                            to: new_status,
                            at: event.timestamp,
                        });
                    }
                }
            }

            EventType::IssueResolved => {
                let mut states = self.issue_states.write().await;
                if let Some(state) = states.get_mut(&event.entity_id) {
                    let old_status = state.status;
                    state.status = IssueStatus::Resolved;
                    state.resolved_at = Some(event.timestamp);
                    state.transitions.push(IssueTransition {
                        from: Some(old_status),
                        to: IssueStatus::Resolved,
                        at: event.timestamp,
                    });
                }
            }

            _ => {}
        }

        
        let mut events = self.events.write().await;
        events.push(event);
    }

    
    pub async fn calculate_metrics(&self, period_days: i64) -> VelocityMetrics {
        let events = self.events.read().await;
        let issue_states = self.issue_states.read().await;

        calculate_metrics(&events, &issue_states, period_days)
    }

    
    pub async fn get_issue_lifecycle(&self, issue_id: &str) -> Option<IssueState> {
        let states = self.issue_states.read().await;
        states.get(issue_id).cloned()
    }

    
    pub async fn get_stats(&self) -> ControllerStats {
        let events = self.events.read().await;
        let issue_states = self.issue_states.read().await;

        let open_issues = issue_states
            .values()
            .filter(|s| !matches!(s.status, IssueStatus::Resolved | IssueStatus::Deprecated))
            .count();

        ControllerStats {
            project_id: self.project_id.clone(),
            total_events: events.len(),
            tracked_issues: issue_states.len(),
            open_issues,
        }
    }
}


#[derive(Debug, Clone)]
pub struct ControllerStats {
    pub project_id: String,
    pub total_events: usize,
    pub tracked_issues: usize,
    pub open_issues: usize,
}

