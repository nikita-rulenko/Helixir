use super::*;

fn snapshot() -> CategorySnapshot {
    CategorySnapshot {
        categories: BTreeMap::from([
            (
                "domain".to_string(),
                CategoryRecord {
                    name: "Engineering".into(),
                    kind: "domain".into(),
                    description: String::new(),
                },
            ),
            (
                "concept".to_string(),
                CategoryRecord {
                    name: "Rust".into(),
                    kind: "concept".into(),
                    description: String::new(),
                },
            ),
            (
                "test".to_string(),
                CategoryRecord {
                    name: "Fixture".into(),
                    kind: "test".into(),
                    description: String::new(),
                },
            ),
        ]),
        memories: BTreeMap::new(),
        memory_categories: HashMap::new(),
        memory_groups: HashMap::new(),
        memory_agents: HashMap::new(),
        parent_by_category: HashMap::from([("concept".to_string(), "domain".to_string())]),
        relations: Vec::new(),
        snapshot_at: chrono::Utc::now(),
    }
}

#[test]
fn persisted_hierarchy_exposes_only_real_direct_children() {
    let snapshot = snapshot();
    let allowed = HashSet::new();
    assert!(direct_children(&snapshot, "domain", &allowed).is_empty());
}

#[test]
fn identity_filter_requires_an_explicit_kind() {
    assert!(SelectedIdentity::parse("codex").is_err());
    let selected = SelectedIdentity::parse("agent:codex").unwrap().unwrap();
    assert_eq!(selected.kind, "agent");
    assert_eq!(selected.value, "codex");
}

#[test]
fn untagged_memories_are_reachable_through_the_uncategorized_cluster() {
    let mut snapshot = snapshot();
    snapshot.memories.insert(
        "internal-memory".to_string(),
        serde_json::from_value(serde_json::json!({
            "id": "internal-memory",
            "memory_id": "mem_without_category",
            "content": "A still-classifying fact"
        }))
        .unwrap(),
    );
    let allowed = HashSet::from(["internal-memory".to_string()]);
    assert_eq!(members_for(&snapshot, UNCATEGORIZED_ID, &allowed), allowed);
}

#[test]
fn category_page_is_bounded_and_sorted_by_memory_count() {
    let mut snapshot = snapshot();
    for index in 0..30 {
        let category = format!("category-{index:02}");
        let memory = format!("memory-{index:02}");
        snapshot.categories.insert(
            category.clone(),
            CategoryRecord {
                name: category.clone(),
                kind: "concept".into(),
                description: String::new(),
            },
        );
        snapshot.memories.insert(
            memory.clone(),
            serde_json::from_value(serde_json::json!({
                "id": memory,
                "memory_id": format!("mem-{index:02}"),
                "content": "bounded category"
            }))
            .unwrap(),
        );
        snapshot.memory_categories.insert(memory, vec![category]);
    }
    let allowed = snapshot.memories.keys().cloned().collect();
    let (categories, _, total, page, pages) =
        category_page(&snapshot, &allowed, None, BTreeSet::new(), None, 1);
    assert_eq!(categories.len(), CATEGORY_PAGE_SIZE);
    assert_eq!(total, 30);
    assert_eq!((page, pages), (1, 30_usize.div_ceil(CATEGORY_PAGE_SIZE)));
}
