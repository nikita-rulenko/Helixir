# Generator Module

## Overview
The generator module transforms the validated HelixQL AST into executable Rust code, creating type-safe graph database operations.

## Structure

### Core Components
- **`mod.rs`** - Main generator entry point, defines output structure
- **`utils.rs`** - Helper functions and code generation utilities

### Code Generation Methods (by domain)
- **`schemas.rs`** - Generates Rust structs for nodes, edges, and vectors
- **`queries.rs`** - Generates query functions with proper signatures
- **`migrations.rs`** - Generates migration code for schema evolution
- **`statements.rs`** - Generates statement execution code
- **`traversal_steps.rs`** - Generates graph traversal operations
- **`source_steps.rs`** - Generates source operations (add_n, add_e, n_from_id, n_from_type, etc.)
- **`bool_ops.rs`** - Generates boolean expression evaluators
- **`object_remappings.rs`** - Generates object transformation code
- **`return_values.rs`** - Generates return value processing
- **`tsdisplay.rs`** - TypeScript display utilities

## Generation Flow

1. **Input**: Validated AST from the analyzer module
2. **Schema Generation**: Creates Rust structs for all schema types
3. **Query Generation**: Transforms queries into Rust functions
4. **Migration Generation**: Creates migration execution code
5. **Output**: Complete Rust source code ready for compilation

## Code Generation Patterns
- Uses Rust's `Display` trait for code generation
- Maintains proper indentation and formatting
- Generates idiomatic Rust code with appropriate error handling
