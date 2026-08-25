




N::TestBM25Node {
    name: String,
    age: I32,
    description: String DEFAULT ""
}

// ============================================================================
// BM25 TEST QUERIES (from HelixDB test suite - exact copy)
// ============================================================================
QUERY test_bm25_with_k(k: I32) =>
    res <- SearchBM25<TestBM25Node>("John", k)
    RETURN res

// ============================================================================
// SIMPLE BM25 TEST QUERIES (from HelixDB test suite)
// ============================================================================

// Test query 1: Simple BM25 search on TestBM25Node
QUERY test_bm25_node(query_text: String, k: I64) =>
    results <- SearchBM25<TestBM25Node>(query_text, k)
    RETURN results

// Test query 2: Add test node with name and description
QUERY add_test_bm25_node(name: String, age: I32, description: String) =>
    node <- AddN<TestBM25Node>({
        name: name,
        age: age,
        description: description
    })
    RETURN node

// Test query 3: Get all test nodes
QUERY get_all_test_bm25_nodes() =>
    nodes <- N<TestBM25Node>
    RETURN nodes
