// Test cases for GitHub issue #847
// WHERE clauses with traversal steps before property access

// Test 1: ReservedPropertyAccess with ToN (original issue case)
// Pattern: _::ToN::ID::EQ(id)
QUERY testToNId(id: ID) =>
    edges <- N<Person>::OutE<Knows>::WHERE(_::ToN::ID::EQ(id))
    RETURN edges

// Test 2: ReservedPropertyAccess with FromN
// Pattern: _::FromN::ID::EQ(id)
QUERY testFromNId(id: ID) =>
    edges <- N<Person>::OutE<Knows>::WHERE(_::FromN::ID::EQ(id))
    RETURN edges

// Test 3: PropertyFetch with ToN (original issue case)
// Pattern: _::ToN::{age}::EQ(age)
QUERY testToNProperty(age: I32) =>
    edges <- N<Person>::OutE<Knows>::WHERE(_::ToN::{age}::EQ(age))
    RETURN edges

// Test 4: PropertyFetch with FromN
// Pattern: _::FromN::{name}::EQ(name)
QUERY testFromNProperty(name: String) =>
    edges <- N<Person>::OutE<Knows>::WHERE(_::FromN::{name}::EQ(name))
    RETURN edges

// Test 5: Simple 2-step case (should still work - regression test)
// Pattern: _::ID::EQ(id)
QUERY testSimpleId(id: ID) =>
    nodes <- N<Person>::WHERE(_::ID::EQ(id))
    RETURN nodes

// Test 6: Simple 2-step property fetch (should still work - regression test)
// Pattern: _{age}::EQ(age)
QUERY testSimpleProperty(age: I32) =>
    nodes <- N<Person>::WHERE(_::{age}::EQ(age))
    RETURN nodes
