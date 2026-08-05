use super::*;

/// The two failures observed live from the fallback tier on 2026-07-02:
/// a missing top-level `entities` array, and objects where strings were
/// expected inside `memories[].entities`. Both must parse, not fall back
/// to the blob path.
#[test]
fn parses_weak_model_json_shapes() {
    // Shape 1: no top-level entities/relations at all.
    let r: ExtractionResult = serde_json::from_str(
            r#"{"memories":[{"text":"the deploy failed","memory_type":"fact","certainty":80,"importance":50,"entities":[]}]}"#,
        )
        .expect("missing top-level arrays must default");
    assert_eq!(r.memories.len(), 1);
    assert!(r.entities.is_empty() && r.relations.is_empty());

    // Shape 2: entity OBJECTS inside memory.entities + float certainty +
    // context as an object + missing memory_type.
    let r: ExtractionResult = serde_json::from_str(
            r#"{"memories":[{"text":"the token expired","certainty":0.9,"importance":"60","entities":[{"id":"e1","name":"token"}],"context":{"name":"auth"}}],"entities":[{"name":"token"}],"relations":[]}"#,
        )
        .expect("weak-model shapes must parse");
    let m = &r.memories[0];
    assert_eq!(m.memory_type, "fact");
    assert_eq!(m.certainty, 90);
    assert_eq!(m.importance, 60);
    assert_eq!(m.entities, vec!["e1".to_string()]);
    assert_eq!(m.context.as_deref(), Some("auth"));
    assert_eq!(r.entities[0].entity_type, "Object");
}
