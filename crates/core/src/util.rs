use sha2::{Digest, Sha256};

// ── Method string constants ──────────────────────────────────────────
// Canonical detection-method keys shared by the collector, renderer,
// and exporter.  Every crate that routes on `NonRpmItem.method` must
// reference these constants instead of inline literals.

/// Method string for Python virtual environments detected via pyvenv.cfg.
pub const METHOD_PYTHON_VENV: &str = "python venv";

/// Method string for system-level pip packages detected via dist-info.
pub const METHOD_PIP_DIST_INFO: &str = "pip dist-info";

/// Method string for npm projects detected via package-lock.json.
pub const METHOD_NPM_LOCKFILE: &str = "npm lockfile";

/// Method string for npm projects detected via package.json (no lockfile).
pub const METHOD_NPM_MANIFEST: &str = "npm manifest";

/// Method string for gem projects detected via Gemfile.lock.
pub const METHOD_GEM_LOCKFILE: &str = "gem lockfile";

/// Method string for system gems detected via `gem list --local`.
pub const METHOD_GEM_SYSTEM: &str = "gem system";

/// Method string for npm global packages detected via `npm list -g --json`.
pub const METHOD_NPM_GLOBAL: &str = "npm global";

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

/// `at` in UTC as RFC 3339, written the way the snapshot `timestamp` and
/// the aggregate `merged_at` have always been written: a `+00:00` offset,
/// and a fraction of 3, 6 or 9 digits, whichever is the shortest exact
/// one, or none for a whole second (`2026-11-23T07:15:02.120+00:00`).
pub fn rfc3339_utc(at: time::OffsetDateTime) -> String {
    let at = at.to_offset(time::UtcOffset::UTC);
    let nanos = at.nanosecond();
    let fraction = if nanos == 0 {
        String::new()
    } else if nanos.is_multiple_of(1_000_000) {
        format!(".{:03}", nanos / 1_000_000)
    } else if nanos.is_multiple_of(1_000) {
        format!(".{:06}", nanos / 1_000)
    } else {
        format!(".{nanos:09}")
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{fraction}+00:00",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second()
    )
}

/// The current time as [`rfc3339_utc`] writes it.
pub fn now_rfc3339_utc() -> String {
    rfc3339_utc(time::OffsetDateTime::now_utc())
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
    fn test_rfc3339_utc_matches_the_recorded_format() {
        let base = 1_795_418_102; // 2026-11-23T07:15:02Z
        let cases = [
            (0, "2026-11-23T07:15:02+00:00"),
            (120_000_000, "2026-11-23T07:15:02.120+00:00"),
            (120_500_000, "2026-11-23T07:15:02.120500+00:00"),
            (120_500_001, "2026-11-23T07:15:02.120500001+00:00"),
        ];
        for (nanos, expected) in cases {
            let at = time::OffsetDateTime::from_unix_timestamp(base)
                .expect("a valid timestamp")
                .replace_nanosecond(nanos)
                .expect("a valid nanosecond");
            assert_eq!(rfc3339_utc(at), expected);
            let parsed = time::OffsetDateTime::parse(
                expected,
                &time::format_description::well_known::Rfc3339,
            )
            .expect("the output parses as RFC 3339");
            assert_eq!(parsed, at);
        }
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
