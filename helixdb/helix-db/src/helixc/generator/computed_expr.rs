// Copyright 2025 HelixDB Inc.
// SPDX-License-Identifier: AGPL-3.0

//! Code generation for computed expression fields in RETURN statements.
//!
//! Handles expressions like `ADD(_::Out<HasRailwayCluster>::COUNT, _::Out<HasObjectCluster>::COUNT)`
//! in object projections.
//!
//! Since `Value` implements standard math ops (`Add`, `Sub`, `Mul`, `Div`), the generated
//! code can directly use operators on `Value` types without conversion.

use crate::helixc::parser::types::{Expression, ExpressionType, MathFunction};

/// Generate Rust code for a computed expression field.
///
/// This handles math function calls (ADD, SUB, MUL, etc.) where arguments
/// may be traversal expressions (like `_::Out<HasCluster>::COUNT`).
///
/// # Arguments
/// * `expression` - The parsed expression to generate code for
/// * `item_var` - The variable name for the current item (e.g., "cluster" in a collection iteration)
///
/// # Returns
/// Rust code string that evaluates to a `Value`
pub fn generate_computed_expression(expression: &Expression, item_var: &str) -> String {
    match &expression.expr {
        ExpressionType::MathFunctionCall(call) => {
            // Generate code for each argument recursively
            let args: Vec<String> = call
                .args
                .iter()
                .map(|arg| generate_computed_expression(arg, item_var))
                .collect();

            match &call.function {
                MathFunction::Add => {
                    if args.len() == 2 {
                        format!("({}) + ({})", args[0], args[1])
                    } else {
                        "Value::Empty".to_string()
                    }
                }
                MathFunction::Sub => {
                    if args.len() == 2 {
                        format!("({}) - ({})", args[0], args[1])
                    } else {
                        "Value::Empty".to_string()
                    }
                }
                MathFunction::Mul => {
                    if args.len() == 2 {
                        format!("({}) * ({})", args[0], args[1])
                    } else {
                        "Value::Empty".to_string()
                    }
                }
                MathFunction::Div => {
                    if args.len() == 2 {
                        format!("({}) / ({})", args[0], args[1])
                    } else {
                        "Value::Empty".to_string()
                    }
                }
                MathFunction::Pow => {
                    // pow is not implemented via traits, need special handling
                    if args.len() == 2 {
                        format!("({}).pow(&({}))", args[0], args[1])
                    } else {
                        "Value::Empty".to_string()
                    }
                }
                MathFunction::Mod => {
                    if args.len() == 2 {
                        format!("({}) % ({})", args[0], args[1])
                    } else {
                        "Value::Empty".to_string()
                    }
                }
                MathFunction::Abs => {
                    if args.len() == 1 {
                        format!("({}).abs()", args[0])
                    } else {
                        "Value::Empty".to_string()
                    }
                }
                MathFunction::Sqrt => {
                    if args.len() == 1 {
                        format!("({}).sqrt()", args[0])
                    } else {
                        "Value::Empty".to_string()
                    }
                }
                MathFunction::Min => {
                    if args.len() == 2 {
                        format!("({}).min(({}))", args[0], args[1])
                    } else {
                        "Value::Empty".to_string()
                    }
                }
                MathFunction::Max => {
                    if args.len() == 2 {
                        format!("({}).max(({}))", args[0], args[1])
                    } else {
                        "Value::Empty".to_string()
                    }
                }
                _ => "Value::Empty /* unsupported math function */".to_string(),
            }
        }
        ExpressionType::Traversal(traversal) => {
            // Generate traversal code that returns a Value directly
            generate_traversal_value(traversal, item_var)
        }
        ExpressionType::IntegerLiteral(val) => {
            format!("Value::from({})", val)
        }
        ExpressionType::FloatLiteral(val) => {
            format!("Value::from({})", val)
        }
        ExpressionType::Identifier(id) => {
            // Identifier - could be a variable reference that's already a Value
            id.clone()
        }
        _ => "Value::Empty".to_string(),
    }
}

/// Generate traversal code that returns a Value.
fn generate_traversal_value(
    traversal: &crate::helixc::parser::types::Traversal,
    item_var: &str,
) -> String {
    use crate::helixc::parser::types::{GraphStepType, StartNode, StepType};

    // Check if this starts with anonymous (_)
    let source_var = match &traversal.start {
        StartNode::Anonymous => item_var.to_string(),
        StartNode::Identifier(id) => id.clone(),
        _ => item_var.to_string(),
    };

    // Check for direct property access pattern:
    // - Exactly one step
    // - That step is StepType::Object with a single field
    // - No graph traversal steps (Out/In/OutE/InE)
    // This generates direct property access like `item.get_property("price")`
    // instead of wrapping in G::from_iter()
    if traversal.steps.len() == 1
        && let StepType::Object(obj) = &traversal.steps[0].step
        && obj.fields.len() == 1
    {
        let field = &obj.fields[0];
        let prop_name = &field.key;
        if prop_name == "id" || prop_name == "ID" {
            return format!("Value::from(uuid_str({}.id(), &arena))", source_var);
        } else if prop_name == "label" || prop_name == "Label" {
            return format!("Value::from({}.label())", source_var);
        } else {
            return format!(
                "{}.get_property(\"{}\").expect(\"property not found\").clone()",
                source_var, prop_name
            );
        }
    }

    // Build the traversal steps for graph traversals
    let mut steps = String::new();
    let mut ends_with_count = false;

    for step_info in &traversal.steps {
        match &step_info.step {
            StepType::Node(graph_step) => match &graph_step.step {
                GraphStepType::Out(label) => {
                    steps.push_str(&format!(".out_node(\"{}\")", label));
                }
                GraphStepType::In(label) => {
                    steps.push_str(&format!(".in_node(\"{}\")", label));
                }
                GraphStepType::OutE(label) => {
                    steps.push_str(&format!(".out_e(\"{}\")", label));
                }
                GraphStepType::InE(label) => {
                    steps.push_str(&format!(".in_e(\"{}\")", label));
                }
                _ => {}
            },
            StepType::Count => {
                steps.push_str(".count_to_val()");
                ends_with_count = true;
            }
            StepType::Object(obj) => {
                // Property access
                if let Some(field) = obj.fields.first() {
                    let prop_name = &field.key;
                    if prop_name == "id" || prop_name == "ID" {
                        steps.push_str(
                            ".map(|item| item.map(|v| Value::from(uuid_str(v.id(), &arena))))",
                        );
                    } else {
                        steps.push_str(&format!(".get_property(\"{}\")", prop_name));
                    }
                }
            }
            _ => {}
        }
    }

    if ends_with_count {
        // COUNT traversals return Value directly
        format!(
            "G::from_iter(&db, &txn, std::iter::once({}.clone()), &arena){}",
            source_var, steps
        )
    } else {
        // Regular traversal - collect to value
        format!(
            "G::from_iter(&db, &txn, std::iter::once({}.clone()), &arena){}.collect_to_value()",
            source_var, steps
        )
    }
}
