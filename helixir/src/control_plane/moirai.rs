//! Admin-only Moirai journal with witness and workspace provenance.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use serde::Deserialize;

use crate::db::HelixClient;

use super::dto::{MoiraiInsightProjection, MoiraiProjection, MoiraiStageProjection};
use super::graph::load_memory_groups;
use super::stats::{MemoryRow, RecentResponse, admin_policy, load_agents};

const INSIGHT_LIMIT: i64 = 80;

pub(super) async fn load_moirai(db: &Arc<HelixClient>, actor: &str) -> Option<MoiraiProjection> {
    let policy = admin_policy(db, actor).await.ok()?;
    let config = crate::core::HelixirConfig::from_env();
    let known_principals = policy.users.keys().cloned().collect::<BTreeSet<_>>();
    let agents = load_agents(db, config.swarm.active_window_secs, &known_principals).await;
    let daemon = agents.iter().find(|agent| agent.role == "daemon");
    let categories: CategoryResponse = db
        .execute_query_no_retry("getAllCategories", &serde_json::json!({"limit": 1_000}))
        .await
        .unwrap_or_default();
    let mut response: RecentResponse = db
        .execute_query_no_retry(
            "searchByContextTag",
            &serde_json::json!({"tag": "moira-insight", "limit": INSIGHT_LIMIT}),
        )
        .await
        .unwrap_or_default();
    response
        .memories
        .sort_by(|left, right| right.created_at.cmp(&left.created_at));
    let last_insight_at = response
        .memories
        .iter()
        .find_map(|row| (!row.created_at.is_empty()).then(|| row.created_at.clone()));
    let last_category_at = categories
        .categories
        .iter()
        .filter_map(|row| (!row.created_at.is_empty()).then_some(row.created_at.as_str()))
        .max()
        .map(str::to_string);
    let mut witnesses = HashMap::<String, Vec<MemoryRow>>::new();
    for insight in &response.memories {
        let rows: WitnessResponse = db
            .execute_query_no_retry(
                "getMoiraiWitnesses",
                &serde_json::json!({"insight_id": insight.memory_id}),
            )
            .await
            .unwrap_or_default();
        witnesses.insert(insight.memory_id.clone(), rows.witnesses);
    }
    let all_ids = response
        .memories
        .iter()
        .map(|row| row.memory_id.clone())
        .chain(
            witnesses
                .values()
                .flat_map(|rows| rows.iter().map(|row| row.memory_id.clone())),
        )
        .collect::<Vec<_>>();
    let group_map = load_memory_groups(db, all_ids).await;
    let insights = response
        .memories
        .into_iter()
        .map(|row| {
            let witness_rows = witnesses.remove(&row.memory_id).unwrap_or_default();
            let source_groups = witness_rows
                .iter()
                .flat_map(|witness| {
                    group_map
                        .get(&witness.memory_id)
                        .into_iter()
                        .flatten()
                        .filter(|group| group.as_str() != crate::core::MOIRAI_GROUP_ID)
                        .cloned()
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let witness_count = witness_rows.len();
            let witnesses = witness_rows
                .into_iter()
                .map(|witness| {
                    let assigned = group_map
                        .get(&witness.memory_id)
                        .cloned()
                        .unwrap_or_default();
                    witness.into_projection(assigned)
                })
                .collect();
            let assigned = group_map.get(&row.memory_id).cloned().unwrap_or_default();
            MoiraiInsightProjection {
                memory: row.into_projection(assigned),
                source_groups,
                witness_count,
                witnesses,
                orphaned: witness_count == 0,
            }
        })
        .collect::<Vec<_>>();
    let witness_count = insights.iter().map(|insight| insight.witness_count).sum();
    let orphan_count = insights.iter().filter(|insight| insight.orphaned).count();
    let insight_count = insights.len();
    let daemon_active = daemon.is_some_and(|agent| agent.active);
    Some(MoiraiProjection {
        enabled: config.mode.insights_enabled(),
        mode: config.mode.label(),
        daemon_active,
        daemon_status: daemon.map(|agent| agent.status.clone()),
        insights,
        stages: vec![
            MoiraiStageProjection {
                name: "Clotho",
                responsibility: "category dictionary and memory tagging",
                state: stage_state(daemon_active, categories.categories.len()),
                artifact_count: categories.categories.len(),
                last_activity_at: last_category_at,
            },
            MoiraiStageProjection {
                name: "Lachesis",
                responsibility: "multi-hop routes and witness provenance",
                state: stage_state(daemon_active, witness_count),
                artifact_count: witness_count,
                last_activity_at: last_insight_at.clone(),
            },
            MoiraiStageProjection {
                name: "Atropos",
                responsibility: "curated durable hypotheses",
                state: stage_state(daemon_active, insight_count),
                artifact_count: insight_count,
                last_activity_at: last_insight_at,
            },
        ],
        witness_count,
        orphan_count,
    })
}

fn stage_state(daemon_active: bool, artifacts: usize) -> &'static str {
    if daemon_active {
        "active"
    } else if artifacts > 0 {
        "standing-by"
    } else {
        "idle"
    }
}

#[derive(Debug, Default, Deserialize)]
struct WitnessResponse {
    #[serde(default)]
    witnesses: Vec<MemoryRow>,
}

#[derive(Debug, Default, Deserialize)]
struct CategoryResponse {
    #[serde(default)]
    categories: Vec<CategoryRow>,
}

#[derive(Debug, Deserialize)]
struct CategoryRow {
    #[serde(default, deserialize_with = "crate::utils::nullable_string")]
    created_at: String,
}
