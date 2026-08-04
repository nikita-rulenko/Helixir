//! Graph expansion phase for smart traversal.

use super::*;

pub async fn graph_expansion_phase(
    client: Arc<HelixClient>,
    vector_hits: &[SearchResult],
    config: &SearchConfig,
) -> Result<Vec<SearchResult>, TraversalError> {
    info!(
        "Starting Phase 2: Graph expansion from {} vector hits",
        vector_hits.len()
    );

    let mut all_results = Vec::new();
    let mut expansion_tasks = Vec::new();

    let max_depth = config.graph_depth;
    let graph_weights = (
        config.graph_semantic_weight,
        config.graph_graph_weight,
        config.graph_temporal_weight,
        config.temporal_decay_days,
    );
    let ew = config.edge_weights;
    let ed = config.edge_damping;

    for hit in vector_hits {
        let ctx = ExpandCtx {
            client: Arc::clone(&client),
            max_depth,
            graph_weights,
            ew,
            ed,
        };
        let hit = hit.clone();

        let task = tokio::spawn(async move {
            let mut visited = HashSet::new();
            visited.insert(hit.memory_id.clone());

            expand_from_node(&ctx, &hit.memory_id, 1, &mut visited, hit.combined_score).await
        });

        expansion_tasks.push(task);
    }

    for task in expansion_tasks {
        match task.await {
            Ok(Ok(results)) => all_results.extend(results),
            Ok(Err(e)) => warn!("Graph expansion failed: {}", e),
            Err(e) => warn!("Graph expansion task panicked: {}", e),
        }
    }

    info!("Phase 2 completed: {} expanded results", all_results.len());
    Ok(all_results)
}

/// Read-only invariants of one graph expansion (#9): everything that stays
/// constant while `expand_from_node` recurses.
struct ExpandCtx {
    client: Arc<HelixClient>,
    max_depth: u32,
    /// (semantic_w, graph_w, temporal_w, decay_days)
    graph_weights: (f64, f64, f64, f64),
    ew: crate::core::config::EdgeWeights,
    ed: crate::core::config::EdgeDamping,
}

async fn expand_from_node(
    ctx: &ExpandCtx,
    node_id: &str,
    current_depth: u32,
    visited: &mut HashSet<String>,
    parent_score: f64,
) -> Result<Vec<SearchResult>, TraversalError> {
    let ExpandCtx { ew, ed, .. } = ctx;
    debug!("Expanding from node {} at depth {}", node_id, current_depth);

    let params = serde_json::json!({
        "memory_id": node_id
    });

    let response: GraphConnectionsResponse = ctx
        .client
        .execute_query("getMemoryLogicalConnections", &params)
        .await
        .map_err(|e| TraversalError::Database(e.to_string()))?;

    let mut results = Vec::new();
    let mut neighbors = Vec::new();

    process_edge_collection(
        ctx,
        &response.implies_out,
        ("IMPLIES", ew.implies),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        ctx,
        &response.because_out,
        ("BECAUSE", ew.because),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        ctx,
        &response.contradicts_out,
        ("CONTRADICTS", ew.contradicts),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        ctx,
        &response.relation_out,
        ("MEMORY_RELATION", ew.memory_relation),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        ctx,
        &response.implies_in,
        ("IMPLIES_IN", ew.implies * ed.implies_in),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        ctx,
        &response.because_in,
        ("BECAUSE_IN", ew.because * ed.because_in),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        ctx,
        &response.contradicts_in,
        ("CONTRADICTS_IN", ew.contradicts * ed.contradicts_in),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    process_edge_collection(
        ctx,
        &response.relation_in,
        ("MEMORY_RELATION_IN", ew.memory_relation * ed.relation_in),
        parent_score,
        visited,
        &mut results,
        &mut neighbors,
    );

    if current_depth < ctx.max_depth {
        neighbors.sort_by(|a, b| crate::toolkit::mind_toolbox::ranking::desc(&a.1, &b.1));
        for (neighbor_id, neighbor_score) in neighbors.into_iter().take(3) {
            if !visited.contains(&neighbor_id) {
                visited.insert(neighbor_id.clone());
                let expanded = Box::pin(expand_from_node(
                    ctx,
                    &neighbor_id,
                    current_depth + 1,
                    visited,
                    neighbor_score,
                ))
                .await?;
                results.extend(expanded);
            }
        }
    }

    Ok(results)
}

fn process_edge_collection(
    ctx: &ExpandCtx,
    memories: &[ConnectedMemory],
    edge: (&str, f64),
    parent_score: f64,
    visited: &HashSet<String>,
    results: &mut Vec<SearchResult>,
    neighbors: &mut Vec<(String, f64)>,
) {
    let (edge_type, edge_weight) = edge;
    let (semantic_w, graph_w, temporal_w, decay_days) = ctx.graph_weights;

    for mem in memories {
        if visited.contains(&mem.memory_id) {
            continue;
        }

        let temporal_score = calculate_temporal_freshness(
            super::super::scoring::event_time(&mem.valid_from, &mem.created_at),
            decay_days,
        );
        let graph_score = calculate_graph_score(edge_weight, parent_score);

        let semantic_sim = 0.5;

        let mut result = SearchResult::from_graph_weighted(
            &mem.memory_id,
            &mem.content,
            GraphScores {
                semantic_sim,
                graph_score,
                temporal_score,
            },
            1,
            vec![edge_type.to_string()],
            ScoreWeights {
                semantic: semantic_w,
                graph: graph_w,
                temporal: temporal_w,
            },
        );
        result.created_at = Some(mem.created_at.clone());
        result.valid_from = Some(mem.valid_from.clone());

        results.push(result);
        neighbors.push((mem.memory_id.clone(), graph_score));
    }
}
