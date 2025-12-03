

use super::models::SimilarMemory;


pub const SYSTEM_PROMPT: &str = r#"You are a memory management expert. Analyze the new memory and similar existing memories to decide what operation to perform.

Your goal is to:
1. Prevent duplicate information
2. Keep memory coherent and up-to-date
3. Resolve conflicts (prefer newer information)
4. Maintain information quality

Always respond with valid JSON."#;


pub fn build_decision_prompt(
    new_memory: &str,
    similar_memories: &[SimilarMemory],
    user_id: &str,
) -> String {
    let similar_str = similar_memories
        .iter()
        .map(|m| {
            format!(
                "  ID: {}\n  Content: {}\n  Similarity: {:.2}\n  Created: {}\n",
                m.id,
                m.content,
                m.score,
                m.created_at.as_deref().unwrap_or("unknown")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"Analyze this new memory and decide what operation to perform.

**New Memory:**
"{new_memory}"

**Similar Existing Memories:**
{similar_str}

**User ID:** {user_id}

**Your Task:**
Decide what to do with the new memory. Choose ONE operation:

1. **ADD** - Add as completely new memory
   - Use when: Information is new and different

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
   - Set `supersedes_memory_id` to old memory ID

6. **CONTRADICT** - Mark logical conflict between memories
   - Use when: Two memories contradict but both might be valid
   - Set `contradicts_memory_id` to conflicting memory ID

**Response Format (JSON):**
{{
  "operation": "ADD|UPDATE|DELETE|NOOP|SUPERSEDE|CONTRADICT",
  "target_memory_id": "mem_xxx" or null,
  "confidence": 0-100,
  "reasoning": "Why you made this decision",
  "merged_content": "New combined content" or null,
  "supersedes_memory_id": "mem_xxx" or null,
  "contradicts_memory_id": "mem_xxx" or null,
  "relates_to": [["mem_xxx", "IMPLIES"]] or null
}}

**Important:**
- SUPERSEDE for temporal evolution, UPDATE for adding details
- CONTRADICT keeps both, DELETE removes one
- Be conservative with DELETE
- Use NOOP to avoid duplicates"#
    )
}

