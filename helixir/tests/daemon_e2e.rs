//! Daemon (#42 / Moira) integration smoke: on-call mode runs exactly one pass
//! through the full stack (Daemon → Orchestrator → Clotho/Atropos) and the
//! `on_pass` sink fires once. Continuous mode is the same loop minus the early
//! break; it can't be asserted without a clock, so on-call covers the runtime.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt \
//!   cargo test -p helixir --test daemon_e2e -- --ignored --nocapture
//! ```

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use helixir::agents::daemon::DaemonConfig;
use helixir::agents::orchestrator::PassConfig;
use helixir::core::HelixirClient;

mod common;

fn token() -> String {
    format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + LLM + Category schema deployed"]
async fn daemon_on_call_runs_exactly_one_pass() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");
    let actor = common::e2e_actor();
    let group = common::e2e_group();
    let admin = client.admin_as(&actor).await.expect("RBAC admin");

    let run = token();
    let user = format!("daem_{run}");
    for i in 0..2 {
        let fact = format!("Run {run} item {i}: the background worker drained queue slot {i}.");
        client
            .add_as_in_group(&actor, &fact, &user, None, None, Some(&group))
            .await
            .expect("add");
    }

    let cfg = DaemonConfig {
        user: user.clone(),
        interval: Duration::from_secs(1),
        once: true,
        host: "test-host".to_string(),
        pass: PassConfig {
            max_seeds: 4,
            max_hops: 3,
            ..PassConfig::default()
        },
        // Every stage on every pass — the pre-cadence behavior this e2e asserts.
        clotho_every: 1,
        insight_every: 1,
        merge_every: 1,
        reconcile_every: 1,
        // 0 = never: this e2e asserts the pre-stitch pipeline stages; the
        // stitch stage has its own suite (stitch_e2e).
        stitch_every: 0,
        verify_every: 0,
    };

    let mut passes = 0u64;
    let mut last_pass_no = 0u64;
    admin
        .daemon()
        .run(cfg, |pass, _run| {
            passes += 1;
            last_pass_no = pass;
        })
        .await
        .expect("daemon run");

    println!("\n==== daemon_e2e ====\non-call passes: {passes}");
    assert_eq!(passes, 1, "on-call mode runs exactly one pass");
    assert_eq!(last_pass_no, 1, "the single pass is numbered 1");
}

#[tokio::test]
#[ignore = "isolated HelixDB profiling probe selected by HELIXIR_PROFILE_STAGE"]
async fn daemon_profile_stage_runs_exactly_one_pass() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");
    let stage = std::env::var("HELIXIR_PROFILE_STAGE").expect("HELIXIR_PROFILE_STAGE");
    let (cadence, watchdog) = match stage.as_str() {
        "baseline" => ([0, 0, 0, 0], false),
        "clotho" => ([1, 0, 0, 0], false),
        "insights" => ([0, 1, 0, 0], false),
        "reconcile" => ([0, 0, 0, 1], false),
        "merge" => ([0, 0, 1, 0], false),
        "hygieia" => ([0, 0, 0, 0], true),
        other => panic!("unsupported HELIXIR_PROFILE_STAGE={other}"),
    };

    let client = HelixirClient::from_env().expect("from_env");
    assert_eq!(
        client.config().watchdog.enabled,
        watchdog,
        "the generated profiling config must isolate Hygieia"
    );
    client.initialize().await.expect("initialize");
    if stage == "merge" {
        assert!(
            helixir::llm::nli::status().installed,
            "merge profiling is invalid without the mandatory NLI model"
        );
    }

    let actor = common::e2e_actor();
    let admin = client.admin_as(&actor).await.expect("RBAC admin");
    let user = std::env::var("HELIXIR_PROFILE_USER").expect("HELIXIR_PROFILE_USER");

    let cfg = DaemonConfig {
        user,
        interval: Duration::from_secs(1),
        once: true,
        host: "profile-host".to_string(),
        pass: PassConfig {
            max_seeds: 4,
            max_hops: 3,
            ..PassConfig::default()
        },
        clotho_every: cadence[0],
        insight_every: cadence[1],
        merge_every: cadence[2],
        reconcile_every: cadence[3],
        stitch_every: 0,
        verify_every: 0,
    };
    let mut orchestrator_callbacks = 0u64;
    admin
        .daemon()
        .run(cfg, |_pass, _run| orchestrator_callbacks += 1)
        .await
        .expect("profile stage");

    let linger_ms = std::env::var("HELIXIR_PROFILE_LINGER_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    if linger_ms > 0 {
        tokio::time::sleep(Duration::from_millis(linger_ms)).await;
    }

    let expected_callbacks = u64::from(cadence[0] != 0 || cadence[1] != 0);
    assert_eq!(orchestrator_callbacks, expected_callbacks);
    println!(
        "profile stage {stage} completed with {orchestrator_callbacks} orchestrator callback(s)"
    );
}
