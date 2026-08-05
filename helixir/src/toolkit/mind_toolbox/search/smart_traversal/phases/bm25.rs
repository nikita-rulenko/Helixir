//! BM25 retrieval helpers for smart traversal.

use super::*;

pub(super) async fn fetch_bm25_memories(
    client: &HelixClient,
    query_text: &str,
    limit: i64,
) -> Result<Vec<VectorMemory>, TraversalError> {
    #[derive(Debug, Deserialize)]
    struct Bm25Response {
        #[serde(default)]
        memories: Vec<VectorMemory>,
    }

    let params = serde_json::json!({
        "text": query_text,
        "limit": limit,
    });

    let resp: Bm25Response = client
        .execute_query("searchMemoriesByBm25", &params)
        .await
        .map_err(|e| TraversalError::Database(e.to_string()))?;
    Ok(resp.memories)
}
