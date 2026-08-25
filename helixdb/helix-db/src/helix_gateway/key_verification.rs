use crate::protocol::HelixError;
use sha2::{Digest, Sha256};
use std::sync::LazyLock;
use subtle::ConstantTimeEq;

/// API KEY HASH (pre-computed SHA-256 hash read from HELIX_API_KEY env var on startup)
static API_KEY_HASH: LazyLock<[u8; 32]> = LazyLock::new(|| {
    let key = std::env::var("HELIX_API_KEY").unwrap_or_default();
    if key.is_empty() {
        return [0u8; 32];
    }
    // Decode hex string to bytes
    let mut hash = [0u8; 32];
    if key.len() == 64 {
        for (i, chunk) in key.as_bytes().chunks(2).enumerate() {
            if let Ok(byte) = u8::from_str_radix(std::str::from_utf8(chunk).unwrap_or("00"), 16) {
                hash[i] = byte;
            }
        }
    }
    hash
});

#[inline(always)]
pub(crate) fn verify_key(key: &str) -> Result<(), HelixError> {
    if *API_KEY_HASH == [0u8; 32] {
        return Err(HelixError::InvalidApiKey);
    }
    let provided_hash = Sha256::digest(key.as_bytes());
    if provided_hash.ct_eq(&API_KEY_HASH[..]).into() {
        Ok(())
    } else {
        Err(HelixError::InvalidApiKey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    // ============================================================================
    // Key Verification Tests
    // ============================================================================

    fn compute_hash(key: &str) -> [u8; 32] {
        Sha256::digest(key.as_bytes()).into()
    }

    fn hash_to_hex(hash: &[u8; 32]) -> String {
        hash.iter().map(|b| format!("{:02x}", b)).collect()
    }

    #[test]
    fn test_sha256_verify_correct_key() {
        let test_key = "test-api-key-12345";
        let hash = compute_hash(test_key);

        // Verify that the hash of the same key matches
        let provided_hash = Sha256::digest(test_key.as_bytes());
        assert!(bool::from(provided_hash.ct_eq(&hash[..])));
    }

    #[test]
    fn test_sha256_verify_wrong_key() {
        let test_key = "test-api-key-12345";
        let wrong_key = "wrong-api-key";
        let hash = compute_hash(test_key);

        // Verify that wrong key fails
        let provided_hash = Sha256::digest(wrong_key.as_bytes());
        assert!(!bool::from(provided_hash.ct_eq(&hash[..])));
    }

    #[test]
    fn test_sha256_verify_empty_key() {
        let test_key = "test-api-key-12345";
        let hash = compute_hash(test_key);

        // Empty key should not verify
        let provided_hash = Sha256::digest("".as_bytes());
        assert!(!bool::from(provided_hash.ct_eq(&hash[..])));
    }

    #[test]
    fn test_sha256_verify_similar_key() {
        let test_key = "test-api-key-12345";
        let similar_key = "test-api-key-12346"; // Off by one character
        let hash = compute_hash(test_key);

        // Similar key should not verify
        let provided_hash = Sha256::digest(similar_key.as_bytes());
        assert!(!bool::from(provided_hash.ct_eq(&hash[..])));
    }

    #[test]
    fn test_sha256_hash_format() {
        let test_key = "test-api-key";
        let hash = compute_hash(test_key);
        let hex_hash = hash_to_hex(&hash);

        // SHA-256 hashes are 32 bytes (64 hex characters)
        assert_eq!(hash.len(), 32);
        assert_eq!(hex_hash.len(), 64);
        // Should be valid hex
        assert!(hex_hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_hex_decode_roundtrip() {
        let test_key = "test-api-key";
        let hash = compute_hash(test_key);
        let hex_hash = hash_to_hex(&hash);

        // Decode the hex back to bytes
        let mut decoded = [0u8; 32];
        for (i, chunk) in hex_hash.as_bytes().chunks(2).enumerate() {
            decoded[i] = u8::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16).unwrap();
        }

        assert_eq!(hash, decoded);
    }
}
