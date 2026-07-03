//! Memory charter — the escalation policy of the write path.
//!
//! The charter decides what Helixir may resolve on its own and what must be
//! surfaced to the agent (and through it, to the human) as a clarification.
//! See `memory-charter.md` at the crate root for the human-readable text;
//! the constitution rules implemented here mirror its §1.
//!
//! Current mode (increment 2, #34): **defer, don't destroy** — a destructive
//! verdict that the charter escalates is NOT executed; the new fact is stored
//! alongside the old, the dispute lives on a `charter_deferred` CONTRADICTS
//! edge, and `resolve_contradiction` settles it (retract = the supersede
//! happens then, with history). Non-destructive escalations stay flag-only.
//! Opt out with `write.charter_blocking = false`.

use crate::llm::decision::{MemoryDecision, MemoryOperation};

/// Memory types whose rewrites always escalate (charter C3): a reversed
/// preference / goal / decision may be a real change of mind, a different
/// context, or an extraction error — only the human knows which.
pub const PROTECTED_TYPES: [&str; 3] = ["preference", "goal", "opinion"];

/// Returns the charter conflict type if this decision must be surfaced to
/// the agent, or `None` if Helixir may resolve it silently.
/// `memory_type` is the NEW fact's ontology type; `target_type` is the type
/// of the memory being rewritten, when known. Either side being protected
/// triggers C3 — extraction typing is noisy, and rewriting a stored
/// preference is protected even when the new fact got classified "fact".
pub fn escalation_reason(
    decision: &MemoryDecision,
    memory_type: &str,
    target_type: Option<&str>,
    low_confidence: u8,
) -> Option<&'static str> {
    let touches_protected = PROTECTED_TYPES.contains(&memory_type)
        || target_type.is_some_and(|t| PROTECTED_TYPES.contains(&t));
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
        MemoryOperation::Update | MemoryOperation::Supersede if touches_protected => {
            Some("protected_type_rewrite")
        }
        // C5: low-confidence rewrites of anything else.
        MemoryOperation::Update | MemoryOperation::Supersede
            if decision.confidence < low_confidence =>
        {
            Some("low_confidence_rewrite")
        }
        _ => None,
    }
}

/// Increment 2 (#34): conflicts whose pending operation is DESTRUCTIVE get
/// DEFERRED under `write.charter_blocking` — the new fact is stored alongside
/// the old one and a `charter_deferred` CONTRADICTS edge carries the dispute
/// until `resolve_contradiction` settles it (retract = supersede then).
/// Contradiction verdicts are already non-destructive; they stay flag-only.
pub fn defers_under_blocking(decision: &MemoryDecision) -> bool {
    matches!(
        decision.operation,
        MemoryOperation::Update | MemoryOperation::Supersede | MemoryOperation::Delete
    )
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
        "low_confidence_rewrite" => {
            format!("Не уверен, стоит ли заменить «{old_short}» на «{new_short}». Заменить?")
        }
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
            escalation_reason(&decision(MemoryOperation::Delete, 100), "fact", None, 70),
            Some("auto_delete")
        );
        // C3: contradictions always escalate.
        assert_eq!(
            escalation_reason(&decision(MemoryOperation::Contradict, 95), "fact", None, 70),
            Some("contradiction")
        );
        // C5: low-confidence rewrites escalate, confident ones do not.
        assert_eq!(
            escalation_reason(&decision(MemoryOperation::Update, 50), "fact", None, 70),
            Some("low_confidence_rewrite")
        );
        assert_eq!(
            escalation_reason(&decision(MemoryOperation::Update, 90), "fact", None, 70),
            None
        );
        // C3: protected types escalate on rewrite even at high confidence.
        assert_eq!(
            escalation_reason(
                &decision(MemoryOperation::Supersede, 95),
                "preference",
                None,
                70,
            ),
            Some("protected_type_rewrite")
        );
        assert_eq!(
            escalation_reason(&decision(MemoryOperation::Update, 95), "goal", None, 70),
            Some("protected_type_rewrite")
        );
        // C3 via the TARGET: rewriting a stored preference escalates even
        // when the new fact was (mis)classified as a plain fact.
        assert_eq!(
            escalation_reason(
                &decision(MemoryOperation::Supersede, 95),
                "fact",
                Some("preference"),
                70,
            ),
            Some("protected_type_rewrite")
        );
        // Plain adds and noops are silent.
        assert_eq!(
            escalation_reason(&decision(MemoryOperation::Add, 10), "fact", None, 70),
            None
        );
        assert_eq!(
            escalation_reason(&decision(MemoryOperation::Noop, 10), "preference", None, 70),
            None
        );
    }
}
