//! Memory charter — the escalation policy of the write path.
//!
//! The charter decides what Helixir may resolve on its own and what must be
//! surfaced to the agent (and through it, to the human) as a clarification.
//! See `memory-charter.md` at the crate root for the human-readable text;
//! the constitution rules implemented here mirror its §1.
//!
//! Current mode: **flag, don't block** — decisions still execute, conflicts
//! are reported in `add_memory.needs_clarification`. Blocking semantics are
//! a later increment, after the charter text is approved.

use crate::llm::decision::{MemoryDecision, MemoryOperation};

/// Confidence below which destructive operations are flagged (charter C5).
const LOW_CONFIDENCE: u8 = 70;

/// Memory types whose rewrites always escalate (charter C3): a reversed
/// preference / goal / decision may be a real change of mind, a different
/// context, or an extraction error — only the human knows which.
const PROTECTED_TYPES: [&str; 3] = ["preference", "goal", "opinion"];

/// Returns the charter conflict type if this decision must be surfaced to
/// the agent, or `None` if Helixir may resolve it silently.
/// `memory_type` is the ontology type of the NEW fact being written.
pub fn escalation_reason(decision: &MemoryDecision, memory_type: &str) -> Option<&'static str> {
    match decision.operation {
        // C1: memory never deletes itself silently.
        MemoryOperation::Delete => Some("auto_delete"),
        // C3: a contradiction is kept (non-destructive) but the human may
        // know which side is true, or that both are.
        MemoryOperation::Contradict => Some("contradiction"),
        MemoryOperation::CrossContradict => Some("cross_user_contradiction"),
        // C3: rewrites of preferences/goals/opinions escalate even when the
        // engine is confident — SUPERSEDE@90 of a preference is still a
        // change of mind only the human can confirm.
        MemoryOperation::Update | MemoryOperation::Supersede
            if PROTECTED_TYPES.contains(&memory_type) =>
        {
            Some("protected_type_rewrite")
        }
        // C5: low-confidence rewrites of anything else.
        MemoryOperation::Update | MemoryOperation::Supersede
            if decision.confidence < LOW_CONFIDENCE =>
        {
            Some("low_confidence_rewrite")
        }
        _ => None,
    }
}

/// A suggested question the agent can ask the user verbatim.
pub fn suggested_question(conflict_type: &str, new_content: &str, existing: &str) -> String {
    let new_short: String = new_content.chars().take(120).collect();
    let old_short: String = existing.chars().take(120).collect();
    match conflict_type {
        "contradiction" | "cross_user_contradiction" => format!(
            "Новый факт «{new_short}» противоречит сохранённому «{old_short}». \
             Что-то изменилось, это разные контексты, или одна из версий неверна?"
        ),
        "protected_type_rewrite" => format!(
            "Зафиксированное «{old_short}» предлагается заменить на «{new_short}». \
             Это смена решения/предпочтения, другой контекст, или ошибка?"
        ),
        "low_confidence_rewrite" => format!(
            "Не уверен, стоит ли заменить «{old_short}» на «{new_short}». Заменить?"
        ),
        "auto_delete" => format!(
            "Предлагается удалить память «{old_short}». Память ничего не удаляет \
             автоматически — подтверди удаление."
        ),
        _ => format!("Нужно решение по факту «{new_short}»."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(operation: MemoryOperation, confidence: u8) -> MemoryDecision {
        MemoryDecision {
            operation,
            confidence,
            ..Default::default()
        }
    }

    #[test]
    fn constitution_rules() {
        // C1: delete always escalates, even at full confidence.
        assert_eq!(
            escalation_reason(&decision(MemoryOperation::Delete, 100), "fact"),
            Some("auto_delete")
        );
        // C3: contradictions always escalate.
        assert_eq!(
            escalation_reason(&decision(MemoryOperation::Contradict, 95), "fact"),
            Some("contradiction")
        );
        // C5: low-confidence rewrites escalate, confident ones do not.
        assert_eq!(
            escalation_reason(&decision(MemoryOperation::Update, 50), "fact"),
            Some("low_confidence_rewrite")
        );
        assert_eq!(
            escalation_reason(&decision(MemoryOperation::Update, 90), "fact"),
            None
        );
        // C3: protected types escalate on rewrite even at high confidence.
        assert_eq!(
            escalation_reason(&decision(MemoryOperation::Supersede, 95), "preference"),
            Some("protected_type_rewrite")
        );
        assert_eq!(
            escalation_reason(&decision(MemoryOperation::Update, 95), "goal"),
            Some("protected_type_rewrite")
        );
        // Plain adds and noops are silent.
        assert_eq!(escalation_reason(&decision(MemoryOperation::Add, 10), "fact"), None);
        assert_eq!(
            escalation_reason(&decision(MemoryOperation::Noop, 10), "preference"),
            None
        );
    }
}
