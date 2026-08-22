//! Read-only DB proof for the physical schema lifecycle inventory (#157).
//!
//! ```text
//! HELIX_E2E=1 cargo test --test schema_inventory_e2e -- --ignored --nocapture
//! ```

use helixir::db::HelixClient;
use helixir::schema_inventory::{SchemaLifecycle, census, declarations};

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 and a live HelixDB with the current 185-query schema"]
async fn every_physical_declaration_has_a_server_side_count() {
    assert_eq!(
        std::env::var("HELIX_E2E").unwrap_or_default(),
        "1",
        "Set HELIX_E2E=1 when running this test with --ignored"
    );
    let client = HelixClient::from_env().expect("HelixClient::from_env");
    let report = census(&client).await;
    let expected = declarations().count();
    assert_eq!(report.items.len(), expected);
    assert!(
        report.failed_queries.is_empty(),
        "deployed schema rejected census queries: {:?}",
        report.failed_queries
    );
    assert_eq!(
        report.counted, expected,
        "deployed schema lacks census coverage"
    );

    for item in &report.items {
        assert!(
            item.count.is_some(),
            "missing count for {}",
            item.declaration.physical_name()
        );
        if item.declaration.lifecycle == SchemaLifecycle::Deprecated {
            assert_eq!(
                item.count,
                Some(0),
                "deprecated declaration still contains data; execute its migration before removal"
            );
        }
    }
    println!(
        "schema inventory v{}: {} active, {} reserved, {} deprecated, {} counted",
        report.inventory_version, report.active, report.reserved, report.deprecated, report.counted
    );
}
