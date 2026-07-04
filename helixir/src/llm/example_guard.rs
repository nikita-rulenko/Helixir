//! #79: the example-leak guard — a deterministic firewall between the
//! prompts' worked examples and the store.
//!
//! The worked examples that fixed schema compliance for weak models (#78)
//! turned out to be a contamination source for the weakest ones: qwen2.5:3b
//! copied "I use ArgoCD for deployments" from the extraction prompt into a
//! user's memory as a fabricated fact. Prompt-side rules don't help — weak
//! models ignore prose, which is why the examples exist at all.
//!
//! The guard fires only on the conjunction that defines fabrication:
//! an atom RESEMBLES a worked-example sentence AND is NOT GROUNDED in the
//! user's actual message. A real ArgoCD user keeps their memories — their
//! atoms are grounded in their own words.

/// Every fact sentence appearing in a worked example across our prompts
/// (extraction + decision). Kept verbatim so token overlap is exact where
/// the model copies verbatim, and high where it lightly rephrases.
const EXAMPLE_FACTS: &[&str] = &[
    // extraction prompt (llm/extractor.rs)
    "The deploy failed on the release pipeline",
    "The auth token expired",
    "I use ArgoCD for deployments",
    "The deploy failed because the auth token expired. I use ArgoCD.",
    // decision prompt (llm/decision/prompt.rs)
    "The lexer turns source text into tokens",
    "The compiler translates source to machine code",
    "A compiler is a kind of language tool",
    "Rust is a programming language",
    "Rust is a systems language",
    "The zephyr-9 deploy failed during the night window",
    "The zephyr-9 auth token expired at midnight",
];

/// How similar to an example an atom must be before grounding is checked.
const RESEMBLANCE_BAR: f64 = 0.6;
/// Below this grounding in the raw message, a resembling atom is a leak.
const GROUNDING_BAR: f64 = 0.3;

fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|t| t.len() > 2) // drops stopword-sized noise (a, the, of, я, мы)
        .map(String::from)
        .collect()
}

/// Containment coefficient: |A ∩ B| / |A| — how much of `a` is covered by `b`.
fn containment(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() {
        return 0.0;
    }
    let hits = a.iter().filter(|t| b.contains(t)).count();
    hits as f64 / a.len() as f64
}

/// True when `atom` looks copied from a worked example and its content is
/// absent from the user's actual message — the signature of a fabricated
/// memory. `raw_message` is the text the user really sent.
pub fn is_example_leak(atom: &str, raw_message: &str) -> bool {
    let atom_tokens = tokens(atom);
    if atom_tokens.is_empty() {
        return false;
    }
    let resembles = EXAMPLE_FACTS
        .iter()
        .any(|ex| containment(&atom_tokens, &tokens(ex)) >= RESEMBLANCE_BAR);
    if !resembles {
        return false;
    }
    containment(&atom_tokens, &tokens(raw_message)) < GROUNDING_BAR
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact fabrication observed live (qwen2.5:3b, 2026-07-03): a
    /// Russian causal message about a deploy produced the prompt's ArgoCD
    /// sentence as a stored "fact".
    #[test]
    fn observed_leak_is_caught() {
        let raw = "Вчера ночью упал продовый деплой, потому что в полночь истёк auth-токен. \
                   После ротации токена деплой на ретрае прошёл успешно.";
        assert!(is_example_leak("I use ArgoCD for deployments", raw));
    }

    #[test]
    fn genuine_argocd_user_is_kept() {
        let raw = "We migrated our CD to ArgoCD last sprint; I use ArgoCD for all deployments now.";
        assert!(!is_example_leak("I use ArgoCD for deployments", raw));
    }

    #[test]
    fn grounded_deploy_fact_is_kept() {
        // Resembles the example's vocabulary but is what the user actually said.
        let raw = "the deploy failed on the release pipeline after the config change";
        assert!(!is_example_leak(
            "The deploy failed on the release pipeline",
            raw
        ));
    }

    #[test]
    fn unrelated_atom_is_kept() {
        let raw = "совещание перенесли на пятницу";
        assert!(!is_example_leak("The meeting moved to Friday", raw));
    }

    #[test]
    fn decision_example_leak_is_caught() {
        let raw = "надо купить молоко и хлеб";
        assert!(is_example_leak(
            "The lexer turns source text into tokens",
            raw
        ));
    }
}
