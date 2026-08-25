//! Bounded candidate generation for Atropos paraphrase consolidation.
//!
//! This is deliberately not the public recall pipeline. Maintenance needs
//! nearest semantic neighbours, not BM25 fusion, graph expansion, PPR,
//! supersession projection, collective enrichment, or flashbacks.

use std::collections::{HashMap, HashSet};

use futures::{StreamExt, stream};
use serde::Deserialize;

use super::content_key::MemoryBrief;
use super::{ToolingError, ToolingManager};
use crate::toolkit::mind_toolbox::search::cosine_score;
use crate::utils::nullable_string;

const EMBEDDING_BATCH: usize = 64;
const DB_CONCURRENCY: usize = 2;
const DOMAIN_BATCH: usize = 256;

#[derive(Debug, Clone)]
pub(crate) struct ParaphrasePair {
    pub seed_id: String,
    pub seed_content: String,
    pub seed_content_key: String,
    pub candidate_id: String,
    pub candidate_content: String,
    pub candidate_content_key: String,
    pub security_domain: String,
    pub cosine: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct CandidateMemory {
    #[serde(default, deserialize_with = "nullable_string")]
    memory_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    content: String,
    #[serde(default, deserialize_with = "nullable_string")]
    content_key: String,
    #[serde(default, deserialize_with = "nullable_string")]
    rbac_scope: String,
    #[serde(default, deserialize_with = "nullable_string")]
    source: String,
}

#[derive(Debug, Default, Deserialize)]
struct CandidateResponse {
    #[serde(default)]
    memories: Vec<CandidateMemory>,
}

impl ToolingManager {
    /// Produce exact-cosine, same-security-domain candidate pairs without
    /// invoking the public search stack. Database concurrency is capped to the
    /// two HelixDB read workers used by the managed deployment.
    pub(crate) async fn paraphrase_pairs(
        &self,
        briefs: &[MemoryBrief],
        threshold: f64,
        neighbour_limit: usize,
    ) -> Result<Vec<ParaphrasePair>, ToolingError> {
        let eligible = briefs
            .iter()
            .filter(|brief| eligible_brief(brief))
            .cloned()
            .collect::<Vec<_>>();
        if eligible.is_empty() {
            return Ok(Vec::new());
        }

        let seed_texts = eligible
            .iter()
            .map(|brief| brief.content.as_str())
            .collect::<Vec<_>>();
        let seed_embeddings = self.embed_chunks(&seed_texts).await?;
        let fetch_limit = neighbour_limit.saturating_mul(3).saturating_add(1) as i64;

        let searches = stream::iter(eligible.iter().zip(seed_embeddings.iter()).map(
            |(brief, embedding)| {
                let db = self.db.clone();
                let brief = brief.clone();
                let query_vector = embedding
                    .iter()
                    .map(|value| f64::from(*value))
                    .collect::<Vec<_>>();
                async move {
                    let response = db
                        .execute_query::<CandidateResponse, _>(
                            "smartVectorSearchWithChunks",
                            &serde_json::json!({
                                "query_vector": query_vector,
                                "limit": fetch_limit,
                            }),
                        )
                        .await
                        .map_err(|error| ToolingError::Database(error.to_string()))?;
                    Ok::<_, ToolingError>((brief, response.memories))
                }
            },
        ))
        .buffer_unordered(DB_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

        // Do not consolidate from a partial scan: retrying an explicitly failed
        // pass is safer than silently producing order-dependent components.
        let mut searched = Vec::with_capacity(searches.len());
        for result in searches {
            searched.push(result?);
        }

        let missing_domain_ids = searched
            .iter()
            .flat_map(|(_, candidates)| candidates)
            .filter(|candidate| {
                !is_raw(&candidate.memory_id, &candidate.source)
                    && !valid_security_domain(&candidate.rbac_scope)
            })
            .map(|candidate| candidate.memory_id.clone())
            .filter(|id| !id.is_empty())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mut legacy_domains = HashMap::new();
        let rbac = crate::core::RbacManager::new(self.db.clone());
        for chunk in missing_domain_ids.chunks(DOMAIN_BATCH) {
            let domains = rbac
                .memory_security_domains(chunk)
                .await
                .map_err(|error| ToolingError::Database(error.to_string()))?;
            legacy_domains.extend(domains);
        }

        let mut candidate_texts = searched
            .iter()
            .flat_map(|(_, candidates)| candidates)
            .filter(|candidate| {
                !candidate.content.trim().is_empty()
                    && !is_raw(&candidate.memory_id, &candidate.source)
            })
            .map(|candidate| candidate.content.as_str())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        candidate_texts.sort_unstable();
        let candidate_embeddings = self.embed_chunks(&candidate_texts).await?;
        let embedding_by_content = candidate_texts
            .into_iter()
            .map(str::to_string)
            .zip(candidate_embeddings)
            .collect::<HashMap<_, _>>();
        let seed_embedding_by_id = eligible
            .iter()
            .map(|brief| brief.memory_id.clone())
            .zip(seed_embeddings)
            .collect::<HashMap<_, _>>();

        let mut best_by_ids: HashMap<(String, String), ParaphrasePair> = HashMap::new();
        for (seed, candidates) in searched {
            let Some(seed_domain) = seed
                .security_domain
                .as_deref()
                .filter(|domain| valid_security_domain(domain))
            else {
                continue;
            };
            let Some(seed_embedding) = seed_embedding_by_id.get(&seed.memory_id) else {
                continue;
            };
            let mut ranked = Vec::new();
            for candidate in candidates {
                if candidate.memory_id == seed.memory_id
                    || candidate.content_key.is_empty()
                    || candidate.content_key == seed.content_key
                    || candidate.content.trim().is_empty()
                    || is_raw(&candidate.memory_id, &candidate.source)
                {
                    continue;
                }
                let domain = if valid_security_domain(&candidate.rbac_scope) {
                    Some(candidate.rbac_scope.as_str())
                } else {
                    legacy_domains
                        .get(&candidate.memory_id)
                        .map(String::as_str)
                        .filter(|domain| valid_security_domain(domain))
                };
                if domain != Some(seed_domain) {
                    continue;
                }
                let Some(candidate_embedding) = embedding_by_content.get(&candidate.content) else {
                    continue;
                };
                let cosine = cosine_score(seed_embedding, candidate_embedding);
                if cosine < threshold {
                    continue;
                }
                ranked.push(ParaphrasePair {
                    seed_id: seed.memory_id.clone(),
                    seed_content: seed.content.clone(),
                    seed_content_key: seed.content_key.clone(),
                    candidate_id: candidate.memory_id,
                    candidate_content: candidate.content,
                    candidate_content_key: candidate.content_key,
                    security_domain: seed_domain.to_string(),
                    cosine,
                });
            }
            ranked.sort_by(|left, right| {
                right
                    .cosine
                    .total_cmp(&left.cosine)
                    .then_with(|| left.candidate_id.cmp(&right.candidate_id))
            });
            ranked.truncate(neighbour_limit);
            for pair in ranked {
                let ids = ordered_ids(&pair.seed_id, &pair.candidate_id);
                match best_by_ids.get(&ids) {
                    Some(existing) if existing.cosine >= pair.cosine => {}
                    _ => {
                        best_by_ids.insert(ids, pair);
                    }
                }
            }
        }

        let mut pairs = best_by_ids.into_values().collect::<Vec<_>>();
        pairs.sort_by(|left, right| {
            left.security_domain
                .cmp(&right.security_domain)
                .then_with(|| left.seed_id.cmp(&right.seed_id))
                .then_with(|| left.candidate_id.cmp(&right.candidate_id))
        });
        Ok(pairs)
    }

    async fn embed_chunks(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, ToolingError> {
        let mut embeddings = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(EMBEDDING_BATCH) {
            embeddings.extend(
                self.embedder
                    .generate_batch(chunk, true)
                    .await
                    .map_err(|error| ToolingError::Embedding(error.to_string()))?,
            );
        }
        Ok(embeddings)
    }
}

fn eligible_brief(brief: &MemoryBrief) -> bool {
    !brief.content.trim().is_empty()
        && !brief.content_key.is_empty()
        && !is_raw(&brief.memory_id, &brief.source)
        && brief
            .security_domain
            .as_deref()
            .is_some_and(valid_security_domain)
}

fn is_raw(memory_id: &str, source: &str) -> bool {
    memory_id.starts_with("raw_") || source == "raw_input"
}

fn valid_security_domain(domain: &str) -> bool {
    ["rbac:group:", "rbac:dedup:", "group:", "dedup:"]
        .iter()
        .any(|prefix| domain.strip_prefix(prefix).is_some_and(|id| !id.is_empty()))
}

fn ordered_ids(left: &str, right: &str) -> (String, String) {
    if left <= right {
        (left.to_string(), right.to_string())
    } else {
        (right.to_string(), left.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_domains_fail_closed() {
        for valid in [
            "rbac:group:alpha",
            "rbac:dedup:development",
            "group:legacy",
            "dedup:legacy",
        ] {
            assert!(valid_security_domain(valid));
        }
        for invalid in ["", "unknown", "unscoped", "rbac:unscoped", "invalid:x"] {
            assert!(!valid_security_domain(invalid));
        }
    }

    #[test]
    fn raw_nodes_are_excluded_by_id_or_source() {
        assert!(is_raw("raw_1", "agent"));
        assert!(is_raw("mem_1", "raw_input"));
        assert!(!is_raw("mem_1", "agent"));
    }
}
