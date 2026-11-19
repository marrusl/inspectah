use inspectah_core::types::redaction::{
    Confidence, DetectionMethod, FindingKind, RedactionFinding, RedactionKind,
};
use regex::Regex;
use std::sync::LazyLock;

/// A compiled pattern with its metadata for secret detection.
pub(crate) struct SecretPattern {
    pub regex: Regex,
    pub finding_kind: FindingKind,
    pub detection_method: DetectionMethod,
    pub confidence: Confidence,
    /// Human-readable remediation advice.
    pub remediation: &'static str,
}

/// Name of the capture group holding the secret itself.
///
/// A pattern that matches context around the secret (a key and its
/// delimiter, a URI's scheme and user, the bytes either side of an AWS key
/// id) puts the secret in `(?P<secret>...)`. The engine replaces only that
/// group, filters on that group's text, and leaves the rest of the match as
/// written. A pattern without the group is replaced whole, which is right
/// only when the whole match is the secret (a PEM block, a hash).
pub(crate) const SECRET_GROUP: &str = "secret";

/// All compiled patterns. `LazyLock` ensures they compile exactly once.
pub(crate) static PATTERNS: LazyLock<Vec<SecretPattern>> = LazyLock::new(|| {
    vec![
        // PEM private key blocks (full BEGIN...END matching with dotall)
        SecretPattern {
            regex: Regex::new(
                r"(?s)-----BEGIN\s+(?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----.*?-----END\s+(?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
            )
            .unwrap(),
            finding_kind: FindingKind::PrivateKey,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Remove private key or exclude file from snapshot",
        },
        // PEM certificate blocks (full BEGIN...END matching)
        SecretPattern {
            regex: Regex::new(
                r"(?s)-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----",
            )
            .unwrap(),
            finding_kind: FindingKind::Certificate,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Review whether certificate exposure is intended; check for adjacent private keys",
        },
        // Modular crypt password hashes ($1$, $2b$, $5$, $6$, $y$, etc.)
        // anywhere in content. Shadow files are handled separately by
        // scan_shadow, but hashes appear in htpasswd, kickstart, ansible,
        // and other configs.
        SecretPattern {
            regex: Regex::new(
                r"\$(?:1|2[aby]?|5|6|y|gy|7|sha1)\$(?:[A-Za-z0-9./+=]+\$){1,2}[A-Za-z0-9./+=]+",
            )
            .unwrap(),
            finding_kind: FindingKind::PasswordHash,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Remove password hash or use a secrets manager",
        },
        // Generic password/credential in key=value config.
        // Covers password=, passwd=, db_password=, secret=, api_key=, token=,
        // credential=, private_key=, access_key=, secret_key=, *_key= (compound
        // forms only — bare "key=" is too broad for config files).
        // Uses (?:^|\b|_) instead of bare \b so compound forms like
        // iscsi_password= or db_password= are caught (underscore is a word
        // character, so \b alone misses mid-identifier boundaries).
        // Whitespace around the delimiter is `[ \t]*`, never `\s*`: `\s`
        // matches a newline, so after an empty value (`proxy_password =` in
        // the stock rhsm.conf) the match ran on into the next line and the
        // token replaced it. `\S+` then requires a value on the same line.
        // Only the value is the secret: the key, delimiter and spacing stay,
        // so `proxy_password = x` still parses as INI and
        // `Environment=API_TOKEN=x` is still a valid assignment.
        SecretPattern {
            regex: Regex::new(
                r"(?i)(?:^|\b|_)(?:password|passwd|db_password|secret|api_key|api_secret|token|credential|private_key|access_key|secret_key|auth_key|encryption_key|signing_key|master_key|service_key)[ \t]*[=:][ \t]*(?P<secret>\S+)",
            )
            .unwrap(),
            finding_kind: FindingKind::Password,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation:
                "Use environment variables or a secrets manager instead of inline passwords",
        },
        // AWS access key ID (AKIA...). The boundary characters either side
        // are context, not secret: replacing them ate the `=` before the key
        // and the newline after it.
        SecretPattern {
            regex: Regex::new(r"(?:^|[^A-Z0-9])(?P<secret>AKIA[0-9A-Z]{16})(?:[^A-Z0-9]|$)").unwrap(),
            finding_kind: FindingKind::AwsKey,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Rotate the AWS access key and use IAM roles",
        },
        // JDBC connection string with password. The secret is the query
        // string: from just after `?` through the last `password=` value on
        // the line and on to the next whitespace. Any parameter there can
        // carry a credential (`token=`, a second `password=`, a nested URI),
        // and naming only the last password's value left the others to
        // patterns it then overlapped. The scheme, host and database stay.
        // No class here admits a line break, so a URL with no password
        // cannot pair with a `?password=` on a later line; spaces and tabs
        // are allowed, as a host's malformed URL can hold them.
        SecretPattern {
            regex: Regex::new(r"jdbc:[a-z]+://[^?\r\n]+\?(?P<secret>[^\r\n]*password=[^&\s]\S*)").unwrap(),
            finding_kind: FindingKind::JdbcPassword,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Use connection pooling with externalized credentials",
        },
        // PostgreSQL connection URI with password. Only the password is
        // replaced; the scheme, user and `@host` stay. The user and
        // password classes exclude CR and LF: with `[^:]+` / `[^@]+` a
        // password-less URI paired with a `contact: ops@host` line below
        // it. They exclude nothing else, so a password with a space or tab
        // in it (`postgres://u:two words@db`) is still redacted.
        SecretPattern {
            regex: Regex::new(r"postgres(?:ql)?://[^:\r\n]+:(?P<secret>[^@\r\n]+)@").unwrap(),
            finding_kind: FindingKind::PostgresPassword,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Use .pgpass or environment variables for PostgreSQL credentials",
        },
        // MongoDB connection URI with password; one line, as Postgres.
        SecretPattern {
            regex: Regex::new(r"mongodb(?:\+srv)?://[^:\r\n]+:(?P<secret>[^@\r\n]+)@").unwrap(),
            finding_kind: FindingKind::MongodbPassword,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Use environment variables for MongoDB credentials",
        },
        // Redis connection URI with password; one line, as Postgres.
        SecretPattern {
            regex: Regex::new(r"redis://:(?P<secret>[^@\r\n]+)@").unwrap(),
            finding_kind: FindingKind::RedisPassword,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Use environment variables for Redis credentials",
        },
        // WireGuard private key (base64, 44 chars including trailing =)
        SecretPattern {
            regex: Regex::new(r"(?i)(?:PrivateKey|PreSharedKey)[ \t]*=[ \t]*(?P<secret>[A-Za-z0-9+/]{43}=)").unwrap(),
            finding_kind: FindingKind::WireguardKey,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Regenerate WireGuard keys and use wg-quick with externalized key files",
        },
        // WiFi PSK in wpa_supplicant or NetworkManager config
        SecretPattern {
            regex: Regex::new(r"(?i)(?:psk|wpa-psk|wifi\.psk)[ \t]*=[ \t]*(?P<secret>\S+)").unwrap(),
            finding_kind: FindingKind::WifiPsk,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Use 802.1X or externalize WiFi credentials",
        },
        // JDBC URL with a password in its authority
        // (`jdbc:mysql://user:pw@host/db`), which MySQL Connector/J and
        // MariaDB accept. The query-string pattern above starts its secret
        // at `?`, so it cannot cover this one. Last in the list so the
        // indices of the patterns above stay put.
        SecretPattern {
            regex: Regex::new(r"jdbc:[a-z]+://[^:/?@\r\n]+:(?P<secret>[^/?@\r\n]+)@").unwrap(),
            finding_kind: FindingKind::JdbcPassword,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Use connection pooling with externalized credentials",
        },
    ]
});

/// Shadow line classification result.
pub(crate) enum ShadowClassification {
    /// Locked account (!! prefix) — not a secret.
    Locked,
    /// Disabled account (* only) — not a secret.
    Disabled,
    /// Empty password field — security finding, low confidence.
    EmptyPassword { username: String },
    /// Real password hash ($N$ prefix) — secret.
    HasHash { username: String },
    /// Not a shadow line (too few fields, etc.).
    NotShadow,
}

/// Classify a single line from /etc/shadow.
pub(crate) fn classify_shadow_line(line: &str) -> ShadowClassification {
    let fields: Vec<&str> = line.split(':').collect();
    if fields.len() < 3 {
        return ShadowClassification::NotShadow;
    }

    let username = fields[0];
    let hash_field = fields[1];

    if hash_field.starts_with("!!") || hash_field == "!" {
        ShadowClassification::Locked
    } else if hash_field == "*" {
        ShadowClassification::Disabled
    } else if hash_field.is_empty() {
        ShadowClassification::EmptyPassword {
            username: username.to_string(),
        }
    } else if hash_field.starts_with('$') {
        ShadowClassification::HasHash {
            username: username.to_string(),
        }
    } else {
        // Unknown format — treat as not-shadow to avoid false positives.
        ShadowClassification::NotShadow
    }
}

/// Scan content for shadow entries. Returns findings for hashes and empty passwords.
pub(crate) fn scan_shadow(content: &str, path: &str) -> Vec<RedactionFinding> {
    let mut findings = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        match classify_shadow_line(line) {
            ShadowClassification::HasHash { username } => {
                findings.push(RedactionFinding {
                    path: path.to_string(),
                    source: "shadow".into(),
                    kind: RedactionKind::Inline,
                    pattern: "shadow_hash".into(),
                    remediation: format!(
                        "User '{}': remove password hash or lock account",
                        username
                    ),
                    line: Some((line_num + 1) as i32),
                    replacement: None,
                    detection_method: DetectionMethod::Pattern,
                    confidence: Some(Confidence::High),
                    finding_kind: Some(FindingKind::ShadowHash),
                });
            }
            ShadowClassification::EmptyPassword { username } => {
                findings.push(RedactionFinding {
                    path: path.to_string(),
                    source: "shadow".into(),
                    kind: RedactionKind::Flagged,
                    pattern: "empty_password".into(),
                    remediation: format!(
                        "User '{}': empty password field — set a password or lock account",
                        username
                    ),
                    line: Some((line_num + 1) as i32),
                    replacement: None,
                    detection_method: DetectionMethod::Pattern,
                    confidence: Some(Confidence::Low),
                    finding_kind: Some(FindingKind::NoPassword),
                });
            }
            // Locked, Disabled, NotShadow — no finding
            _ => {}
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_locked_classification() {
        assert!(matches!(
            classify_shadow_line("root:!!:19000:0:99999:7:::"),
            ShadowClassification::Locked
        ));
        assert!(matches!(
            classify_shadow_line("root:!:19000:0:99999:7:::"),
            ShadowClassification::Locked
        ));
    }

    #[test]
    fn test_shadow_disabled_classification() {
        assert!(matches!(
            classify_shadow_line("nobody:*:19000:0:99999:7:::"),
            ShadowClassification::Disabled
        ));
    }

    #[test]
    fn test_shadow_empty_classification() {
        match classify_shadow_line("nobody::19000:0:99999:7:::") {
            ShadowClassification::EmptyPassword { username } => {
                assert_eq!(username, "nobody");
            }
            other => panic!(
                "expected EmptyPassword, got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    #[test]
    fn test_shadow_hash_classification() {
        match classify_shadow_line("admin:$6$rounds=65536$salt$hash...:19000:0:99999:7:::") {
            ShadowClassification::HasHash { username } => {
                assert_eq!(username, "admin");
            }
            other => panic!("expected HasHash, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn test_pattern_count() {
        // Ensure all patterns compile and we have the expected count.
        // 9 original + Certificate + PasswordHash = 11
        assert!(PATTERNS.len() >= 11, "expected at least 11 secret patterns");
    }

    // --- Expanded pattern coverage for KEY/CREDENTIAL surfaces ---

    #[test]
    fn test_credential_pattern_matches() {
        let pat = &PATTERNS[3]; // Generic password/credential pattern
        assert!(
            pat.regex.is_match("credential = s3cretValue"),
            "bare 'credential' must match"
        );
        assert!(
            pat.regex.is_match("CREDENTIAL=foobar"),
            "uppercase CREDENTIAL must match"
        );
    }

    #[test]
    fn test_compound_key_patterns_match() {
        let pat = &PATTERNS[3];
        assert!(
            pat.regex.is_match("private_key = abc123"),
            "private_key must match"
        );
        assert!(
            pat.regex.is_match("access_key=AKIA1234567890"),
            "access_key must match"
        );
        assert!(
            pat.regex.is_match("secret_key = wJalrXUtnFEMI"),
            "secret_key must match"
        );
        assert!(
            pat.regex.is_match("auth_key=myauthkey"),
            "auth_key must match"
        );
        assert!(
            pat.regex.is_match("encryption_key = aes256key"),
            "encryption_key must match"
        );
        assert!(
            pat.regex.is_match("signing_key = rsasigningkey"),
            "signing_key must match"
        );
        assert!(
            pat.regex.is_match("master_key=masterpass"),
            "master_key must match"
        );
        assert!(
            pat.regex.is_match("service_key = svckey123"),
            "service_key must match"
        );
        assert!(
            pat.regex.is_match("api_secret = secretval"),
            "api_secret must match"
        );
    }

    #[test]
    fn test_bare_key_does_not_match() {
        let pat = &PATTERNS[3];
        // Bare "key=value" must NOT match — too broad for config files
        assert!(
            !pat.regex.is_match("key = value"),
            "bare 'key' must NOT match (too broad)"
        );
        assert!(
            !pat.regex.is_match("cache_key = abc"),
            "cache_key should not match (not security-related)"
        );
    }

    /// Index of each line-oriented `key = value` pattern in `PATTERNS`.
    const GENERIC_PASSWORD: usize = 3;
    const JDBC: usize = 5;
    const POSTGRES: usize = 6;
    const MONGODB: usize = 7;
    const REDIS: usize = 8;
    const WIREGUARD_KEY: usize = 9;
    const WIFI_PSK: usize = 10;
    const JDBC_AUTHORITY: usize = 11;

    #[test]
    fn test_key_value_patterns_never_cross_a_newline() {
        let wg_key = "A".repeat(43) + "=";
        let cases: Vec<(usize, String)> = vec![
            // Empty value followed by a comment: the rhsm.conf case.
            (
                GENERIC_PASSWORD,
                "proxy_password =\n\n# host/domain suffix".into(),
            ),
            (GENERIC_PASSWORD, "proxy_password =   \nno_proxy =".into()),
            (GENERIC_PASSWORD, "token:\n  - item".into()),
            // Newline between key and delimiter.
            (GENERIC_PASSWORD, "password\n= hunter2".into()),
            (WIFI_PSK, "psk=\n[ipv4]".into()),
            (WIFI_PSK, "psk\n=secret".into()),
            (WIREGUARD_KEY, format!("PrivateKey =\n{wg_key}")),
            (WIREGUARD_KEY, format!("PrivateKey\n= {wg_key}")),
        ];
        for (idx, text) in &cases {
            assert!(
                !PATTERNS[*idx].regex.is_match(text),
                "pattern {idx} must not match across a newline or on an empty value: {text:?}"
            );
        }
        // The same keys with a value on the same line still match.
        let same_line: Vec<(usize, String)> = vec![
            (GENERIC_PASSWORD, "proxy_password = hunter2".into()),
            (GENERIC_PASSWORD, "proxy_password =\thunter2".into()),
            (WIFI_PSK, "psk=secret".into()),
            (WIREGUARD_KEY, format!("PrivateKey = {wg_key}")),
        ];
        for (idx, text) in &same_line {
            assert!(
                PATTERNS[*idx].regex.is_match(text),
                "pattern {idx} must match a value on the same line: {text:?}"
            );
        }
    }

    #[test]
    fn test_uri_patterns_never_cross_a_line_break() {
        for nl in ["\n", "\r\n"] {
            let cases = [
                // A URI with no password, then an `x: y@z` line later.
                (
                    POSTGRES,
                    format!("url = postgres://db.example.com/app{nl}contact: ops@example.com"),
                ),
                (POSTGRES, format!("postgres://app:{nl}pw@db")),
                (
                    MONGODB,
                    format!("mongodb://db.example.com{nl}user: x@example.com"),
                ),
                (MONGODB, format!("mongodb+srv://u:{nl}pw@h")),
                (REDIS, format!("redis://:secretpw{nl}mail = a@b")),
                (JDBC_AUTHORITY, format!("jdbc:mysql://app:{nl}pw@db")),
                (
                    JDBC,
                    format!(
                        "primary=jdbc:mysql://db/one{nl}secondary=jdbc:mysql://db/two?password=hunter2"
                    ),
                ),
            ];
            for (idx, text) in &cases {
                for m in PATTERNS[*idx].regex.find_iter(text) {
                    assert!(
                        !m.as_str().contains(['\n', '\r']),
                        "pattern {idx} matched across a line break: {:?} in {text:?}",
                        m.as_str()
                    );
                }
            }
        }
        // Single-line URIs are still detected.
        let same_line = [
            (POSTGRES, "postgres://app:pw@db.example.com/app", "pw"),
            (POSTGRES, "postgresql://app:pw@db:5432/app", "pw"),
            (MONGODB, "mongodb://app:pw@db.example.com/app", "pw"),
            (MONGODB, "mongodb+srv://app:pw@cluster.example.com", "pw"),
            (REDIS, "redis://:pw@cache:6379/0", "pw"),
            (
                JDBC,
                "jdbc:mysql://db:3306/app?user=u&password=pw",
                "user=u&password=pw",
            ),
            (
                JDBC,
                "jdbc:postgresql://db/app?ssl=true&password=pw\r",
                "ssl=true&password=pw",
            ),
            (JDBC_AUTHORITY, "jdbc:mysql://app:pw@db:3306/app", "pw"),
            // A space or tab is not a line break.
            (POSTGRES, "postgres://app:two words@db", "two words"),
            (POSTGRES, "postgresql://app:tab\tbed@db", "tab\tbed"),
            (MONGODB, "mongodb://app:two words@db", "two words"),
            (MONGODB, "mongodb+srv://app:tab\tbed@db", "tab\tbed"),
            (REDIS, "redis://:two words@cache", "two words"),
            (REDIS, "redis://:tab\tbed@cache", "tab\tbed"),
            (JDBC_AUTHORITY, "jdbc:mysql://app:two words@db", "two words"),
            (JDBC, "jdbc:mysql://db/my app?password=pw", "password=pw"),
        ];
        for (idx, text, secret) in same_line {
            let caps = PATTERNS[idx]
                .regex
                .captures(text)
                .unwrap_or_else(|| panic!("pattern {idx} must match {text:?}"));
            assert_eq!(&caps[SECRET_GROUP], secret, "pattern {idx} on {text:?}");
        }
    }

    #[test]
    fn test_every_pattern_with_context_declares_its_secret_group() {
        // A pattern whose match is the secret alone is replaced whole; every
        // other pattern must name the secret, or the key, delimiter or URI
        // around it is replaced too.
        let whole_match_is_secret = [
            FindingKind::PrivateKey,
            FindingKind::Certificate,
            FindingKind::PasswordHash,
        ];
        for (idx, pat) in PATTERNS.iter().enumerate() {
            let has_group = pat
                .regex
                .capture_names()
                .flatten()
                .any(|n| n == SECRET_GROUP);
            assert_eq!(
                has_group,
                !whole_match_is_secret.contains(&pat.finding_kind),
                "pattern {idx} ({:?})",
                pat.finding_kind
            );
        }
    }

    #[test]
    fn test_environment_credential_patterns() {
        // Simulates systemd drop-in: Environment=DB_KEY=value
        let content = "Environment=CREDENTIAL_FILE=/etc/secrets/db.key";
        let findings = scan_shadow(content, "/dev/null"); // scan_shadow won't match
        assert!(findings.is_empty(), "not a shadow file");

        // But the generic pattern should match credential references
        let pat = &PATTERNS[3];
        // The full line won't match directly because Environment= prefix
        // pushes the credential= out. But the pattern WILL catch:
        let env_content = "credential = /etc/secrets/db.key";
        assert!(
            pat.regex.is_match(env_content),
            "credential=... must match the expanded pattern"
        );
    }
}
