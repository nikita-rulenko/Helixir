//! Retroactive causal stitching (#83, increment 2) — Lachesis's second duty.
//!
//! The write path builds BECAUSE edges within one add_memory call (extractor)
//! and from a new atom to recalled candidates (decision.relates_to). What
//! neither can do is connect two OLD memories whose causal relation only
//! becomes visible after both exist. This pass walks a bounded window of a
//! user's memories, proposes candidate pairs by ENTITY OVERLAP, asks the LLM
//! whether an explicit causal relation holds, and persists the survivors as
//! BECAUSE edges tagged `lachesis-stitch` (hypothesis-grade provenance — the
//! apophenia guardrail: generated connections are hypotheses, never asserted
//! truth).
//!
//! Discipline inherited from the OOM incident: everything is capped —
//! window, judged pairs per pass, persisted edges per pass — and pairs that
//! already share ANY logical edge are skipped, so re-running converges
//! instead of flooding.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;
use tracing::{debug, info, warn};

use crate::toolkit::tooling_manager::ToolingManager;
use crate::toolkit::tooling_manager::types::ToolingError;

/// What one stitch pass did — journaled by the daemon like other stages.
#[derive(Debug, Default, Clone)]
pub struct StitchStats {
    pub window: usize,
    pub pairs_considered: usize,
    pub judged: usize,
    pub persisted: usize,
    pub skipped_linked: usize,
}

/// The judge's verdict for one pair, parsed leniently (weak-model lesson #78).
#[derive(Deserialize)]
struct Verdict {
    #[serde(default)]
    relation: String,
    /// Which side is the CAUSE: "a" or "b".
    #[serde(default)]
    cause: String,
    #[serde(default)]
    confidence: i64,
}

const JUDGE_SYS: &str = r#"You judge whether two memories from one user's store stand in an EXPLICIT causal relation. Be conservative: shared topic or co-occurrence is NOT causation — answer NONE unless one memory plainly states or strongly entails the cause of the other.

Respond with STRICT JSON only:
{"relation": "BECAUSE" | "NONE", "cause": "a" | "b", "confidence": 0-100, "reason": "one short sentence"}

"cause" names the memory that is the CAUSE (only meaningful when relation is BECAUSE)."#;

pub struct Stitcher<'a> {
    tooling: &'a ToolingManager,
}

impl<'a> Stitcher<'a> {
    pub fn new(tooling: &'a ToolingManager) -> Self {
        Self { tooling }
    }

    /// One bounded stitching pass over `user`'s recent memories.
    pub async fn stitch_pass(&self, user: &str) -> Result<StitchStats, ToolingError> {
        let cfg = &self.tooling.config.moira.lachesis;
        let mut stats = StitchStats::default();

        // 1) Window: recent memories, atoms only (raw sources are episode
        // containers, not facts — their causality belongs to their atoms).
        let mems: Vec<(String, String)> = self
            .tooling
            .list_user_memories(user, cfg.stitch_window as i64)
            .await?
            .into_iter()
            .filter(|(id, _)| !id.starts_with("raw_"))
            .collect();
        stats.window = mems.len();
        if mems.len() < 2 {
            return Ok(stats);
        }

        // 2) Per memory: entity names (candidate signal) + already-linked
        // neighbors (skip signal). One graph-stats + one connections query per
        // memory, bounded by the window cap.
        let mut entities: HashMap<String, HashSet<String>> = HashMap::new();
        let mut linked: HashMap<String, HashSet<String>> = HashMap::new();
        for (id, _) in &mems {
            entities.insert(id.clone(), self.memory_entities(id).await);
            linked.insert(id.clone(), self.memory_neighbors(id).await);
        }

        // 3) Candidate pairs: >=1 shared entity, no existing logical edge in
        // either direction; ranked by overlap size so the strongest signals
        // are judged first under the cap.
        let mut pairs: Vec<(usize, usize, usize)> = Vec::new();
        for i in 0..mems.len() {
            for j in (i + 1)..mems.len() {
                let (a, b) = (&mems[i].0, &mems[j].0);
                let overlap = entities[a].intersection(&entities[b]).count();
                if overlap == 0 {
                    continue;
                }
                if linked[a].contains(b) || linked[b].contains(a) {
                    stats.skipped_linked += 1;
                    continue;
                }
                pairs.push((i, j, overlap));
            }
        }
        stats.pairs_considered = pairs.len();
        pairs.sort_by(|x, y| y.2.cmp(&x.2));
        pairs.truncate(cfg.stitch_max_judged);

        // 4) Judge each surviving pair; persist under the per-pass cap.
        for (i, j, overlap) in pairs {
            if stats.persisted >= cfg.stitch_max_persist {
                debug!("stitch: persist cap reached ({})", cfg.stitch_max_persist);
                break;
            }
            let (a_id, a_text) = &mems[i];
            let (b_id, b_text) = &mems[j];
            let user_prompt = format!(
                "Memory A ({a_id}): {a_text}\n\nMemory B ({b_id}): {b_text}\n\nShared entities: {overlap}."
            );
            let verdict = match self
                .tooling
                .llm_provider
                .generate(JUDGE_SYS, &user_prompt, Some("json_object"))
                .await
            {
                Ok((content, _)) => match serde_json::from_str::<Verdict>(&content) {
                    Ok(v) => v,
                    Err(e) => {
                        debug!("stitch: unparseable verdict for {a_id}/{b_id}: {e}");
                        continue;
                    }
                },
                Err(e) => {
                    warn!("stitch: judge call failed: {e}");
                    continue;
                }
            };
            stats.judged += 1;
            debug!(
                "stitch verdict {a_id}/{b_id}: relation={} cause={} confidence={}",
                verdict.relation, verdict.cause, verdict.confidence
            );

            if verdict.relation != "BECAUSE"
                || verdict.confidence < cfg.stitch_min_confidence as i64
            {
                continue;
            }
            // Direction follows the decision-path semantics: the EFFECT
            // carries the outgoing BECAUSE edge to its CAUSE ("A because B").
            let (effect, cause) = match verdict.cause.as_str() {
                "a" => (b_id, a_id),
                "b" => (a_id, b_id),
                _ => continue,
            };
            match self
                .tooling
                .reasoning_engine
                .add_relation(
                    effect,
                    cause,
                    crate::toolkit::mind_toolbox::reasoning::ReasoningType::Because,
                    (verdict.confidence.clamp(0, 90)) as i32,
                    Some("lachesis-stitch"),
                )
                .await
            {
                Ok(_) => {
                    stats.persisted += 1;
                    info!(
                        "stitch: BECAUSE {} -> {} (confidence {})",
                        effect, cause, verdict.confidence
                    );
                }
                Err(e) => debug!("stitch: persist {effect}->{cause} skipped: {e}"),
            }
        }

        info!(
            "lachesis.stitch_pass(user={user}): window={} pairs={} judged={} persisted={} skipped_linked={}",
            stats.window,
            stats.pairs_considered,
            stats.judged,
            stats.persisted,
            stats.skipped_linked
        );
        Ok(stats)
    }

    /// Entity NAMES attached to a memory (via the graph-stats projection).
    async fn memory_entities(&self, memory_id: &str) -> HashSet<String> {
        let resp: Result<serde_json::Value, _> = self
            .tooling
            .db
            .execute_query(
                "getMemoryGraphStats",
                &serde_json::json!({"memory_id": memory_id}),
            )
            .await;
        let mut out = HashSet::new();
        if let Ok(v) = resp {
            for bucket in ["entities", "mentions"] {
                if let Some(arr) = v[bucket].as_array() {
                    for n in arr {
                        if let Some(name) = n["name"].as_str() {
                            out.insert(name.to_lowercase());
                        }
                    }
                }
            }
        }
        out
    }

    /// memory_ids already connected to this one by ANY logical/generic edge,
    /// either direction — those pairs are settled, never re-judged.
    async fn memory_neighbors(&self, memory_id: &str) -> HashSet<String> {
        let resp: Result<serde_json::Value, _> = self
            .tooling
            .db
            .execute_query(
                "getMemoryLogicalConnections",
                &serde_json::json!({"memory_id": memory_id}),
            )
            .await;
        let mut out = HashSet::new();
        if let Ok(v) = resp {
            for bucket in [
                "implies_out",
                "implies_in",
                "because_out",
                "because_in",
                "contradicts_out",
                "contradicts_in",
                "relation_out",
                "relation_in",
            ] {
                if let Some(arr) = v[bucket].as_array() {
                    for n in arr {
                        if let Some(id) = n["memory_id"].as_str() {
                            out.insert(id.to_string());
                        }
                    }
                }
            }
        }
        out
    }
}
