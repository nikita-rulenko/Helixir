use serde::{Deserialize, Deserializer};
use std::collections::BTreeSet;

pub fn nullable_string<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Option::<String>::deserialize(d).map(|o| o.unwrap_or_default())
}

/// Resolve the logical principal used to present an agent-presence row.
///
/// New rows always carry an explicit `principal_id`, which wins. Historical
/// rows may not; for those only, the longest existing principal prefix is a
/// display/counting fallback. Callers must never persist this answer or use it
/// for authorization.
pub fn resolve_agent_principal_for_display(
    explicit: &str,
    agent_id: &str,
    known_principals: &BTreeSet<String>,
) -> String {
    let explicit = explicit.trim();
    if !explicit.is_empty() {
        return explicit.to_string();
    }
    known_principals
        .iter()
        .filter(|principal| agent_id.starts_with(principal.as_str()))
        .max_by_key(|principal| principal.len())
        .cloned()
        .unwrap_or_else(|| agent_id.to_string())
}

#[inline]
pub fn safe_truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

#[inline]
pub fn safe_truncate_ellipsis(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        format!("{}...", s.chars().take(max_chars).collect::<String>())
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_truncate_ascii() {
        assert_eq!(safe_truncate("hello world", 5), "hello");
    }

    #[test]
    fn test_safe_truncate_cyrillic() {
        assert_eq!(safe_truncate("Привет мир", 6), "Привет");
    }

    #[test]
    fn test_safe_truncate_mixed() {
        assert_eq!(safe_truncate("Hello Мир!", 8), "Hello Ми");
    }

    #[test]
    fn test_safe_truncate_shorter() {
        assert_eq!(safe_truncate("hi", 10), "hi");
    }

    #[test]
    fn test_safe_truncate_ellipsis() {
        assert_eq!(safe_truncate_ellipsis("hello world", 5), "hello...");
        assert_eq!(safe_truncate_ellipsis("hi", 10), "hi");
    }

    #[test]
    fn display_principal_prefers_explicit_then_longest_legacy_prefix() {
        let known = ["codex".to_string(), "codex-web".to_string()]
            .into_iter()
            .collect();
        assert_eq!(
            resolve_agent_principal_for_display("claude", "codex-web-build", &known),
            "claude"
        );
        assert_eq!(
            resolve_agent_principal_for_display("", "codex-web-build", &known),
            "codex-web"
        );
        assert_eq!(
            resolve_agent_principal_for_display("", "worker", &known),
            "worker"
        );
    }
}
