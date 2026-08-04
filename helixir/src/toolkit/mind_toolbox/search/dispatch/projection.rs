//! Result-family projection and raw/atom collapsing.

use super::*;

impl SearchEngine {
    /// #82: collapse raw+atom families inside one result window. For every
    /// `raw_*` row present, fetch its incoming PART_OF edges (the atom→raw
    /// family link written by the add pipeline) and, when family members
    /// share the window, keep only the best-ranked one — annotated with the
    /// folded ids under `metadata.collapsed`. Zero cost when no raw row is
    /// in the window (the overwhelmingly common case).
    /// NOTE: deliberately NOT called inside [`SearchEngine::search`] — the
    /// write path's dedup recall (Phase A) needs the RAW candidates visible,
    /// or the duplicate gate loses the very atom it must compare against.
    /// The presentation layer (ToolingManager::search) calls this instead.
    pub async fn collapse_raw_families(&self, results: &mut Vec<UnifiedSearchResult>) {
        let raw_ids: Vec<String> = results
            .iter()
            .filter(|r| r.memory_id.starts_with("raw_"))
            .map(|r| r.memory_id.clone())
            .collect();
        if raw_ids.is_empty() {
            return;
        }

        let mut drop_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut annotations: Vec<(String, Vec<String>)> = Vec::new();

        for raw_id in raw_ids {
            // Two lookups joined on the internal node id: the EDGE projection
            // carries relation_type but only internal from_node UUIDs, while
            // the NODE projection carries memory_id + internal id. Both are
            // existing queries — no schema change.
            let edges: serde_json::Value = match self
                .client
                .execute_query("getMemoryIncomingRelations", &json!({"memory_id": &raw_id}))
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    debug!("family edge lookup failed for {}: {}", raw_id, e);
                    continue;
                }
            };
            let part_of_nodes: std::collections::HashSet<String> = edges["relations_in"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter(|e| e["relation_type"].as_str() == Some("PART_OF"))
                        .filter_map(|e| e["from_node"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            if part_of_nodes.is_empty() {
                continue;
            }
            let nodes: serde_json::Value = match self
                .client
                .execute_query(
                    "getMemoryLogicalConnections",
                    &json!({"memory_id": &raw_id}),
                )
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    debug!("family node lookup failed for {}: {}", raw_id, e);
                    continue;
                }
            };
            let family: std::collections::HashSet<String> = nodes["relation_in"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter(|n| {
                            n["id"]
                                .as_str()
                                .is_some_and(|id| part_of_nodes.contains(id))
                        })
                        .filter_map(|n| n["memory_id"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            if family.is_empty() {
                continue;
            }

            // Members of this family present in the window, best score first
            // (results are already rank-ordered).
            let present: Vec<String> = results
                .iter()
                .filter(|r| r.memory_id == raw_id || family.contains(&r.memory_id))
                .map(|r| r.memory_id.clone())
                .collect();
            if present.len() < 2 {
                continue;
            }
            // Content-lossless folding only. Sibling ATOMS are distinct
            // facts and must never fold into each other; the raw↔atom pair
            // is the only true redundancy (atom content is contained in the
            // raw). So: best member is an atom → fold ONLY the raw; best
            // member is the raw → fold the present atoms (their content is
            // inside the kept raw).
            let keeper = present[0].clone();
            let folded: Vec<String> = if keeper == raw_id {
                present.into_iter().skip(1).collect()
            } else {
                vec![raw_id.clone()]
            };
            drop_ids.extend(folded.iter().cloned());
            annotations.push((keeper, folded));
        }

        if drop_ids.is_empty() {
            return;
        }
        results.retain(|r| !drop_ids.contains(&r.memory_id));
        for (keeper, folded) in annotations {
            if let Some(row) = results.iter_mut().find(|r| r.memory_id == keeper) {
                row.metadata.insert("collapsed".to_string(), json!(folded));
            }
        }
    }

    /// #92: superseded rows lose ranking priority. A densely-linked stale
    /// hub carries PPR mass its own corrections cannot beat (observed live:
    /// stale fact at 0.926/ppr=1.0 above two explicit corrections) — so a
    /// row with an incoming SUPERSEDES edge gets its score multiplied by
    /// `retrieval.superseded_penalty` and an honest `superseded: true` +
    /// `superseded_by` in metadata. Reachability is untouched: the row still
    /// returns, ranked below its successor. Checked only for the top window
    /// (a stale hub is by definition ranked high); best-effort — a DB error
    /// leaves ranking as-is.
    pub(super) async fn demote_superseded(
        &self,
        results: &mut [UnifiedSearchResult],
        limit: usize,
    ) {
        let penalty = self.config.retrieval.superseded_penalty;
        if penalty >= 1.0 || results.is_empty() {
            return;
        }
        let window = superseded_window(limit, results.len());
        let ids: Vec<&str> = results[..window]
            .iter()
            .map(|r| r.memory_id.as_str())
            .collect();

        #[derive(serde::Deserialize)]
        struct Node {
            #[serde(default, deserialize_with = "crate::utils::nullable_string")]
            id: String,
            #[serde(default, deserialize_with = "crate::utils::nullable_string")]
            memory_id: String,
        }
        #[derive(serde::Deserialize)]
        struct Edge {
            #[serde(default, deserialize_with = "crate::utils::nullable_string")]
            from_node: String,
            #[serde(default, deserialize_with = "crate::utils::nullable_string")]
            to_node: String,
        }
        #[derive(serde::Deserialize, Default)]
        struct Resp {
            #[serde(default)]
            memories: Vec<Node>,
            #[serde(default)]
            superseded_edges: Vec<Edge>,
            #[serde(default)]
            successors: Vec<Node>,
        }

        let resp: Resp = match self
            .client
            .execute_query("getSupersededBatch", &json!({ "memory_ids": ids }))
            .await
        {
            Ok(r) => r,
            Err(e) => {
                debug!("superseded check skipped ({e})");
                return;
            }
        };
        if resp.superseded_edges.is_empty() {
            return;
        }

        let mut uuid_to_mid: std::collections::HashMap<&str, &str> =
            std::collections::HashMap::new();
        for n in resp.memories.iter().chain(resp.successors.iter()) {
            if !n.id.is_empty() && !n.memory_id.is_empty() {
                uuid_to_mid.insert(n.id.as_str(), n.memory_id.as_str());
            }
        }

        let mut demoted = 0usize;
        for edge in &resp.superseded_edges {
            let Some(stale_mid) = uuid_to_mid.get(edge.to_node.as_str()) else {
                continue;
            };
            let successor_mid = uuid_to_mid.get(edge.from_node.as_str()).copied();
            if let Some(row) = results[..window]
                .iter_mut()
                .find(|r| r.memory_id == *stale_mid)
            {
                // Idempotent under multiple SUPERSEDES edges: penalize once.
                if row.metadata.contains_key("superseded") {
                    continue;
                }
                row.score *= penalty as f32;
                row.metadata
                    .insert("superseded".to_string(), serde_json::Value::Bool(true));
                if let Some(succ) = successor_mid {
                    row.metadata.insert(
                        "superseded_by".to_string(),
                        serde_json::Value::String(succ.to_string()),
                    );
                }
                demoted += 1;
            }
        }
        if demoted > 0 {
            info!("Superseded demotion (#92): {demoted} stale row(s) penalized x{penalty}");
            results.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
    }

    pub async fn search_for_dedup(
        &self,
        query: &str,
        query_embedding: &[f32],
        user_id: &str,
        limit: usize,
    ) -> Result<Vec<UnifiedSearchResult>, SearchError> {
        let query_preview: String = query.chars().take(30).collect();
        info!(
            "SearchEngine.search_for_dedup: query='{}...', user={}, limit={}",
            query_preview, user_id, limit
        );

        if let Some(ref traversal) = self.smart_traversal {
            let config = self.make_search_config(
                limit,
                2,
                self.config.search_thresholds.min_vector_score,
                self.config.search_thresholds.min_combined_score,
                self.config.search_thresholds.temporal_weight,
            );
            let results = traversal
                .search(query, query_embedding, None, config, TimeWindow::default())
                .await
                .unwrap_or_default();

            Ok(results
                .into_iter()
                .take(limit)
                .map(|r| UnifiedSearchResult {
                    memory_id: r.memory_id,
                    content: r.content,
                    score: r.combined_score as f32,
                    method: "dedup_collective".to_string(),
                    metadata: r.metadata.unwrap_or_default(),
                    created_at: r.created_at.unwrap_or_default(),
                    user_count: None,
                    controversy: None,
                })
                .collect())
        } else {
            self.vector_search_unified(query, None, limit).await
        }
    }
}
