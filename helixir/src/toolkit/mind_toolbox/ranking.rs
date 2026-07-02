//! Numeric helpers shared by every ranking sort, hardened against NaN.
//!
//! `f32`/`f64` scores can turn into NaN at boundaries we don't fully control —
//! e.g. a degenerate vector from HelixDB yielding a div-by-zero cosine. Since
//! `f64::partial_cmp` returns `None` for NaN, the ranking sorts used to call
//! `.unwrap()` on it, which **panics**; in the MCP stdio server that panic
//! unwinds the main loop and silently kills the whole process with no crash
//! dump (#41). And `f64::clamp` does *not* sanitize NaN — it only bounds
//! out-of-range values, so a NaN flows straight through the scoring math.
//!
//! These two helpers close both holes: comparisons treat NaN as `Equal` so a
//! sort can never panic, and a non-finite score is mapped to `0.0` at the
//! scoring boundary so a degenerate result ranks lowest instead of poisoning
//! the ranking.

use std::cmp::Ordering;

/// Descending comparator over two `f64` scores, NaN-safe.
///
/// Higher score first; any NaN sinks to the bottom. Never panics — the old
/// `partial_cmp().unwrap()` panicked on NaN and killed the MCP process (#41).
///
/// Note this is deliberately *not* `partial_cmp().unwrap_or(Equal)`: making
/// NaN compare `Equal` to everything breaks transitivity, so `sort_by` can
/// shuffle the *finite* scores around an interspersed NaN. Sinking NaN to a
/// consistent extreme keeps a valid total order, so a stray degenerate score
/// can neither crash the sort nor reorder the real results — it just ranks
/// last.
pub fn desc(a: &f64, b: &f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater, // a is NaN → a ranks after b
        (false, true) => Ordering::Less,    // b is NaN → a ranks before b
        (false, false) => b.partial_cmp(a).unwrap_or(Ordering::Equal),
    }
}

/// Map a non-finite score (NaN, ±∞) to `0.0`, then clamp into `[0, 1]`.
///
/// A degenerate score must rank lowest, not propagate NaN through every
/// subsequent combine/sort. Use this in place of a bare `.clamp(0.0, 1.0)`
/// wherever externally-sourced scores enter the ranking math.
pub fn sanitize_unit(x: f64) -> f64 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desc_orders_higher_first() {
        let mut v = vec![0.2_f64, 0.9, 0.5];
        v.sort_by(|a, b| desc(a, b));
        assert_eq!(v, vec![0.9, 0.5, 0.2]);
    }

    #[test]
    fn desc_sinks_nan_to_the_bottom_and_keeps_finites_ordered() {
        // The exact failure mode of #41: NaNs in the slice. The old
        // `partial_cmp().unwrap()` panicked here; `desc` must not — and it
        // must not reorder the finite scores either.
        let mut v = vec![0.9_f64, f64::NAN, 0.3, f64::NAN, 0.6];
        v.sort_by(|a, b| desc(a, b));
        let finite: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
        assert_eq!(finite, vec![0.9, 0.6, 0.3], "finite scores stay descending");
        assert!(
            v[3].is_nan() && v[4].is_nan(),
            "NaNs must sink to the bottom, got {v:?}"
        );
    }

    #[test]
    fn sanitize_unit_maps_non_finite_to_zero() {
        assert_eq!(sanitize_unit(f64::NAN), 0.0);
        assert_eq!(sanitize_unit(f64::INFINITY), 0.0);
        assert_eq!(sanitize_unit(f64::NEG_INFINITY), 0.0);
    }

    #[test]
    fn sanitize_unit_clamps_and_passes_finite() {
        assert_eq!(sanitize_unit(1.5), 1.0);
        assert_eq!(sanitize_unit(-0.2), 0.0);
        assert_eq!(sanitize_unit(0.73), 0.73);
    }
}
