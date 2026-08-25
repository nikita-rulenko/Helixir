# Parser Module

## Overview
The parser module transforms HelixQL (HQL) source code into an Abstract Syntax Tree (AST) using the Pest parser generator framework.

## Structure

### Core Components
- **`mod.rs`** - Main parser entry point, orchestrates parsing of schemas, queries, and migrations
- **`grammar.pest`** - Pest grammar defining HQL syntax rules
- **`types.rs`** - AST node definitions and data structures
- **`location.rs`** - Location tracking for error reporting

### Parse Methods (by domain)
- **`schema_parse_methods.rs`** - Parses node, edge, and vector schema definitions
- **`query_parse_methods.rs`** - Parses query definitions with parameters and statements
- **`migration_parse_methods.rs`** - Parses schema migration definitions
- **`traversal_parse_methods.rs`** - Parses traversal (anonymous/id/starting node, vector or edge etc)
- **`graph_step_parse_methods.rs`** - Parses graph step operations (object remapping/order by/where/range etc)
- **`creation_step_parse_methods.rs`** - Parses node/edge/vector creation operations
- **`expression_parse_methods.rs`** - Parses expressions e.g. assignment, for loop, boolean expressions etc
- **`object_parse_methods.rs`** - Parses object fields for remappings/parameters/item creations etc
- **`return_value_parse_methods.rs`** - Parses return statements and remappings

## Parsing Flow

1. **Input**: HQL files containing schemas, queries, and migrations
2. **Lexing**: Pest tokenizes input according to `grammar.pest` rules
3. **AST Construction**: 
   - Schemas parsed first (establishing type definitions)
   - Migrations parsed second (for schema evolution)
   - Queries parsed last (can reference schema types)
4. **Output**: `Source` struct containing parsed schemas, migrations, and queries

## Key Types

- `Source` - Top-level container for all parsed content
- `Schema` - Contains node, edge, and vector type definitions
- `Query` - Parsed query with parameters, statements, and return values
- `Migration` - Schema version transition definitions

## Error Handling
- `ParserError` enum handles parse errors, lex errors, and schema validation
- Location tracking enables precise error reporting with file/line/column info
