# Analyzer Module

## Overview
The analyzer module performs static analysis and type checking on the HelixQL AST, ensuring queries are grammatically and semantically correct before code generation.

## Structure

### Core Components
- **`mod.rs`** - Main analyzer entry point, orchestrates validation passes
- **`types.rs`** - Type system definitions and type inference structures
- **`diagnostic.rs`** - Diagnostic messages and error reporting
- **`error_codes.rs`** - Error code definitions and messages
- **`errors.rs`** - Error handling utilities
- **`fix.rs`** - Auto-fix suggestions for common errors
- **`pretty.rs`** - Pretty printing utilities for diagnostics
- **`utils.rs`** - Helper functions for analysis

### Validation Methods (in `methods/`)
- **`schema_methods.rs`** - Schema validation and field lookup building
- **`query_validation.rs`** - Query structure and parameter validation
- **`migration_validation.rs`** - Schema migration consistency checks
- **`statement_validation.rs`** - Statement-level validation
- **`traversal_validation.rs`** - Graph traversal operation validation
- **`graph_step_validation.rs`** - Individual graph step validation
- **`object_validation.rs`** - Object literal and remapping validation
- **`infer_expr_type.rs`** - Expression type inference
- **`exclude_validation.rs`** - Field exclusion validation

## Analysis Flow

1. **Input**: Parsed AST from the parser module
2. **Schema Validation**: Verifies schema definitions are valid
3. **Migration Validation**: Ensures migrations are consistent across versions
4. **Query Validation**: Type-checks queries against schemas
5. **Output**: Diagnostics (errors/warnings) and validated AST for code generation


## Error Handling
- Error codes provide consistent, searchable error identification
- Diagnostics include source location for precise error reporting
- Fix suggestions help users resolve common issues