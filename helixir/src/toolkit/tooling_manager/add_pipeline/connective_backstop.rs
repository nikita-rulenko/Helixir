//! Deterministic causal-connective backstop (#66 hardening).
//!
//! "Reasons in chains" is the product's flagship claim, yet a fresh user's
//! FIRST causal write depended entirely on the extractor's mood: it had to
//! split the clause into two atoms AND emit a relation, or no edge existed at
//! all (relation inference needs neighbours a fresh user does not have).
//! Measured live: three consecutive "X because Y" writes could produce zero
//! edges — worse on fallback-tier models.
//!
//! The backstop makes the floor deterministic: when the RAW message carries an
//! explicit causal connective, at least two atoms were stored from it, and the
//! whole pipeline produced ZERO relations — wire a BECAUSE edge between the
//! two atoms that best align with the clause's cause/effect sides. The LLM
//! path stays first and, when it works, produces richer edges; this fires only
//! when it produced nothing.

/// (connective, cause_side_is_second) — for "X because Y" the cause is the
/// SECOND clause; for "X therefore Y" the cause is the FIRST.
const CONNECTIVES: &[(&str, bool)] = &[
    (" because ", true),
    (" therefore ", false),
    (" потому что ", true),
    (" так как ", true),
    (" из-за ", true),
    (" поэтому ", false),
];

/// Split `message` at the first known causal connective. Returns
/// `(cause_text, effect_text)` or None when no connective is present.
pub(super) fn split_causal(message: &str) -> Option<(String, String)> {
    let lower = message.to_lowercase();
    for (conn, cause_is_second) in CONNECTIVES {
        if let Some(pos) = lower.find(conn) {
            let first = message[..pos].trim().to_string();
            let second = message[pos + conn.len()..].trim().to_string();
            if first.is_empty() || second.is_empty() {
                return None;
            }
            return Some(if *cause_is_second {
                (second, first)
            } else {
                (first, second)
            });
        }
    }
    None
}

/// Crude-but-deterministic token overlap: how many words (len > 3) of
/// `clause` appear in `atom`.
fn overlap(atom: &str, clause: &str) -> usize {
    let atom_lower = atom.to_lowercase();
    clause
        .to_lowercase()
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .filter(|w| atom_lower.contains(*w))
        .count()
}

/// Pick (cause_atom_index, effect_atom_index) among the stored atoms by
/// aligning each clause side with its best-overlapping atom. Returns None
/// when both sides land on the SAME atom (no pair to wire).
pub(super) fn pick_cause_effect(
    atom_texts: &[&str],
    cause_text: &str,
    effect_text: &str,
) -> Option<(usize, usize)> {
    let best = |clause: &str| -> usize {
        atom_texts
            .iter()
            .enumerate()
            .max_by_key(|(_, a)| overlap(a, clause))
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    let cause = best(cause_text);
    let effect = best(effect_text);
    if cause == effect {
        None
    } else {
        Some((cause, effect))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_and_orients_causal_clauses() {
        let (cause, effect) = split_causal("The deploy failed because the token expired").unwrap();
        assert_eq!(cause, "the token expired");
        assert_eq!(effect, "The deploy failed");

        let (cause, effect) = split_causal("Кэш прогрет, поэтому ответы быстрые").unwrap();
        assert_eq!(cause, "Кэш прогрет,");
        assert_eq!(effect, "ответы быстрые");

        assert!(split_causal("no connective here").is_none());
    }

    #[test]
    fn aligns_atoms_to_clause_sides() {
        let atoms = vec![
            "The deploy failed on the third stage.",
            "The auth token expired at midnight.",
        ];
        let picked = pick_cause_effect(
            &atoms.iter().map(|s| *s).collect::<Vec<_>>(),
            "the token expired",
            "the deploy failed",
        );
        assert_eq!(picked, Some((1, 0)), "cause=token atom, effect=deploy atom");

        // Both sides matching one atom → no pair.
        let one = vec!["everything in one atom about deploy and token"];
        assert!(
            pick_cause_effect(
                &one.iter().map(|s| *s).collect::<Vec<_>>(),
                "token",
                "deploy"
            )
            .is_none()
        );
    }
}
