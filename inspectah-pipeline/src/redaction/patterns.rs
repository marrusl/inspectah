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

/// All compiled patterns. `LazyLock` ensures they compile exactly once.
pub(crate) static PATTERNS: LazyLock<Vec<SecretPattern>> = LazyLock::new(|| {
    vec![
        // PEM private key headers
        SecretPattern {
            regex: Regex::new(r"-----BEGIN\s+(?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----").unwrap(),
            finding_kind: FindingKind::PrivateKey,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Remove private key or exclude file from snapshot",
        },
        // Generic password in key=value config (password=, passwd=, db_password=, etc.)
        SecretPattern {
            regex: Regex::new(
                r"(?i)(?:password|passwd|db_password|secret|api_key|token)\s*[=:]\s*\S+",
            )
            .unwrap(),
            finding_kind: FindingKind::Password,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Use environment variables or a secrets manager instead of inline passwords",
        },
        // AWS access key ID (AKIA...)
        SecretPattern {
            regex: Regex::new(r"(?:^|[^A-Z0-9])AKIA[0-9A-Z]{16}(?:[^A-Z0-9]|$)").unwrap(),
            finding_kind: FindingKind::AwsKey,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Rotate the AWS access key and use IAM roles",
        },
        // JDBC connection string with password
        SecretPattern {
            regex: Regex::new(r"jdbc:[a-z]+://[^?]+\?.*password=[^&\s]+").unwrap(),
            finding_kind: FindingKind::JdbcPassword,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Use connection pooling with externalized credentials",
        },
        // PostgreSQL connection URI with password
        SecretPattern {
            regex: Regex::new(r"postgres(?:ql)?://[^:]+:[^@]+@").unwrap(),
            finding_kind: FindingKind::PostgresPassword,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Use .pgpass or environment variables for PostgreSQL credentials",
        },
        // MongoDB connection URI with password
        SecretPattern {
            regex: Regex::new(r"mongodb(?:\+srv)?://[^:]+:[^@]+@").unwrap(),
            finding_kind: FindingKind::MongodbPassword,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Use environment variables for MongoDB credentials",
        },
        // Redis connection URI with password
        SecretPattern {
            regex: Regex::new(r"redis://:[^@]+@").unwrap(),
            finding_kind: FindingKind::RedisPassword,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Use environment variables for Redis credentials",
        },
        // WireGuard private key (base64, 44 chars including trailing =)
        SecretPattern {
            regex: Regex::new(r"(?i)(?:PrivateKey|PreSharedKey)\s*=\s*[A-Za-z0-9+/]{43}=")
                .unwrap(),
            finding_kind: FindingKind::WireguardKey,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Regenerate WireGuard keys and use wg-quick with externalized key files",
        },
        // WiFi PSK in wpa_supplicant or NetworkManager config
        SecretPattern {
            regex: Regex::new(r"(?i)(?:psk|wpa-psk|wifi\.psk)\s*=\s*\S+").unwrap(),
            finding_kind: FindingKind::WifiPsk,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            remediation: "Use 802.1X or externalize WiFi credentials",
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
            other => panic!("expected EmptyPassword, got {:?}", std::mem::discriminant(&other)),
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
        assert!(PATTERNS.len() >= 9, "expected at least 9 secret patterns");
    }
}
