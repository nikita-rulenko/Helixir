

use chrono::{DateTime, Duration, Utc};

use super::models::{EventType, IssueState, IssueStatus, VelocityEvent, VelocityMetrics};


pub fn calculate_metrics(
    events: &[VelocityEvent],
    issue_states: &std::collections::HashMap<String, IssueState>,
    period_days: i64,
) -> VelocityMetrics {
    let now = Utc::now();
    let period_start = now - Duration::days(period_days);

    
    let period_events: Vec<_> = events
        .iter()
        .filter(|e| e.timestamp >= period_start)
        .collect();

    
    let resolved_bugs: Vec<_> = period_events
        .iter()
        .filter(|e| e.event_type == EventType::IssueResolved)
        .collect();

    let mut resolution_times: Vec<f64> = Vec::new();

    for resolved_event in &resolved_bugs {
        if let Some(state) = issue_states.get(&resolved_event.entity_id) {
            if let Some(resolved_at) = state.resolved_at {
                let duration = resolved_at - state.created_at;
                resolution_times.push(duration.num_seconds() as f64);
            }
        }
    }

    let avg_bug_resolution_secs = if !resolution_times.is_empty() {
        resolution_times.iter().sum::<f64>() / resolution_times.len() as f64
    } else {
        0.0
    };

    
    let bugs_open_count = issue_states
        .values()
        .filter(|s| !matches!(s.status, IssueStatus::Resolved | IssueStatus::Deprecated))
        .count();

    
    let commits: Vec<_> = period_events
        .iter()
        .filter(|e| e.event_type == EventType::CommitMade)
        .collect();

    let commits_per_day = if period_days > 0 {
        commits.len() as f64 / period_days as f64
    } else {
        0.0
    };

    
    let memories: Vec<_> = period_events
        .iter()
        .filter(|e| e.event_type == EventType::MemoryAdded)
        .collect();

    let memories_per_session = if !commits.is_empty() {
        memories.len() as f64 / commits.len() as f64
    } else {
        memories.len() as f64
    };

    
    let features: Vec<_> = period_events
        .iter()
        .filter(|e| e.event_type == EventType::FeatureCompleted)
        .collect();

    
    let velocity_score = calculate_velocity_score(
        avg_bug_resolution_secs,
        commits_per_day,
        resolved_bugs.len(),
        features.len(),
    );

    VelocityMetrics {
        avg_bug_resolution_secs,
        bugs_resolved_count: resolved_bugs.len(),
        bugs_open_count,
        avg_feature_implementation_secs: 0.0, 
        features_completed_count: features.len(),
        commits_per_day,
        memories_per_session,
        bug_reopen_rate: 0.0,    
        memory_update_rate: 0.0, 
        velocity_score,
        period_start,
        period_end: now,
    }
}


pub fn calculate_velocity_score(
    avg_resolution_secs: f64,
    commits_per_day: f64,
    bugs_resolved: usize,
    features_completed: usize,
) -> f64 {
    let mut score = 0.0;

    
    let resolution_hours = avg_resolution_secs / 3600.0;
    if resolution_hours < 24.0 {
        score += 40.0;
    } else if resolution_hours < 168.0 {
        
        score += 40.0 * (1.0 - (resolution_hours - 24.0) / (168.0 - 24.0));
    }

    
    score += (commits_per_day * 6.0).min(30.0);

    
    score += (bugs_resolved as f64 * 3.0).min(15.0);

    
    score += (features_completed as f64 * 5.0).min(15.0);

    score.min(100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_velocity_score_max() {
        
        let score = calculate_velocity_score(
            3600.0,  
            10.0,    
            10,      
            5,       
        );
        assert!((score - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_velocity_score_zero() {
        
        let score = calculate_velocity_score(
            0.0, 
            0.0, 
            0,   
            0,   
        );
        
        assert!((score - 40.0).abs() < 0.01);
    }

    #[test]
    fn test_velocity_score_slow_resolution() {
        
        let score = calculate_velocity_score(
            168.0 * 3600.0, 
            5.0,            
            5,              
            3,              
        );
        
        assert!((score - 60.0).abs() < 0.01);
    }
}

