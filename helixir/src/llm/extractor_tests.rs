use super::*;

#[test]
fn test_extraction_result_serialization() {
    let result = ExtractionResult {
        memories: vec![ExtractedMemory {
            text: "User prefers Rust".to_string(),
            memory_type: "preference".to_string(),
            certainty: 90,
            importance: 70,
            entities: vec!["rust".to_string()],
            context: Some("work".to_string()),
        }],
        entities: vec![ExtractedEntity {
            id: "rust".to_string(),
            name: "Rust".to_string(),
            entity_type: "concept".to_string(),
            relations: None,
        }],
        relations: vec![],
    };

    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("preference"));
}
