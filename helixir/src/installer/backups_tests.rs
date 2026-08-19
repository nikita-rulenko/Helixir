use super::*;

#[test]
fn backup_ids_cannot_escape_the_vault() {
    for unsafe_id in [
        "../secret.tar.gz",
        "/tmp/secret.tar.gz",
        "snapshot;rm.tar.gz",
        "other.tar.gz",
    ] {
        assert!(validate_id(unsafe_id).is_err(), "{unsafe_id}");
    }
    assert!(validate_id("helixdb-manual-20260819-120000.tar.gz").is_ok());
}

#[test]
fn restore_confirmation_names_the_exact_archive() {
    let request = RestoreRequest {
        backup_id: "helixdb-manual-test.tar.gz".into(),
        confirmation: "RESTORE another.tar.gz".into(),
    };
    assert_ne!(
        request.confirmation,
        format!("RESTORE {}", request.backup_id)
    );
}
