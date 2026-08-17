pub fn get_cognitive_protocol() -> &'static str {
    include_str!("prompts/cognitive_protocol.md")
}

pub fn get_server_instructions() -> String {
    "You have PERSISTENT MEMORY through Helixir — a knowledge graph you SHARE with other agents as a collective. \
     You are NOT stateless: you accumulate experience across sessions and can draw on what other agents have already learned. \
     ALWAYS: \
     (1) Call search_memory at the start of a conversation to recall context. If it returns nothing for your user_id, \
     re-run it with scope='collective' BEFORE concluding you have no memory — the store is shared, not per-agent. \
     (2) Save decisions and outcomes with add_memory; state causes and structure EXPLICITLY (\"because\", \"is part of\", \"is a kind of\") — explicit connectives guarantee typed edges the whole swarm can later walk. If it returns needs_clarification, surface those questions to the user; \
     never resolve a flagged conflict silently. \
     (3) Use the FastThink tools (think_start → think_add → think_recall → think_conclude → think_commit) for complex, multi-step reasoning. \
     (4) Activate the cognitive role matching the task (researcher / architect / developer / mentor / creative / analyst). \
     (5) Read results as CURATED, not raw: they are capped at the top-K by score; metadata.collapsed on a result lists \
     same-story ids folded under it (content reachable by id, never lost); a thin recall means ask a sharper question, \
     not that the memory is empty (older memories may be stored in English even when the conversation was not — \
     if a recall in the conversation's language is thin, retry the same query in English). Moirai-generated hypotheses are an admin-only layer; \
     ordinary recalls and reasoning chains never treat their provenance as asserted truth. \
     (6) To recall a PERIOD, pass time_from/time_to to search_memory; rows outside the window that the graph pulled in \
     arrive flagged flashback with their event_date — present them as dated associations, not as events of that period. \
     (7) RBAC is permanent and fail-closed: pass your stable actor_id on every call, and pass a concrete group_id on \
     working-group writes; user_id is provenance, not a credential, and a dedup federation id is never a write target. \
     (8) Read pending_outcomes on writes: surface ops_alert notices and settle contradiction_review notices with \
     resolve_contradiction instead of silently choosing a winner. \
     Your memory is your identity.".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_instructions_cover_permanent_rbac_and_outbox_handling() {
        let instructions = get_server_instructions();

        for required in [
            "RBAC is permanent",
            "actor_id",
            "group_id",
            "pending_outcomes",
            "resolve_contradiction",
        ] {
            assert!(
                instructions.contains(required),
                "server instructions must mention {required}"
            );
        }
    }

    #[test]
    fn cognitive_protocol_explains_session_and_worker_presence() {
        let protocol = get_cognitive_protocol();

        for required in ["HELIXIR_RBAC_ACTOR", "distinct worker", "agent_farewell"] {
            assert!(
                protocol.contains(required),
                "cognitive protocol must mention {required}"
            );
        }
    }
}
