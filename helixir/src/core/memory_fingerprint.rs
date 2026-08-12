//! Stable content fingerprints shared by normal and system-memory writers.

use sha2::{Digest, Sha256};

pub(crate) fn content_key(text: &str, memory_type: &str) -> String {
    content_key_scoped(text, memory_type, None)
}

pub(crate) fn content_key_scoped(
    text: &str,
    memory_type: &str,
    fingerprint_scope: Option<&str>,
) -> String {
    let normalized = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut hasher = Sha256::new();
    if let Some(scope) = fingerprint_scope {
        hasher.update(scope.as_bytes());
        hasher.update([0u8]);
    }
    hasher.update(memory_type.to_lowercase().as_bytes());
    hasher.update([0u8]);
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}
