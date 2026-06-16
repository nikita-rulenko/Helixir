//! Contradiction-debt reconciliation (#45) — the drain policy.
//!
//! Cross-user disputes are recorded non-destructively as `resolved=0` CONTRADICTS
//! edges ([[cross_user.rs]]). That is correct — no user overwrites another — but
//! the debt grows unboundedly as the collective scales unless something drains
//! it. This is the Cutter's hygiene pass: decide which open disputes to retire
//! and how, leaving only LIVE, meaningful disagreements for an owner's eyes.
//!
//! Pure policy here (fully unit-tested); the toolkit gathers the open disputes
//! and applies the verdicts. Raised by Nikita 2026-06-16.

use std::collections::HashMap;

use super::Atropos;
use crate::toolkit::tooling_manager::contradictions::OpenContradiction;
use crate::toolkit::tooling_manager::types::ToolingError;

/// What kind of disagreement a CONTRADICTS edge encodes, parsed from its
/// `resolution_strategy` (`cross_user_{conflict_type}` from the write path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisputeKind {
    /// Tastes / choices that legitimately coexist — NOT debt.
    Preference,
    /// A factual claim where at most one side holds — real debt to resolve.
    Factual,
}

/// Classify by the strategy label. Preference-like disputes coexist; everything
/// else is treated as a factual claim (the conservative default — a real claim
/// is never silently dropped as "just a preference").
pub fn classify(resolution_strategy: &str) -> DisputeKind {
    let s = resolution_strategy.to_lowercase();
    if ["preference", "opinion", "taste", "style", "subjective"]
        .iter()
        .any(|k| s.contains(k))
    {
        DisputeKind::Preference
    } else {
        DisputeKind::Factual
    }
}

/// One open dispute with the decay/temporal signals the toolkit could gather.
#[derive(Debug, Clone)]
pub struct OpenDispute {
    pub from_id: String,
    pub to_id: String,
    pub resolution_strategy: String,
    /// The "to" side has been superseded (a SUPERSEDES edge / valid_to) → stale.
    pub to_superseded: bool,
    /// The "from" side has been superseded.
    pub from_superseded: bool,
}

/// The drain verdict for one open dispute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrainVerdict {
    /// Retire it; the carried label becomes the resolved edge's strategy.
    Resolve(String),
    /// Leave open — a live disagreement that needs an owner to reconcile.
    Keep,
}

impl DrainVerdict {
    pub fn is_resolve(&self) -> bool {
        matches!(self, DrainVerdict::Resolve(_))
    }
}

/// The drain policy for a single open dispute:
/// - **preference** → retire as settled-by-design (coexistence is not debt);
/// - **factual, one side already superseded** → moot, the live side won the
///   temporal record → retire toward it;
/// - **factual, both sides live** → keep open for the owning agent (#25/#39).
pub fn drain_decision(d: &OpenDispute) -> DrainVerdict {
    match classify(&d.resolution_strategy) {
        DisputeKind::Preference => DrainVerdict::Resolve("coexist_preference".into()),
        DisputeKind::Factual => {
            if d.to_superseded || d.from_superseded {
                DrainVerdict::Resolve("superseded_side".into())
            } else {
                DrainVerdict::Keep
            }
        }
    }
}

/// Outcome of a reconciliation sweep — the debt metric to watch. Monotonic
/// growth of `kept_live` across passes signals the reconciliation loop is not
/// keeping up (or genuine disagreement is accumulating and needs human eyes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DebtSummary {
    pub scanned: usize,
    pub drained_preference: usize,
    pub drained_superseded: usize,
    pub kept_live: usize,
    /// Live disputes surfaced to an owner's outbox this pass (deduped).
    pub notified: usize,
}

impl DebtSummary {
    pub fn record(&mut self, d: &OpenDispute, v: &DrainVerdict) {
        self.scanned += 1;
        match v {
            DrainVerdict::Resolve(label) if label == "coexist_preference" => {
                self.drained_preference += 1
            }
            DrainVerdict::Resolve(_) => self.drained_superseded += 1,
            DrainVerdict::Keep => self.kept_live += 1,
        }
        let _ = d;
    }
}

impl Atropos<'_> {
    /// The Cutter's hygiene pass (#45): drain dead cross-user disputes for a user
    /// so `resolved=0` debt does not grow unboundedly as the collective scales.
    /// Returns the debt metric. Preferences are retired as coexistence; live
    /// factual disagreements are kept for an owner. Idempotent.
    pub async fn reconcile(&self, user_id: &str, limit: i64) -> Result<DebtSummary, ToolingError> {
        let open = self
            .tooling
            .gather_open_contradictions(user_id, limit)
            .await?;

        // Group by from-memory: the resolve query is per-memory (coarse), so a
        // memory's edges are retired only when NONE is a live "keep" — never
        // clobber a disagreement that still needs an owner's eyes.
        let mut by_from: HashMap<String, Vec<OpenContradiction>> = HashMap::new();
        for oc in open {
            by_from.entry(oc.from_id.clone()).or_default().push(oc);
        }

        let mut summary = DebtSummary::default();
        for (from_id, group) in by_from {
            let mut decided: Vec<(OpenDispute, DrainVerdict)> = Vec::new();
            for oc in &group {
                let d = OpenDispute {
                    from_id: oc.from_id.clone(),
                    to_id: oc.to_id.clone(),
                    resolution_strategy: oc.resolution_strategy.clone(),
                    // Temporal signal: a superseded side makes a factual dispute
                    // moot → the policy retires it toward the live side.
                    to_superseded: self.tooling.is_superseded(&oc.to_id).await,
                    from_superseded: self.tooling.is_superseded(&oc.from_id).await,
                };
                let v = drain_decision(&d);
                decided.push((d, v));
            }

            let all_resolve = decided.iter().all(|(_, v)| v.is_resolve());
            if all_resolve {
                if let Some((_, DrainVerdict::Resolve(label))) = decided.first() {
                    self.tooling
                        .resolve_memory_contradictions(&from_id, label)
                        .await?;
                }
            }
            for (d, v) in &decided {
                // If the group is mixed, nothing was retired — record as kept.
                let applied = if all_resolve { v.clone() } else { DrainVerdict::Keep };
                // A kept live dispute is surfaced to the owner's outbox (deduped)
                // — never silently left, never silently resolved.
                if matches!(applied, DrainVerdict::Keep) {
                    summary.notified += self
                        .tooling
                        .surface_dispute(&d.from_id, &d.to_id, &d.resolution_strategy)
                        .await;
                }
                summary.record(d, &applied);
            }
        }
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dispute(strategy: &str, to_superseded: bool) -> OpenDispute {
        OpenDispute {
            from_id: "m_new".into(),
            to_id: "m_old".into(),
            resolution_strategy: strategy.into(),
            to_superseded,
            from_superseded: false,
        }
    }

    #[test]
    fn preference_classifies_and_drains_as_coexist() {
        assert_eq!(classify("cross_user_preference"), DisputeKind::Preference);
        let v = drain_decision(&dispute("cross_user_preference", false));
        assert_eq!(v, DrainVerdict::Resolve("coexist_preference".into()));
    }

    #[test]
    fn unknown_strategy_is_factual_by_default() {
        // Never silently treat a real claim as a mere preference.
        assert_eq!(classify("cross_user_assertion"), DisputeKind::Factual);
        assert_eq!(classify(""), DisputeKind::Factual);
    }

    #[test]
    fn live_factual_dispute_is_kept() {
        let v = drain_decision(&dispute("cross_user_factual", false));
        assert_eq!(v, DrainVerdict::Keep);
    }

    #[test]
    fn superseded_factual_dispute_is_retired() {
        let v = drain_decision(&dispute("cross_user_factual", true));
        assert_eq!(v, DrainVerdict::Resolve("superseded_side".into()));
    }

    #[test]
    fn summary_tallies_each_outcome() {
        let mut s = DebtSummary::default();
        for d in [
            dispute("cross_user_preference", false), // drained pref
            dispute("cross_user_factual", true),     // drained superseded
            dispute("cross_user_factual", false),    // kept live
        ] {
            let v = drain_decision(&d);
            s.record(&d, &v);
        }
        assert_eq!(
            s,
            DebtSummary {
                scanned: 3,
                drained_preference: 1,
                drained_superseded: 1,
                kept_live: 1,
                notified: 0,
            }
        );
    }
}
