use super::models::SimilarMemory;

pub const SYSTEM_PROMPT: &str = r#"You are a memory management expert. Analyze the new memory and similar existing memories to decide what operation to perform.

MEMORY CHARTER (the constitution — these override everything else):
- C1: memory never destroys itself. DELETE is never executed as deletion; prefer SUPERSEDE (history preserved) and use it only under the same-subject gate.
- C3: preferences, goals and opinions are never rewritten silently. A reversed preference may be a real change of mind, a different context, or an extraction error — when a rewrite touches one of these types, the system defers it for a human-level answer; your verdict should already lean ADD or CONTRADICT rather than SUPERSEDE unless the evidence is unmistakable.
- C5: low-confidence rewrites escalate. If you are not sure the two statements are the same subject and the new one genuinely replaces the old, choose ADD.

Your goal is to:
1. Prevent duplicate information
2. Keep memory coherent and up-to-date
3. Resolve conflicts (prefer newer information)
4. Maintain information quality
5. For cross-user memories: detect shared knowledge and conflicting preferences

Always respond with valid JSON."#;

pub fn build_decision_prompt(
    new_memory: &str,
    similar_memories: &[SimilarMemory],
    user_id: &str,
) -> String {
    let has_cross_user = similar_memories.iter().any(|m| m.is_cross_user);

    let similar_str = similar_memories
        .iter()
        .map(|m| {
            let owner_info = if m.is_cross_user {
                format!(
                    "  Owner: {} (DIFFERENT USER)\n",
                    m.user_id.as_deref().unwrap_or("unknown")
                )
            } else {
                String::new()
            };
            format!(
                "  ID: {}\n  Type: {}\n  Content: {}\n  Similarity: {:.2}\n  Created: {}\n{}",
                m.id,
                m.memory_type.as_deref().unwrap_or("unknown"),
                m.content,
                m.score,
                m.created_at.as_deref().unwrap_or("unknown"),
                owner_info
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let cross_user_section = if has_cross_user {
        r#"

**Cross-User Operations (use ONLY when memories are from DIFFERENT users):**

7. **LINK_EXISTING** - Same fact/knowledge from another user (PREFERRED for cross-user duplicates)
   - Use when: The new memory conveys the same meaning/fact as another user's memory, even if worded differently
   - This is the MOST COMMON cross-user operation — if two users know the same thing, LINK them
   - Set `link_to_memory_id` to the existing memory ID

8. **CROSS_CONTRADICT** - Conflicting preference/opinion across users
   - Use when: Different users have clearly opposing preferences, opinions, or beliefs about the same topic
   - Example: User A says "Python is best" vs User B says "Rust is best"
   - Set `contradicts_memory_id` to the conflicting memory ID
   - Set `conflict_type` to describe the conflict (e.g., "preference", "opinion", "approach")

**CRITICAL cross-user rules:**
- If a DIFFERENT USER already has the same fact/knowledge → ALWAYS use LINK_EXISTING (not ADD or NOOP)
- If a DIFFERENT USER has an opposing opinion/preference → ALWAYS use CROSS_CONTRADICT (not CONTRADICT)
- Never use ADD when a cross-user memory says the same thing — that creates wasteful duplicates"#
    } else {
        ""
    };

    let cross_user_reminder = if has_cross_user {
        "\n- For DIFFERENT USER memories: prefer LINK_EXISTING (same fact) or CROSS_CONTRADICT (opposing views)"
    } else {
        ""
    };

    let cross_user_json = if has_cross_user {
        r#",
  "link_to_memory_id": "mem_xxx" or null,
  "conflict_type": "preference|opinion|approach" or null"#
    } else {
        ""
    };

    let operations_list = if has_cross_user {
        "ADD|UPDATE|DELETE|NOOP|SUPERSEDE|CONTRADICT|LINK_EXISTING|CROSS_CONTRADICT"
    } else {
        "ADD|UPDATE|DELETE|NOOP|SUPERSEDE|CONTRADICT"
    };

    format!(
        r#"Analyze this new memory and decide what operation to perform.

**New Memory:**
"{new_memory}"

**Similar Existing Memories:**
{similar_str}

**User ID:** {user_id}

**Your Task:**
Decide what to do with the new memory. Choose ONE operation.

**FIRST apply the SAME-SUBJECT GATE (prevents false merges/conflicts):**
UPDATE, SUPERSEDE, CONTRADICT and DELETE may be used ONLY when the new memory and
the candidate describe the SAME SPECIFIC subject — the same entity, the same
attribute, the same question. High topical similarity is NOT enough: two facts
about "deduplication", or two facts mentioning "the MCP server", can score
similar yet describe DIFFERENT things. If they are merely on a RELATED topic
(not the same specific claim), the answer is ADD — then wire a RELATES_TO edge.
When unsure, prefer ADD: a wrong merge/supersede DESTROYS information, while a
missed one is only a harmless duplicate. Never UPDATE/SUPERSEDE/CONTRADICT just
because two memories share keywords or a theme.

1. **ADD** - Add as completely new memory
   - Use when: Information is new, OR is only topically related to a candidate
     (different specific subject) — this is the DEFAULT when the gate fails

2. **UPDATE** - Update existing memory with new information
   - Use when: New memory enhances or extends existing one
   - Provide `merged_content` combining both memories

3. **DELETE** - Delete existing conflicting memory
   - Use when: New memory is correct and old one is wrong
   - Specify which memory to delete via `target_memory_id`

4. **NOOP** - Ignore (duplicate or redundant)
   - Use when: Information already exists

5. **SUPERSEDE** - Replace old memory with evolved version
   - Use when: Preference/opinion changed over time
   - Use when: Both memories answer the SAME mutable question (current
     state, status, version, plan, "next step") and the new one reports a
     LATER state — even if worded very differently. "Stage X is next" vs
     "stage X is complete" is SUPERSEDE, never ADD.
   - Set `supersedes_memory_id` to old memory ID

6. **CONTRADICT** - Mark logical conflict between memories
   - Use when: Two memories contradict but both might be valid
   - Set `contradicts_memory_id` to conflicting memory ID
{cross_user_section}

**ALWAYS build typed edges via `relates_to` (this is the core value of the graph):**
Independently of the operation above, wire the new memory into the existing ones
it genuinely connects to. `relates_to` is a list of [existing_memory_id, EDGE_TYPE]
pairs. Pick the MOST SPECIFIC edge type; do NOT default everything to IMPLIES:
- CAUSAL/LOGICAL: BECAUSE (A is the cause of B), IMPLIES (A logically leads to B),
  SUPPORTS (A is evidence for B), CONTRADICTS (A conflicts with B).
- ASSOCIATIVE/STRUCTURAL: RELATES_TO (same topic / strongly related, no cause or
  hierarchy), PART_OF (A is a component of B), IS_A (A is a kind/instance of B).
Use the `Type:` (ontology) of each memory as a signal: two `preference`/`opinion`
memories on one topic that differ are usually CONTRADICT/SUPERSEDE; a `fact` that
elaborates another `fact` is RELATES_TO or SUPPORTS; a narrower concept under a
broader one is IS_A or PART_OF. When you choose ADD/UPDATE/NOOP and the new memory
is still topically related to a similar one, emit a RELATES_TO edge so the graph
stays connected rather than leaving orphan nodes.
WORKED EXAMPLE (structural) — new memory "The lexer turns source text into
tokens" with candidates [mem_1 "The compiler translates source to machine code",
mem_2 "A compiler is a kind of language tool"]: operation ADD, and
relates_to = [["mem_1","PART_OF"]] (the lexer is a component of the compiler) —
NOT SUPPORTS or IMPLIES. If a candidate were "Rust is a programming language" and
the new memory were "Rust is a systems language", that pair is IS_A. Reach for
PART_OF/IS_A whenever the structural relation is real; only fall back to
RELATES_TO when no component/kind relation holds.
WORKED EXAMPLE (causal, across separate writes) — new memory "The zephyr-9
deploy failed during the night window" with candidate mem_7 "The zephyr-9 auth
token expired at midnight" (stored days earlier): operation ADD, and
relates_to = [["mem_7","BECAUSE"]] — the candidate is the CAUSE of the new
fact, so the edge is BECAUSE, not RELATES_TO. This is the single most valuable
edge you can build: it lets a later "why did the deploy fail?" question walk
straight to the answer. Whenever a candidate states a cause, consequence or
evidence of the new memory, choose BECAUSE / IMPLIES / SUPPORTS over
RELATES_TO — topical similarity alone is what RELATES_TO is for.

**Response Format (JSON):**
{{
  "operation": "{operations_list}",
  "target_memory_id": "mem_xxx" or null,
  "confidence": 0-100,
  "reasoning": "Why you made this decision",
  "merged_content": "New combined content" or null,
  "supersedes_memory_id": "mem_xxx" or null,
  "contradicts_memory_id": "mem_xxx" or null,
  "relates_to": [["mem_xxx", "IMPLIES"]] or null{cross_user_json}
}}

**Important:**
- SUPERSEDE for temporal evolution, UPDATE for adding details
- CONTRADICT keeps both, DELETE removes one
- Be conservative with DELETE
- Use NOOP to avoid duplicates
- When using UPDATE with merged_content: the result MUST be a single coherent statement about ONE topic. Do NOT merge unrelated facts. If the new memory and existing memory are about different topics, use ADD instead.
- merged_content must NEVER contain contradictions ("X but Y", "X however Y" about different subjects){cross_user_reminder}"#
    )
}

/// Batch variant (#32 W1): one prompt deciding every gray-zone fact of an
/// add_memory call at once. Personal-phase only (no cross-user operations).
/// Kept compact on purpose — small local models handle short schemas better.
pub fn build_batch_decision_prompt(
    items: &[(usize, &str, &[SimilarMemory])],
    user_id: &str,
) -> String {
    let items_str = items
        .iter()
        .map(|(i, new_memory, candidates)| {
            let cands = candidates
                .iter()
                .map(|m| {
                    format!(
                        "    - id: {} | type: {} | sim: {:.2} | text: {}",
                        m.id,
                        m.memory_type.as_deref().unwrap_or("unknown"),
                        m.score,
                        m.content
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("ITEM {i}:\n  new: \"{new_memory}\"\n  candidates:\n{cands}")
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        r#"Decide one memory operation for EACH item below. Items are independent.

Operations: ADD (new info) | UPDATE (extends an existing memory; provide merged_content about ONE topic, no contradictions) | NOOP (duplicate) | SUPERSEDE (replaces an outdated version; set supersedes_memory_id) | CONTRADICT (conflicts but both may be valid; set contradicts_memory_id) | DELETE (old one is plainly wrong; set target_memory_id).

SAME-SUBJECT GATE (most important): UPDATE/SUPERSEDE/CONTRADICT/NOOP/DELETE are allowed ONLY when the item and the candidate are about the SAME SPECIFIC subject (same entity + same attribute/claim). Shared keywords or a shared theme is NOT enough — two facts both about "dedup" or both mentioning "the MCP server" are usually DIFFERENT facts → ADD. If in doubt, ADD (a wrong merge destroys info; a missed one is a harmless duplicate).

Rules: be conservative with DELETE; SUPERSEDE for temporal evolution AND whenever both memories answer the same mutable question (state/status/plan) with the new one reporting a later state — even if worded differently; UPDATE for added detail; NOOP for exact duplicates; never merge unrelated or merely-related topics.

ALSO build typed edges: for each item, set `relates_to` to a list of [candidate_id, EDGE_TYPE] for every candidate the item genuinely connects to (even when the operation is ADD/NOOP — keep the graph connected). EDGE_TYPE, most specific first: BECAUSE / IMPLIES / SUPPORTS / CONTRADICTS (causal) or RELATES_TO / PART_OF / IS_A (associative). Do not default to IMPLIES; use RELATES_TO for plain topical relatedness. Use each candidate's `type:` (ontology) as a signal.

**User ID:** {user_id}

{items_str}

Respond with JSON only:
{{"decisions":[{{"i": <item number>, "operation": "ADD|UPDATE|NOOP|SUPERSEDE|CONTRADICT|DELETE", "target_memory_id": null, "confidence": 0-100, "reasoning": "...", "merged_content": null, "supersedes_memory_id": null, "contradicts_memory_id": null, "relates_to": [["mem_xxx","RELATES_TO"]] }}]}}
Every item number must appear exactly once."#
    )
}
