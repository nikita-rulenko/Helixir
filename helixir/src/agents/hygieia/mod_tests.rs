use super::*;

#[test]
fn flood_tracker_pauses_after_consecutive_caps_only() {
    let mut t = FloodTracker::default();
    assert_eq!(t.observe(6, 6, 3), FloodVerdict::Capped(1));
    assert_eq!(t.observe(2, 6, 3), FloodVerdict::Ok, "streak broken");
    assert_eq!(t.observe(6, 6, 3), FloodVerdict::Capped(1));
    assert_eq!(t.observe(6, 6, 3), FloodVerdict::Capped(2));
    assert_eq!(t.observe(6, 6, 3), FloodVerdict::PauseInsights);
    assert_eq!(t.observe(6, 6, 3), FloodVerdict::Ok, "latched: fires once");
}

#[test]
fn mem_usage_cell_parses_docker_units() {
    let s = parse_mem_usage("557.3MiB / 3GiB").unwrap();
    assert!((s.used_mib - 557.3).abs() < 0.01);
    assert!((s.limit_mib - 3072.0).abs() < 0.01);
    assert!((s.pct() - 18.14).abs() < 0.1);
    assert!(parse_mem_usage("garbage").is_none());
}

#[test]
fn orphan_policy_flags_lone_fresh_daemon() {
    use crate::toolkit::tooling_manager::swarm::AgentPresence;
    let now = chrono::Utc::now();
    let mk = |id: &str, role: &str, ago_secs: i64| AgentPresence {
        agent_id: id.into(),
        name: id.into(),
        role: role.into(),
        host: "h".into(),
        last_seen: (now - chrono::Duration::seconds(ago_secs)).to_rfc3339(),
        status: "working".into(),
    };
    // Fresh daemon + stale workers → orphan.
    let roster = vec![
        mk("daemon:claude", "daemon", 30),
        mk("zc-a", "developer", 90_000),
    ];
    assert_eq!(
        orphan_daemon(&roster, now, 6 * 3600),
        Some("daemon:claude".to_string())
    );
    // A recently-active worker clears the suspicion.
    let roster2 = vec![
        mk("daemon:claude", "daemon", 30),
        mk("zc-a", "developer", 600),
    ];
    assert_eq!(orphan_daemon(&roster2, now, 6 * 3600), None);
    // No daemon → nothing to flag.
    let roster3 = vec![mk("zc-a", "developer", 90_000)];
    assert_eq!(orphan_daemon(&roster3, now, 6 * 3600), None);
}
