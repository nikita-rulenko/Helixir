use super::*;

fn e(t: &str, w: f64) -> ChainEdge<'_> {
    ChainEdge {
        edge_type: t,
        weight: w,
    }
}

#[test]
fn reasoning_backed_chain_is_a_hypothesis() {
    let v = assess(&[e("IMPLIES", 0.72), e("BECAUSE", 0.70)], 0.5, 0.5);
    assert_eq!(v.label, EpistemicLabel::PlausibleHypothesis);
    assert!(
        v.requires_verification,
        "a hypothesis is never asserted as truth"
    );
    assert!(
        v.coherence > 0.7 && v.coherence <= 0.72,
        "geomean ~0.71, got {}",
        v.coherence
    );
    assert_eq!(v.reasoning_support, 1.0);
}

#[test]
fn bare_association_chain_is_apophenia() {
    // Two memories linked only by shared tags — the canonical apophenia case.
    let v = assess(&[e("VIA_CATEGORY", 0.5), e("VIA_CATEGORY", 0.5)], 0.5, 0.5);
    assert_eq!(v.label, EpistemicLabel::LikelyApophenia);
    assert!(!v.requires_verification);
    assert_eq!(v.reasoning_support, 0.0);
}

#[test]
fn weak_reasoning_chain_is_apophenia() {
    // Reasoning-typed but the per-hop confidence is too low to trust.
    let v = assess(&[e("MEMORY_RELATION", 0.30), e("IMPLIES", 0.35)], 0.5, 0.5);
    assert_eq!(v.label, EpistemicLabel::LikelyApophenia);
    assert!(v.coherence < 0.5);
}

#[test]
fn geometric_mean_is_length_fair() {
    // A long, firmly-reasoned chain must not be rejected just for being long
    // (a raw weight product would underflow the bar).
    let long: Vec<ChainEdge> = (0..8).map(|_| e("IMPLIES", 0.7)).collect();
    let v = assess(&long, 0.5, 0.5);
    assert_eq!(v.label, EpistemicLabel::PlausibleHypothesis);
    assert!(
        (v.coherence - 0.7).abs() < 1e-9,
        "geomean of all-0.7 is 0.7, got {}",
        v.coherence
    );
}

#[test]
fn mixed_chain_keeps_a_reasoning_majority() {
    // One associative bridge among reasoning hops still passes the support bar.
    let v = assess(
        &[e("IMPLIES", 0.7), e("VIA_CATEGORY", 0.6), e("BECAUSE", 0.7)],
        0.5,
        0.5,
    );
    assert!(v.reasoning_support >= 0.5);
    assert_eq!(v.label, EpistemicLabel::PlausibleHypothesis);
}

#[test]
fn empty_is_not_a_chain() {
    assert_eq!(assess(&[], 0.5, 0.5).label, EpistemicLabel::LikelyApophenia);
}

// --- PMI subset-overlap routing (the cross-domain apophenia guard) ---

#[test]
fn pmi_thick_axis_gates_itself_out() {
    // A subset covering the whole universe co-occurs with anything at exactly
    // chance → PMI 0, regardless of overlap. The raw-material problem, solved.
    assert!(pmi(10, 100, 10, 100).abs() < 1e-9);
}

#[test]
fn pmi_specific_pair_scores_high() {
    // Two small subsets fully overlapping, far above chance.
    assert!(pmi(5, 5, 5, 1000) > 3.0);
}

#[test]
fn pmi_no_overlap_is_neg_inf() {
    assert_eq!(pmi(10, 10, 0, 1000), f64::NEG_INFINITY);
}

#[test]
fn pmi_specific_beats_thick() {
    let specific = pmi(5, 5, 5, 100); // narrow, fully overlapping
    let thick = pmi(5, 100, 5, 100); // B spans the whole universe
    assert!(
        specific > thick,
        "specific {specific} should beat thick {thick}"
    );
}
