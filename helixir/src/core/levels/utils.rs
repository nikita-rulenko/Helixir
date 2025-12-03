

use super::definitions::{get_level_definition, LEVELS};
use super::models::{AccumulatedSchema, HelixirLevel, LevelDefinition};


pub fn validate_level_dependencies(target_level: HelixirLevel) -> Vec<HelixirLevel> {
    let definition = get_level_definition(target_level);
    let mut required: std::collections::HashSet<HelixirLevel> =
        definition.dependencies.iter().cloned().collect();

    
    for dep in &definition.dependencies {
        let dep_def = get_level_definition(*dep);
        required.extend(dep_def.dependencies.iter().cloned());
    }

    
    let mut sorted: Vec<_> = required.into_iter().collect();
    sorted.sort();
    sorted
}


pub fn get_deployment_order(max_level: HelixirLevel) -> Vec<HelixirLevel> {
    max_level.levels_up_to()
}


pub fn get_accumulated_schema(max_level: HelixirLevel) -> AccumulatedSchema {
    let mut schema = AccumulatedSchema::default();

    for level in get_deployment_order(max_level) {
        let definition = get_level_definition(level);
        schema.add_level(definition);
    }

    schema
}


pub fn get_accumulated_queries(max_level: HelixirLevel) -> Vec<String> {
    let mut queries = Vec::new();

    for level in get_deployment_order(max_level) {
        let definition = get_level_definition(level);
        queries.extend(definition.queries.clone());
    }

    queries
}


pub fn format_level_info(level: HelixirLevel) -> String {
    let definition = get_level_definition(level);

    let mut output = String::new();
    output.push_str(&format!("\n{}\n", "=".repeat(60)));
    output.push_str(&format!("Level {}: {}\n", level.number(), definition.name));
    output.push_str(&format!("{}\n", "=".repeat(60)));
    output.push_str(&format!("Description: {}\n", definition.description));
    output.push_str("\nSchema:\n");
    output.push_str(&format!(
        "  Nodes: {}\n",
        if definition.schema_nodes.is_empty() {
            "none".to_string()
        } else {
            definition.schema_nodes.join(", ")
        }
    ));
    output.push_str(&format!(
        "  Edges: {}\n",
        if definition.schema_edges.is_empty() {
            "none".to_string()
        } else {
            definition.schema_edges.join(", ")
        }
    ));
    if !definition.schema_extends.is_empty() {
        output.push_str(&format!("  Extends: {}\n", definition.schema_extends.join(", ")));
    }
    output.push_str(&format!("\nQueries: {}\n", definition.queries.join(", ")));
    output.push_str(&format!(
        "Dependencies: {:?}\n",
        definition.dependencies.iter().map(|d| d.number()).collect::<Vec<_>>()
    ));
    if !definition.notes.is_empty() {
        output.push_str(&format!("\nNotes:\n{}\n", definition.notes));
    }
    output.push_str(&format!("{}\n", "=".repeat(60)));

    output
}


pub fn format_pyramid() -> String {
    let mut output = String::new();
    output.push_str(&format!("\n{}\n", "=".repeat(60)));
    output.push_str("HELIXIR LEVEL PYRAMID\n");
    output.push_str(&format!("{}\n", "=".repeat(60)));

    for i in (0..=5).rev() {
        if let Some(level) = HelixirLevel::from_number(i) {
            let definition = get_level_definition(level);
            let indent = " ".repeat((5 - i as usize) * 2);
            output.push_str(&format!("{}Level {}: {}\n", indent, i, definition.name));
            if i > 0 {
                output.push_str(&format!("{}  ↓\n", indent));
            }
        }
    }

    output.push_str(&format!("{}\n", "=".repeat(60)));
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deployment_order() {
        let order = get_deployment_order(HelixirLevel::Level3);
        assert_eq!(order.len(), 4);
        assert_eq!(order[0], HelixirLevel::Level0);
        assert_eq!(order[3], HelixirLevel::Level3);
    }

    #[test]
    fn test_accumulated_schema() {
        let schema = get_accumulated_schema(HelixirLevel::Level1);
        assert!(schema.nodes.contains(&"User".to_string()));
        assert!(schema.nodes.contains(&"Memory".to_string()));
    }

    #[test]
    fn test_dependencies() {
        let deps = validate_level_dependencies(HelixirLevel::Level3);
        assert!(deps.contains(&HelixirLevel::Level0));
        assert!(deps.contains(&HelixirLevel::Level1));
        assert!(deps.contains(&HelixirLevel::Level2));
        assert!(!deps.contains(&HelixirLevel::Level3));
    }
}

