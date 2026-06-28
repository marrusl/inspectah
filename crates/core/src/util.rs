use sha2::{Digest, Sha256};

/// Compute a short hash (12 hex chars) from a path for use in language environment identifiers.
///
/// This generates a stable, deterministic hash from the path string that can be used to create
/// unique identifiers for language environments at different paths.
pub fn env_hash(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..6])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_hash_stability() {
        let path = "/opt/python-envs/myapp";
        let hash1 = env_hash(path);
        let hash2 = env_hash(path);
        assert_eq!(hash1, hash2, "hash should be stable for same input");
        assert_eq!(hash1.len(), 12, "hash should be 12 hex chars");
    }

    #[test]
    fn test_env_hash_uniqueness() {
        let hash1 = env_hash("/opt/env1");
        let hash2 = env_hash("/opt/env2");
        assert_ne!(
            hash1, hash2,
            "different paths should produce different hashes"
        );
    }

    #[test]
    fn test_env_hash_hex_chars() {
        let hash = env_hash("/some/path");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash should only contain hex chars"
        );
    }
}
