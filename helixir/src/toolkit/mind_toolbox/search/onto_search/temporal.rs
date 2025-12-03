

use chrono::{DateTime, NaiveDateTime, Utc};


pub fn parse_datetime_utc(dt_string: &str) -> Option<DateTime<Utc>> {
    if dt_string.is_empty() {
        return None;
    }
    let dt_string = dt_string.replace('Z', "+00:00");

    
    if let Ok(dt) = DateTime::parse_from_rfc3339(&dt_string) {
        return Some(dt.with_timezone(&Utc));
    }

    
    let dt_str = dt_string.split('+').next().unwrap_or(&dt_string);
    if let Ok(naive) = NaiveDateTime::parse_from_str(dt_str, "%Y-%m-%dT%H:%M:%S") {
        return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
    }
    None
}


pub fn is_within_temporal_window(created_at: &str, hours: Option<f64>) -> bool {
    let Some(hours) = hours else { return true; };
    let Some(created) = parse_datetime_utc(created_at) else { return true; };
    let cutoff = Utc::now() - chrono::Duration::hours(hours as i64);
    created >= cutoff
}


pub fn calculate_temporal_freshness(created_at: &str, decay_days: f64) -> f64 {
    let Some(created) = parse_datetime_utc(created_at) else { return 0.5; };
    let days_old = (Utc::now() - created).num_milliseconds() as f64 / 86_400_000.0;
    (-days_old / decay_days).exp().clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_datetime_utc() {
        assert!(parse_datetime_utc("2023-01-01T00:00:00Z").is_some());
        assert!(parse_datetime_utc("2023-01-01T00:00:00+00:00").is_some());
        assert!(parse_datetime_utc("").is_none());
    }

    #[test]
    fn test_temporal_freshness() {
        let now = Utc::now().to_rfc3339();
        assert!(calculate_temporal_freshness(&now, 30.0) > 0.99);
    }
}

