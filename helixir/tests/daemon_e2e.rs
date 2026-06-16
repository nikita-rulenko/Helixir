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

fn token() -> String {
    format!(
        "{:x}",
        SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos()
    )
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + LLM + Category schema deployed"]
async fn daemon_on_call_runs_exactly_one_pass() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    let client = HelixirClient::from_env().expect("from_env");
    client.initialize().await.expect("initialize");

    let run = token();
    let user = format!("daem_{run}");
    for i in 0..2 {
        let fact = format!("Run {run} item {i}: the background worker drained queue slot {i}.");
        client.add(&fact, &user, None, None).await.expect("add");
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
    };

    let mut passes = 0u64;
    let mut last_pass_no = 0u64;
    client
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
