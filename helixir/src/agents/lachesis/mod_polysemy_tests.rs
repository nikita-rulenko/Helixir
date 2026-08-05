use super::*;
use std::collections::HashMap;

fn adj_of(edges: &[(&str, &str)]) -> HashMap<String, Vec<(String, f64)>> {
    let mut adj: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    for (a, b) in edges {
        adj.entry((*a).into()).or_default().push(((*b).into(), 1.0));
        adj.entry((*b).into()).or_default().push(((*a).into(), 1.0));
    }
    adj
}

/// Two dense cliques joined ONLY through one pivot — the benchmarking
/// shape. The guard must find the pivot; label propagation must put the
/// cliques in different communities.
#[test]
fn pivot_between_two_cliques_is_a_polysemous_bridge() {
    let adj = adj_of(&[
        // finance clique
        ("energy", "commodity"),
        ("commodity", "markets"),
        ("energy", "markets"),
        // software clique
        ("debugging", "queries"),
        ("queries", "testing"),
        ("debugging", "testing"),
        // the two-faced pivot
        ("markets", "benchmarking"),
        ("benchmarking", "debugging"),
    ]);
    let comm = communities(&adj);
    assert_ne!(
        comm["markets"], comm["debugging"],
        "cliques must land in different communities: {comm:?}"
    );

    let path: Vec<(String, f64)> = ["energy", "markets", "benchmarking", "debugging", "queries"]
        .iter()
        .map(|s| (s.to_string(), 1.0))
        .collect();
    // The pivot's own label lands on one side, so the detected crossing
    // is the hop AT or NEXT TO the pivot (index 1 or 2) — either
    // truncation point drops the cross-domain jump.
    let bridge = polysemous_bridge(&path, &adj, &comm);
    assert!(
        matches!(bridge, Some(1) | Some(2)),
        "the crossing must be detected around the pivot: {bridge:?}"
    );
}

/// A chain inside ONE community never trips the guard, however long.
#[test]
fn intra_community_chain_is_untouched() {
    let adj = adj_of(&[
        ("retrieval", "knowledge"),
        ("knowledge", "databases"),
        ("databases", "version-control"),
        ("retrieval", "databases"),
        ("knowledge", "version-control"),
    ]);
    let comm = communities(&adj);
    let path: Vec<(String, f64)> = ["retrieval", "knowledge", "databases", "version-control"]
        .iter()
        .map(|s| (s.to_string(), 1.0))
        .collect();
    assert_eq!(polysemous_bridge(&path, &adj, &comm), None);
}

/// A cross-community hop whose endpoints ALSO share a direct link is a
/// genuine bridge (two domains really touching), not polysemy — kept.
#[test]
fn genuine_cross_domain_link_is_kept() {
    let adj = adj_of(&[
        ("a1", "a2"),
        ("a1", "a3"),
        ("a2", "a3"),
        ("b1", "b2"),
        ("b1", "b3"),
        ("b2", "b3"),
        ("a3", "pivot"),
        ("pivot", "b1"),
        // the endpoints of the pivot hop know each other directly:
        ("a3", "b1"),
    ]);
    let comm = communities(&adj);
    let path: Vec<(String, f64)> = ["a1", "a3", "pivot", "b1", "b2"]
        .iter()
        .map(|s| (s.to_string(), 1.0))
        .collect();
    assert_eq!(
        polysemous_bridge(&path, &adj, &comm),
        None,
        "direct a3-b1 adjacency proves the domains genuinely touch"
    );
}
