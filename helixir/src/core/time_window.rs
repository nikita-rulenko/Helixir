//! #87: the two-sided time window — event-time bounds for search.
//!
//! A window constrains ATTENTION, never REACHABILITY: it hard-filters the
//! SEEDS of a search, while graph expansion stays exempt and may pull rows
//! from outside the window back in as *flashbacks* — flagged with their
//! event date and capped by a separate small allowance so they never crowd
//! in-window rows. Chains walk freely by definition. This mirrors human
//! recall: thinking about last week can still surface last year's memory,
//! but you KNOW it is old.

use chrono::{DateTime, NaiveDate, Utc};

/// Inclusive event-time bounds. Either side may be open. `default()` is the
/// fully open window — semantically "no window at all".
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TimeWindow {
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

impl TimeWindow {
    /// A one-sided "last N days" window — the `temporal_days` shorthand.
    pub fn last_days(days: f64, now: DateTime<Utc>) -> Self {
        let millis = (days * 24.0 * 60.0 * 60.0 * 1000.0) as i64;
        Self {
            from: Some(now - chrono::Duration::milliseconds(millis)),
            to: None,
        }
    }

    /// True when at least one bound is set — filtering and flashback
    /// flagging happen only for active windows.
    pub fn is_active(&self) -> bool {
        self.from.is_some() || self.to.is_some()
    }

    pub fn contains(&self, t: &DateTime<Utc>) -> bool {
        if let Some(from) = &self.from {
            if t < from {
                return false;
            }
        }
        if let Some(to) = &self.to {
            if t > to {
                return false;
            }
        }
        true
    }

    /// Window check on an RFC3339 timestamp string (the storage format).
    /// Unparseable timestamps count as IN the window — the filter must
    /// never hide a memory because its date string is malformed.
    pub fn contains_rfc3339(&self, when: &str) -> bool {
        match DateTime::parse_from_rfc3339(when) {
            Ok(t) => self.contains(&t.with_timezone(&Utc)),
            Err(_) => true,
        }
    }
}

/// Parse a user-supplied window bound: full RFC3339 (`2026-06-20T14:00:00Z`)
/// or a bare date (`2026-06-20`). A bare date expands to the START of the
/// day for `from` and the END of the day for `to`, so `time_from="2026-06-01",
/// time_to="2026-06-30"` covers the whole month inclusively.
pub fn parse_time_bound(input: &str, is_upper_bound: bool) -> Result<DateTime<Utc>, String> {
    let s = input.trim();
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Ok(t.with_timezone(&Utc));
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let t = if is_upper_bound {
            d.and_hms_milli_opt(23, 59, 59, 999)
        } else {
            d.and_hms_opt(0, 0, 0)
        };
        if let Some(t) = t {
            return Ok(DateTime::from_naive_utc_and_offset(t, Utc));
        }
    }
    Err(format!(
        "cannot parse '{s}' as a time bound — use RFC3339 (2026-06-20T14:00:00Z) or a date (2026-06-20)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn open_window_contains_everything() {
        let w = TimeWindow::default();
        assert!(!w.is_active());
        assert!(w.contains(&t("1999-01-01T00:00:00Z")));
        assert!(w.contains(&t("2099-01-01T00:00:00Z")));
    }

    #[test]
    fn two_sided_window_filters_both_ends() {
        let w = TimeWindow {
            from: Some(t("2026-06-01T00:00:00Z")),
            to: Some(t("2026-06-30T23:59:59Z")),
        };
        assert!(w.is_active());
        assert!(!w.contains(&t("2026-05-31T23:59:59Z")));
        assert!(w.contains(&t("2026-06-15T12:00:00Z")));
        assert!(!w.contains(&t("2026-07-01T00:00:00Z")));
    }

    #[test]
    fn one_sided_upper_bound_is_a_retro_window() {
        let w = TimeWindow {
            from: None,
            to: Some(t("2025-12-31T23:59:59Z")),
        };
        assert!(w.contains(&t("2024-01-01T00:00:00Z")));
        assert!(!w.contains(&t("2026-01-01T00:00:00Z")));
    }

    #[test]
    fn last_days_matches_the_legacy_cutoff_math() {
        let now = t("2026-07-05T00:00:00Z");
        let w = TimeWindow::last_days(30.0, now);
        assert_eq!(w.from, Some(t("2026-06-05T00:00:00Z")));
        assert_eq!(w.to, None);
    }

    #[test]
    fn malformed_timestamp_never_hides_a_memory() {
        let w = TimeWindow::last_days(1.0, Utc::now());
        assert!(w.contains_rfc3339("not-a-date"));
    }

    #[test]
    fn parse_accepts_rfc3339_and_bare_dates() {
        assert_eq!(
            parse_time_bound("2026-06-20T14:00:00+02:00", false).unwrap(),
            t("2026-06-20T12:00:00Z")
        );
        assert_eq!(
            parse_time_bound("2026-06-20", false).unwrap(),
            t("2026-06-20T00:00:00Z")
        );
        assert_eq!(
            parse_time_bound("2026-06-20", true).unwrap(),
            t("2026-06-20T23:59:59.999Z")
        );
        assert!(parse_time_bound("июнь 2026", false).is_err());
    }
}
