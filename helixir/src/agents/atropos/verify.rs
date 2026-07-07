//! #91 part 2: the verification duty — insight debt stops accruing.
//!
//! Every Lachesis hypothesis ships labelled `HYPOTHESIS (generated, requires
//! verification)`; before this duty nothing ever verified or retired them, so
//! the store accumulated unfalsified leads forever. Now aging hypotheses get
//! an adversarial review against their own witness memories:
//!
//! - **promote** — the witnesses genuinely support the link: the text is
//!   relabelled `VERIFIED (generated, confirmed by review)` in place (the
//!   correction path, embedding regenerated).
//! - **retire** — the link is spurious/apophenic: a retirement note is stored
//!   and the hypothesis is SUPERSEDED by it — which, since #92, automatically
//!   demotes the retired hypothesis in every search. History stays reachable.
//! - **keep** — uncertain: it ages further and is reviewed again later.
//!
//! Conservative by construction: the judge must cite the witnesses, caps
//! bound the work per pass, and a dead LLM simply keeps everything.

use serde::Deserialize;
use tracing::{info, warn};

use crate::toolkit::tooling_manager::ToolingManager;

/// What one verification pass did.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct VerifyStats {
    pub reviewed: usize,
    pub promoted: usize,
    pub retired: usize,
    pub kept: usize,
}

const JUDGE_SYS: &str = "You are an adversarial reviewer of GENERATED hypotheses \
in a memory system. You are given one hypothesis and the witness memories that \
were used to generate it. Decide STRICTLY from the witnesses: \
'promote' ONLY when the witnesses genuinely establish the claimed connection; \
'retire' when the connection is coincidental, rests on a word used in two \
unrelated senses (e.g. financial vs software 'benchmarking'), or the witnesses \
do not support it; 'keep' when you are unsure. Be conservative: when in doubt, \
'keep'. Respond with JSON only: \
{\"verdict\":\"promote|retire|keep\",\"reason\":\"one sentence citing the witnesses\"}";

#[derive(Deserialize)]
struct Verdict {
    verdict: String,
    #[serde(default)]
    reason: String,
}

#[derive(Deserialize)]
struct MemRow {
    #[serde(default, deserialize_with = "crate::utils::nullable_string")]
    memory_id: String,
    #[serde(default, deserialize_with = "crate::utils::nullable_string")]
    content: String,
    #[serde(default, deserialize_with = "crate::utils::nullable_string")]
    created_at: String,
    #[serde(default, deserialize_with = "crate::utils::nullable_string")]
    user_id: String,
}

#[derive(Deserialize)]
struct MemRows {
    #[serde(default)]
    memories: Vec<MemRow>,
}

/// The verifier. Borrows the toolkit like every agent.
pub struct Verifier<'a> {
    tooling: &'a ToolingManager,
}

impl<'a> Verifier<'a> {
    pub fn new(tooling: &'a ToolingManager) -> Self {
        Self { tooling }
    }

    /// One bounded verification pass over aging hypotheses.
    pub async fn verify_pass(&self) -> VerifyStats {
        let cfg = &self.tooling.config.moira.atropos;
        let mut stats = VerifyStats::default();
        if cfg.verify_min_age_hours <= 0.0 || cfg.verify_max_per_pass == 0 {
            return stats;
        }
        let llm = &self.tooling.llm_provider;

        // Aging, still-unverified hypotheses (BM25 by the label prefix; the
        // prefix filter below is authoritative).
        let rows: MemRows = match self
            .tooling
            .db
            .execute_query(
                "searchMemoriesByBm25",
                &serde_json::json!({"text": "HYPOTHESIS generated requires verification", "limit": 100}),
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!("verify pass: hypothesis scan failed ({e})");
                return stats;
            }
        };
        let now = chrono::Utc::now();
        let min_age = chrono::Duration::seconds((cfg.verify_min_age_hours * 3600.0) as i64);
        let mut aging: Vec<MemRow> = rows
            .memories
            .into_iter()
            .filter(|m| {
                m.user_id == "helixir"
                    && m.content.starts_with("HYPOTHESIS (generated")
                    && chrono::DateTime::parse_from_rfc3339(&m.created_at)
                        .map(|t| now - t.with_timezone(&chrono::Utc) >= min_age)
                        .unwrap_or(false)
            })
            .collect();
        aging.sort_by(|a, b| a.created_at.cmp(&b.created_at)); // oldest first
        aging.truncate(cfg.verify_max_per_pass);

        for hyp in aging {
            let witnesses = self.witness_contents(&hyp.memory_id).await;
            if witnesses.is_empty() {
                stats.reviewed += 1;
                // No provenance to judge against: never guess a verdict. But a
                // witness-less hypothesis can never be verified either — past
                // the aged-out bar it retires as unverifiable (#91's "the
                // journal must not grow unverified leads forever").
                let unverifiable_age = cfg.verify_unverifiable_age_hours;
                let old_enough = unverifiable_age > 0.0
                    && chrono::DateTime::parse_from_rfc3339(&hyp.created_at)
                        .map(|t| {
                            now - t.with_timezone(&chrono::Utc)
                                >= chrono::Duration::seconds((unverifiable_age * 3600.0) as i64)
                        })
                        .unwrap_or(false);
                if old_enough {
                    if self
                        .retire(&hyp, "unverifiable: no witness provenance", &now)
                        .await
                    {
                        stats.retired += 1;
                        info!("verify: RETIRED {} (unverifiable)", hyp.memory_id);
                    }
                } else {
                    stats.kept += 1;
                }
                continue;
            }
            let prompt = format!(
                "HYPOTHESIS:\n{}\n\nWITNESS MEMORIES:\n{}\n\nVerdict?",
                hyp.content,
                witnesses
                    .iter()
                    .enumerate()
                    .map(|(i, w)| format!("{}. {}", i + 1, w))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            let verdict: Option<Verdict> =
                match llm.generate(JUDGE_SYS, &prompt, Some("json_object")).await {
                    Ok((resp, _)) => serde_json::from_str(&resp)
                        .map_err(|e| warn!("verify judge parse failed: {e}"))
                        .ok(),
                    Err(e) => {
                        warn!("verify judge call failed: {e}");
                        None
                    }
                };
            stats.reviewed += 1;
            match verdict.as_ref().map(|v| v.verdict.as_str()) {
                Some("promote") => {
                    let reason = verdict
                        .as_ref()
                        .map(|v| v.reason.clone())
                        .unwrap_or_default();
                    let new_content = hyp.content.replacen(
                        "HYPOTHESIS (generated, requires verification)",
                        "VERIFIED (generated, confirmed by review)",
                        1,
                    ) + &format!(
                        " [verified {}: {}]",
                        now.format("%Y-%m-%d"),
                        crate::safe_truncate(&reason, 140)
                    );
                    match self
                        .tooling
                        .update_memory(&hyp.memory_id, &new_content, "helixir")
                        .await
                    {
                        Ok(_) => {
                            stats.promoted += 1;
                            info!("verify: PROMOTED {}", hyp.memory_id);
                        }
                        Err(e) => warn!("verify: promote update failed ({e})"),
                    }
                }
                Some("retire") => {
                    let reason = verdict
                        .as_ref()
                        .map(|v| v.reason.clone())
                        .unwrap_or_default();
                    if self.retire(&hyp, &reason, &now).await {
                        stats.retired += 1;
                        info!("verify: RETIRED {}", hyp.memory_id);
                    }
                }
                _ => {
                    stats.kept += 1;
                }
            }
        }
        if stats.reviewed > 0 {
            info!(
                "verify pass: {} reviewed — {} promoted, {} retired, {} kept",
                stats.reviewed, stats.promoted, stats.retired, stats.kept
            );
        }
        stats
    }

    /// The witnesses: memories with a SUPPORTS edge INTO the hypothesis (the
    /// provenance persist_insights wrote).
    async fn witness_contents(&self, memory_id: &str) -> Vec<String> {
        #[derive(Deserialize, Default)]
        struct Incoming {
            #[serde(default)]
            supports_in: Vec<MemRow>,
        }
        match self
            .tooling
            .db
            .execute_query::<Incoming, _>(
                "getMemoryIncomingRelations",
                &serde_json::json!({"memory_id": memory_id}),
            )
            .await
        {
            Ok(r) => r
                .supports_in
                .into_iter()
                .filter(|m| !m.content.is_empty())
                .take(6)
                .map(|m| m.content.chars().take(240).collect())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Retirement: a note memory + SUPERSEDES(note → hypothesis). Since #92
    /// the superseded hypothesis auto-demotes in every search — the aging
    /// policy costs nothing beyond this edge.
    async fn retire(
        &self,
        hyp: &MemRow,
        reason: &str,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> bool {
        let summary: String = hyp
            .content
            .chars()
            .skip("HYPOTHESIS (generated, requires verification): ".len())
            .take(140)
            .collect();
        let text = format!(
            "RETIRED hypothesis (failed verification {}): {} — {}",
            now.format("%Y-%m-%d"),
            summary,
            crate::safe_truncate(reason, 140)
        );
        let note_id = {
            let vector = match self.tooling.embedder.generate(&text, true).await {
                Ok(v) => v,
                Err(e) => {
                    warn!("verify: retirement embed failed ({e})");
                    return false;
                }
            };
            let memory = crate::llm::extractor::ExtractedMemory {
                text,
                memory_type: "fact".to_string(),
                certainty: 85,
                importance: 40,
                entities: vec![],
                context: None,
            };
            match self
                .tooling
                .store_new_memory(&memory, "helixir", &vector, "insight-retired")
                .await
            {
                Ok((id, _)) => id,
                Err(e) => {
                    warn!("verify: retirement note store failed ({e})");
                    return false;
                }
            }
        };
        match self
            .tooling
            .record_supersession(
                &note_id,
                &hyp.memory_id,
                "hypothesis retired by verification",
            )
            .await
        {
            Ok(()) => true,
            Err(e) => {
                warn!("verify: supersession failed ({e})");
                false
            }
        }
    }
}
