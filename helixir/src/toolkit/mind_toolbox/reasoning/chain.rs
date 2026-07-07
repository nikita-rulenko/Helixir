//! Reasoning-chain traversal: [`ReasoningEngine::get_chain`].
//!
//! Walks the 8 logical directions (IMPLIES in/out, BECAUSE in/out, CONTRADICTS
//! in/out, MEMORY_RELATION in/out) up to `max_depth`.
//!
//! Two regimes, chosen by the caller via `guidance`:
//! - `None` (legacy) — frontier pops LIFO (historic DFS behaviour) and the
//!   non-`deep` modes pick the next hop with one LLM call per step;
//! - `Some` (algo_opt R3) — true breadth-first order, and the next hop is the
//!   candidate whose content embedding is closest to the query embedding.
//!   Embeddings come from the (persistent) cache, so this path makes **zero**
//!   LLM calls and usually zero embedding HTTP calls.

use serde::Deserialize;
use tracing::{debug, warn};

use super::engine::ReasoningEngine;
use super::types::{ReasoningChain, ReasoningError, ReasoningType, project_relation};
use crate::llm::EmbeddingGenerator;
use crate::toolkit::mind_toolbox::search::smart_traversal::cosine_score;

/// Query context for embedding-guided traversal (algo_opt R3).
pub struct ChainGuidance<'a> {
    pub query_embedding: &'a [f32],
    pub embedder: &'a EmbeddingGenerator,
}

impl ReasoningEngine {
    pub async fn get_chain(
        &self,
        memory_id: &str,
        seed_content: &str,
        chain_type: &str,
        max_depth: usize,
        guidance: Option<ChainGuidance<'_>>,
    ) -> Result<ReasoningChain, ReasoningError> {
        #[derive(Deserialize)]
        struct ConnectionsResult {
            #[serde(default)]
            implies_out: Vec<MemoryNode>,
            #[serde(default)]
            implies_in: Vec<MemoryNode>,
            #[serde(default)]
            because_out: Vec<MemoryNode>,
            #[serde(default)]
            because_in: Vec<MemoryNode>,
            #[serde(default)]
            contradicts_out: Vec<MemoryNode>,
            #[serde(default)]
            contradicts_in: Vec<MemoryNode>,
            #[serde(default)]
            relation_out: Vec<MemoryNode>,
            #[serde(default)]
            relation_in: Vec<MemoryNode>,
        }

        #[derive(Deserialize, Clone)]
        struct MemoryNode {
            memory_id: String,
            #[serde(default)]
            content: String,
        }

        let is_deep = chain_type == "deep";
        let effective_max_depth = if is_deep { max_depth.max(8) } else { max_depth };

        let mut relations = Vec::new();
        let mut visited = std::collections::HashSet::new();
        // Frontier carries `(memory_id, content, depth)` so projection helpers
        // can pair `to_memory_id` with the matching `to_memory_content` regardless
        // of edge direction. See #17.
        let mut frontier: std::collections::VecDeque<(String, String, usize)> =
            std::collections::VecDeque::from([(
                memory_id.to_string(),
                seed_content.to_string(),
                0,
            )]);

        // Guided mode walks breadth-first; legacy keeps the historic LIFO pop.
        while let Some((current_id, current_content, current_depth)) = if guidance.is_some() {
            frontier.pop_front()
        } else {
            frontier.pop_back()
        } {
            if current_depth >= effective_max_depth || visited.contains(&current_id) {
                continue;
            }
            visited.insert(current_id.clone());

            let result = match self
                .client
                .execute_query::<ConnectionsResult, _>(
                    "getMemoryLogicalConnections",
                    &serde_json::json!({"memory_id": &current_id}),
                )
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    warn!(
                        "getMemoryLogicalConnections failed for {}: {} (depth={})",
                        crate::safe_truncate(&current_id, 16),
                        e,
                        current_depth
                    );
                    continue;
                }
            };

            let candidates: Vec<(MemoryNode, ReasoningType, bool)> = match chain_type {
                "causal" => {
                    let mut c = Vec::new();
                    for n in &result.because_in {
                        c.push((n.clone(), ReasoningType::Because, true));
                    }
                    for n in &result.because_out {
                        c.push((n.clone(), ReasoningType::Because, false));
                    }
                    for n in &result.implies_in {
                        c.push((n.clone(), ReasoningType::Implies, true));
                    }
                    c
                }
                "forward" => {
                    let mut c = Vec::new();
                    for n in &result.implies_out {
                        c.push((n.clone(), ReasoningType::Implies, false));
                    }
                    for n in &result.implies_in {
                        c.push((n.clone(), ReasoningType::Implies, true));
                    }
                    for n in &result.because_out {
                        c.push((n.clone(), ReasoningType::Because, false));
                    }
                    c
                }
                _ => {
                    let mut all = Vec::new();
                    for n in &result.implies_out {
                        all.push((n.clone(), ReasoningType::Implies, false));
                    }
                    for n in &result.implies_in {
                        all.push((n.clone(), ReasoningType::Implies, true));
                    }
                    for n in &result.because_out {
                        all.push((n.clone(), ReasoningType::Because, false));
                    }
                    for n in &result.because_in {
                        all.push((n.clone(), ReasoningType::Because, true));
                    }
                    for n in &result.contradicts_out {
                        all.push((n.clone(), ReasoningType::Contradicts, false));
                    }
                    for n in &result.contradicts_in {
                        all.push((n.clone(), ReasoningType::Contradicts, true));
                    }
                    for n in &result.relation_out {
                        all.push((n.clone(), ReasoningType::Supports, false));
                    }
                    for n in &result.relation_in {
                        all.push((n.clone(), ReasoningType::Supports, true));
                    }
                    all
                }
            };

            let mut unvisited: Vec<_> = candidates
                .into_iter()
                .filter(|(n, _, _)| !visited.contains(&n.memory_id))
                .collect();

            if unvisited.is_empty() {
                continue;
            }

            // A REASONING chain must prefer logical edges: in `both` mode the
            // candidate pool mixes typed hops (BECAUSE/IMPLIES/CONTRADICTS)
            // with generic MEMORY_RELATION ones (surfaced as SUPPORTS), and
            // cosine-only selection happily follows a semantically-close
            // SUPPORTS neighbor while a BECAUSE sits right there — the chain
            // then explains nothing. When any typed hop exists, select within
            // the typed subset; generic hops remain the fallback so sparse
            // graphs still walk.
            if !is_deep
                && unvisited
                    .iter()
                    .any(|(_, t, _)| *t != ReasoningType::Supports)
            {
                unvisited.retain(|(_, t, _)| *t != ReasoningType::Supports);
            }

            if is_deep {
                for (node, relation_type, is_incoming) in &unvisited {
                    relations.push(project_relation(
                        &current_id,
                        &current_content,
                        &node.memory_id,
                        &node.content,
                        *relation_type,
                        *is_incoming,
                        80,
                    ));

                    frontier.push_back((
                        node.memory_id.clone(),
                        node.content.clone(),
                        current_depth + 1,
                    ));
                }
            } else {
                let best = if unvisited.len() == 1 {
                    unvisited.into_iter().next()
                } else if let Some(g) = &guidance {
                    // R3: pick the hop whose content is semantically closest to
                    // the query. Cache-hot embeddings make this LLM- and
                    // (typically) HTTP-free.
                    let texts: Vec<&str> = unvisited
                        .iter()
                        .map(|(n, _, _)| n.content.as_str())
                        .collect();
                    match g.embedder.generate_batch(&texts, true).await {
                        Ok(embeddings) => {
                            let best_idx = embeddings
                                .iter()
                                .enumerate()
                                .map(|(i, e)| (i, cosine_score(g.query_embedding, e)))
                                .max_by(|a, b| {
                                    a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
                                })
                                .map(|(i, _)| i)
                                .unwrap_or(0);
                            unvisited.into_iter().nth(best_idx)
                        }
                        Err(e) => {
                            warn!("Embedding chain selection failed: {}", e);
                            unvisited.into_iter().next()
                        }
                    }
                } else if let Some(llm) = &self.llm_provider {
                    let prompt = format!(
                        "Given current memory and {} connected memories, which ONE is most logically relevant?\n\nCurrent: {}\n\nOptions:\n{}\n\nRespond with just the number (1-{}).",
                        unvisited.len(),
                        &current_id[..current_id.len().min(50)],
                        unvisited
                            .iter()
                            .enumerate()
                            .map(|(i, (n, t, _))| format!(
                                "{}. [{}] {}",
                                i + 1,
                                t.edge_name(),
                                n.content.chars().take(100).collect::<String>()
                            ))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        unvisited.len()
                    );

                    match llm
                        .generate(
                            "You are a reasoning assistant. Pick the most relevant connection.",
                            &prompt,
                            None,
                        )
                        .await
                    {
                        Ok((response, _)) => {
                            let choice: usize = response.trim().parse().unwrap_or(1);
                            unvisited.into_iter().nth(choice.saturating_sub(1))
                        }
                        Err(e) => {
                            warn!("LLM chain selection failed: {}", e);
                            unvisited.into_iter().next()
                        }
                    }
                } else {
                    unvisited.into_iter().next()
                };

                if let Some((node, relation_type, is_incoming)) = best {
                    relations.push(project_relation(
                        &current_id,
                        &current_content,
                        &node.memory_id,
                        &node.content,
                        relation_type,
                        is_incoming,
                        80,
                    ));

                    frontier.push_back((
                        node.memory_id.clone(),
                        node.content.clone(),
                        current_depth + 1,
                    ));
                }
            }
        }

        let max_depth_reached = relations.iter().count();
        let reasoning_trail = self.build_reasoning_trail(&relations);

        debug!(
            "Chain traversal for {}: type={}, relations={}, visited={}",
            crate::safe_truncate(memory_id, 12),
            chain_type,
            relations.len(),
            visited.len()
        );

        Ok(ReasoningChain {
            seed_memory_id: memory_id.to_string(),
            relations,
            chain_type: chain_type.to_string(),
            depth: max_depth_reached,
            reasoning_trail,
        })
    }
}
