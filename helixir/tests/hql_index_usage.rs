//! Regression checks for the HelixDB v2 indexed lookup contract.

const SCHEMA: &str = include_str!("../schema/schema.hx");
const QUERIES: &str = include_str!("../schema/queries.hx");

#[test]
fn memory_identity_fields_remain_indexed() {
    assert!(SCHEMA.contains("INDEX memory_id: String"));
    assert!(SCHEMA.contains("INDEX content_key: String"));
}

#[test]
fn exact_memory_lookups_never_fall_back_to_full_label_scans() {
    for field in ["memory_id", "content_key"] {
        let full_scan = format!("N<Memory>::WHERE(_::{{{field}}}::EQ(");
        assert!(
            !QUERIES.contains(&full_scan),
            "exact Memory.{field} lookups must use N<Memory>({{{field}: value}}) so HelixDB emits n_from_index"
        );
    }
}

#[test]
fn non_unique_content_key_reads_are_explicitly_collected() {
    assert!(QUERIES.contains("N<Memory>({content_key: content_key})::RANGE(0, 1000000)"));
    assert!(QUERIES.contains("QUERY restampContentKeyGroup"));
    assert!(QUERIES.contains("updated_count <- updated::COUNT"));
    assert!(QUERIES.contains("RETURN updated_count"));
}
