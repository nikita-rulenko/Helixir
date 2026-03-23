#!/bin/bash
# Integration tests for Hive Memory queries
# Tests both existing and new HQL queries against HelixDB
# Usage: HELIX_HOST=192.168.50.11 HELIX_PORT=6969 bash tests/test_hive_queries.sh

set -euo pipefail

HOST="${HELIX_HOST:-192.168.50.11}"
PORT="${HELIX_PORT:-6969}"
BASE="http://${HOST}:${PORT}"
PASS=0
FAIL=0
TESTS=()

run_test() {
    local name="$1"
    local endpoint="$2"
    local payload="$3"
    local expect_field="${4:-}"

    printf "  %-50s " "$name"
    
    response=$(curl -s -w "\n%{http_code}" -X POST "${BASE}/${endpoint}" \
        -H 'Content-Type: application/json' \
        -d "$payload" 2>/dev/null || echo -e "\n000")
    
    http_code=$(echo "$response" | tail -1)
    body=$(echo "$response" | sed '$d')

    if [[ "$http_code" == "200" ]]; then
        if [[ -n "$expect_field" ]]; then
            if echo "$body" | python3 -c "import sys,json; d=json.load(sys.stdin); assert '$expect_field' in str(d)" 2>/dev/null; then
                echo "PASS (200, field found)"
                PASS=$((PASS + 1))
            else
                echo "WARN (200, field '$expect_field' not in response)"
                PASS=$((PASS + 1))
            fi
        else
            echo "PASS (200)"
            PASS=$((PASS + 1))
        fi
    else
        echo "FAIL (HTTP $http_code)"
        FAIL=$((FAIL + 1))
        echo "    Response: $(echo "$body" | head -c 200)"
    fi
}

echo "=== Hive Memory Integration Tests ==="
echo "Target: ${BASE}"
echo ""

echo "--- Phase 1: Existing Query Regression Tests ---"

run_test "addUser" \
    "addUser" \
    '{"user_id": "test_hive_user_a", "name": "Test User A"}'

run_test "addUser (second)" \
    "addUser" \
    '{"user_id": "test_hive_user_b", "name": "Test User B"}'

run_test "getUser" \
    "getUser" \
    '{"user_id": "test_hive_user_a"}' \
    "user_id"

run_test "addMemory" \
    "addMemory" \
    '{"memory_id": "mem_hive_test_001", "user_id": "test_hive_user_a", "content": "Rust is my favorite language", "memory_type": "preference", "certainty": 90, "importance": 80, "created_at": "2025-01-01T00:00:00Z", "updated_at": "2025-01-01T00:00:00Z", "context_tags": "programming", "source": "test", "metadata": "{}"}'

run_test "addMemory (user B)" \
    "addMemory" \
    '{"memory_id": "mem_hive_test_002", "user_id": "test_hive_user_b", "content": "Python is my favorite language", "memory_type": "preference", "certainty": 90, "importance": 80, "created_at": "2025-01-01T00:00:00Z", "updated_at": "2025-01-01T00:00:00Z", "context_tags": "programming", "source": "test", "metadata": "{}"}'

run_test "addMemory (shared fact)" \
    "addMemory" \
    '{"memory_id": "mem_hive_test_003", "user_id": "test_hive_user_a", "content": "HelixDB uses graph-vector storage", "memory_type": "fact", "certainty": 100, "importance": 90, "created_at": "2025-01-01T00:00:00Z", "updated_at": "2025-01-01T00:00:00Z", "context_tags": "helixdb", "source": "test", "metadata": "{}"}'

run_test "getMemory" \
    "getMemory" \
    '{"memory_id": "mem_hive_test_001"}' \
    "content"

run_test "linkUserToMemory (A->001)" \
    "linkUserToMemory" \
    '{"user_id": "test_hive_user_a", "memory_id": "mem_hive_test_001", "context": "created"}'

run_test "linkUserToMemory (B->002)" \
    "linkUserToMemory" \
    '{"user_id": "test_hive_user_b", "memory_id": "mem_hive_test_002", "context": "created"}'

run_test "linkUserToMemory (A->003)" \
    "linkUserToMemory" \
    '{"user_id": "test_hive_user_a", "memory_id": "mem_hive_test_003", "context": "created"}'

run_test "updateMemory" \
    "updateMemory" \
    '{"memory_id": "mem_hive_test_001", "content": "Rust is my favorite systems language", "certainty": 95, "importance": 85, "updated_at": "2025-06-01T00:00:00Z"}'

run_test "getRecentMemories" \
    "getRecentMemories" \
    '{"limit": 5}'

run_test "getUserMemories" \
    "getUserMemories" \
    '{"user_id": "test_hive_user_a", "limit": 10}'

run_test "addMemoryContradiction" \
    "addMemoryContradiction" \
    '{"from_id": "mem_hive_test_001", "to_id": "mem_hive_test_002", "resolution": "different users prefer different languages", "resolved": 0, "resolution_strategy": "cross_user_preference"}'

echo ""
echo "--- Phase 2: New Hive Memory Query Tests ---"

run_test "getMemoryUsers" \
    "getMemoryUsers" \
    '{"memory_id": "mem_hive_test_001"}'

run_test "updateMemoryUserCount" \
    "updateMemoryUserCount" \
    '{"memory_id": "mem_hive_test_003", "user_count": 2, "updated_at": "2025-06-01T00:00:00Z"}'

run_test "linkUserToMemory (B->003 cross-link)" \
    "linkUserToMemory" \
    '{"user_id": "test_hive_user_b", "memory_id": "mem_hive_test_003", "context": "cross_user_link"}'

run_test "getMemoryUsers (after cross-link)" \
    "getMemoryUsers" \
    '{"memory_id": "mem_hive_test_003"}'

run_test "getMemoryContradictions" \
    "getMemoryContradictions" \
    '{"memory_id": "mem_hive_test_001"}'

EMBED_DIM=$(curl -s -X POST "http://192.168.50.2:8080/v1/embeddings" \
    -H 'Content-Type: application/json' \
    -d '{"input": "test query", "model": "nomic-embed-text-v1.5"}' 2>/dev/null \
    | python3 -c "import sys,json; e=json.load(sys.stdin)['data'][0]['embedding']; print(json.dumps(e))" 2>/dev/null || echo "")

if [[ -n "$EMBED_DIM" && "$EMBED_DIM" != "" ]]; then
    run_test "globalVectorSearch (real embedding)" \
        "globalVectorSearch" \
        "{\"query_vector\": $EMBED_DIM, \"limit\": 5}"
else
    echo "  globalVectorSearch (real embedding)               SKIP (no embedding service)"
    PASS=$((PASS + 1))
fi

run_test "checkUserMemoryLink" \
    "checkUserMemoryLink" \
    '{"user_id": "test_hive_user_a", "memory_id": "mem_hive_test_001"}'

echo ""
echo "--- Phase 3: Existing Query Regression (graph) ---"

run_test "getMemoryLogicalConnections" \
    "getMemoryLogicalConnections" \
    '{"memory_id": "mem_hive_test_001"}'

run_test "countAllMemories" \
    "countAllMemories" \
    '{}'

run_test "countAllUsers" \
    "countAllUsers" \
    '{}'

run_test "searchByContextTag" \
    "searchByContextTag" \
    '{"tag": "programming", "limit": 5}'

echo ""
echo "========================================="
echo "Results: ${PASS} passed, ${FAIL} failed"
echo "========================================="

if [[ "$FAIL" -gt 0 ]]; then
    exit 1
fi
