use inspectah_core::types::redaction::{
    Confidence, DetectionMethod, FindingKind, RedactionFinding, RedactionKind,
};
use regex::{Captures, Regex};
use std::sync::LazyLock;

/// A compiled pattern with its metadata for secret detection.
pub(crate) struct SecretPattern {
    pub regex: Regex,
    pub finding_kind: FindingKind,
    pub detection_method: DetectionMethod,
    pub confidence: Confidence,
    /// Human-readable remediation advice.
    pub remediation: &'static str,
    /// A condition on a match that the regex cannot state, checked against
    /// the content the match came from. A match it refuses is not a
    /// finding. `None` accepts every match.
    pub accept: Option<fn(&str, &Captures<'_>) -> bool>,
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

/// One of the JDBC URL patterns in [`PATTERNS`]. They differ only in what
/// opens the URL, which decides where it ends.
fn jdbc_pattern(regex: &str, accept: Option<fn(&str, &Captures<'_>) -> bool>) -> SecretPattern {
    SecretPattern {
        regex: Regex::new(regex).unwrap(),
        finding_kind: FindingKind::JdbcPassword,
        detection_method: DetectionMethod::Pattern,
        confidence: Confidence::High,
        remediation: "Use connection pooling with externalized credentials",
        accept,
    }
}

/// Returns true if the double-quoted JDBC URL in `caps` carries a
/// credential: an `@`, or a `password=` with a value, anywhere between
/// its quotes once backslashes are set aside. The regex finds where the
/// quoted value ends, honouring `\"`; this decides whether it holds a
/// credential. A backslash is an escape in JSON and TOML but not in an XML
/// attribute, so `\@` may be a backslash and an `@`, and either way the
/// `@` is there.
fn quoted_jdbc_url_carries_a_credential(_content: &str, caps: &Captures<'_>) -> bool {
    let Some(url) = caps.name(SECRET_GROUP) else {
        return false;
    };
    let plain: String = url.as_str().chars().filter(|&c| c != '\\').collect();
    plain.contains('@')
        || plain.match_indices(JDBC_PASSWORD_MARKER).any(|(i, m)| {
            plain[i + m.len()..]
                .chars()
                .next()
                .is_some_and(|c| c != '&' && !c.is_whitespace())
        })
}

/// The query-string key that marks a JDBC URL as carrying a credential.
const JDBC_PASSWORD_MARKER: &str = "password=";

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
            accept: None,
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
            accept: None,
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
            accept: None,
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
            accept: None,
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
            accept: None,
            remediation: "Rotate the AWS access key and use IAM roles",
        },
        // JDBC URL carrying a credential: an `@` (a user and password in
        // the authority) or a non-empty `password=` anywhere after the
        // scheme. Only the scheme stays: the secret is the rest of the URL,
        // authority, host, database and query string together. Naming only
        // part of it left the rest to parsing a URL whose password can hold
        // `?`, `@` or nothing before the `:` (`jdbc:mysql://:pw@db`,
        // `jdbc:mysql://u:pa?ss@db`), and each part it got wrong shipped.
        //
        // Where the URL ends depends on what comes right before `jdbc:`, so
        // there is one pattern per opener, and they exclude one another, so
        // only one can claim a URL:
        //
        // - after `"` or `'`, the quoted scalar, up to its closing quote or
        //   the end of the line; a backslash escape, or `''` inside single
        //   quotes, does not close it. After `>`, XML element text, up to
        //   the next `<`. The marker must sit inside, and the closing quote
        //   or tag stays as written. Running on to the next whitespace
        //   instead took `"` and `</url>` with the URL. In double quotes
        //   the regex only finds where the value ends, and
        //   `quoted_jdbc_url_carries_a_credential` looks for the marker
        //   with backslashes set aside: in an XML attribute `\@` is a
        //   backslash and an `@`, and reading it as one escape hid the `@`.
        // - after anything else, the line's last marker and on to the next
        //   whitespace. This reach crosses whitespace on purpose, for an
        //   authority password holding a space (`jdbc:mysql://u:two
        //   words@db`), so it also takes a later `password=` or `@` on the
        //   line and what lies between: `url=jdbc:mysql://u:pw@db pool=4
        //   password=x` loses `pool=4`. Stopping before ` key=` would keep
        //   it and ship the end of a password such as `p@ss w=x`, and the
        //   two cannot be told apart. This pattern recognises its case by
        //   consuming the character before `jdbc:`, so its match starts
        //   there: a URL right after a comment marker (`#jdbc:`) is
        //   redacted rather than exempt.
        //
        // A quoted URL with no marker inside its quotes matches none of
        // them, so a `password="..."` after it is the generic pattern's and
        // does not pull the URL's closing quote into this one. No class
        // here admits a line break, so a URL with no credential cannot pair
        // with an `@` on a later line; spaces and tabs are allowed, as a
        // host's malformed URL can hold them.
        jdbc_pattern(
            r#"(?:(?m:^)|[^"'>\r\n])jdbc:[a-z]+://(?P<secret>[^\r\n]*(?:@|password=[^&\s])\S*)"#,
            None,
        ),
        jdbc_pattern(
            r#""jdbc:[a-z]+://(?P<secret>(?:[^"\\\r\n]|\\[^\r\n]?)+)"#,
            Some(quoted_jdbc_url_carries_a_credential),
        ),
        jdbc_pattern(
            r#"'jdbc:[a-z]+://(?P<secret>(?:[^'\r\n]|'')*(?:@|password=[^&\s'])(?:[^'\r\n]|'')*)"#,
            None,
        ),
        jdbc_pattern(
            r#">jdbc:[a-z]+://(?P<secret>[^<\r\n]*(?:@|password=[^&\s<])[^<\r\n]*)"#,
            None,
        ),
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
            accept: None,
            remediation: "Use .pgpass or environment variables for PostgreSQL credentials",
        },
        // MongoDB connection URI with password; one line, as Postgres.
        SecretPattern {
            regex: Regex::new(r"mongodb(?:\+srv)?://[^:\r\n]+:(?P<secret>[^@\r\n]+)@").unwrap(),
            finding_kind: FindingKind::MongodbPassword,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            accept: None,
            remediation: "Use environment variables for MongoDB credentials",
        },
        // Redis connection URI with password; one line, as Postgres.
        SecretPattern {
            regex: Regex::new(r"redis://:(?P<secret>[^@\r\n]+)@").unwrap(),
            finding_kind: FindingKind::RedisPassword,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            accept: None,
            remediation: "Use environment variables for Redis credentials",
        },
        // WireGuard private key (base64, 44 chars including trailing =)
        SecretPattern {
            regex: Regex::new(r"(?i)(?:PrivateKey|PreSharedKey)[ \t]*=[ \t]*(?P<secret>[A-Za-z0-9+/]{43}=)").unwrap(),
            finding_kind: FindingKind::WireguardKey,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            accept: None,
            remediation: "Regenerate WireGuard keys and use wg-quick with externalized key files",
        },
        // WiFi PSK in wpa_supplicant or NetworkManager config
        SecretPattern {
            regex: Regex::new(r"(?i)(?:psk|wpa-psk|wifi\.psk)[ \t]*=[ \t]*(?P<secret>\S+)").unwrap(),
            finding_kind: FindingKind::WifiPsk,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            accept: None,
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
    const JDBC_DOUBLE_QUOTED: usize = 6;
    const JDBC_SINGLE_QUOTED: usize = 7;
    const JDBC_XML_TEXT: usize = 8;
    const POSTGRES: usize = 9;
    const MONGODB: usize = 10;
    const REDIS: usize = 11;
    const WIREGUARD_KEY: usize = 12;
    const WIFI_PSK: usize = 13;

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
                (JDBC, format!("jdbc:mysql://app:{nl}pw@db")),
                (
                    JDBC,
                    format!("url=jdbc:mysql://db/app{nl}contact: ops@example.com"),
                ),
                (
                    JDBC,
                    format!(
                        "primary=jdbc:mysql://db/one{nl}secondary=jdbc:mysql://db/two?password=hunter2"
                    ),
                ),
                (
                    JDBC_DOUBLE_QUOTED,
                    format!("url=\"jdbc:mysql://db/app{nl}contact: ops@example.com\""),
                ),
                (
                    JDBC_SINGLE_QUOTED,
                    format!("url='jdbc:mysql://db/app{nl}contact: ops@example.com'"),
                ),
                (
                    JDBC_XML_TEXT,
                    format!("<url>jdbc:mysql://db/app{nl}contact: ops@example.com</url>"),
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
                "db:3306/app?user=u&password=pw",
            ),
            (
                JDBC,
                "jdbc:postgresql://db/app?ssl=true&password=pw\r",
                "db/app?ssl=true&password=pw",
            ),
            (
                JDBC,
                "jdbc:mysql://app:pw@db:3306/app",
                "app:pw@db:3306/app",
            ),
            // A space or tab is not a line break.
            (POSTGRES, "postgres://app:two words@db", "two words"),
            (POSTGRES, "postgresql://app:tab\tbed@db", "tab\tbed"),
            (MONGODB, "mongodb://app:two words@db", "two words"),
            (MONGODB, "mongodb+srv://app:tab\tbed@db", "tab\tbed"),
            (REDIS, "redis://:two words@cache", "two words"),
            (REDIS, "redis://:tab\tbed@cache", "tab\tbed"),
            (JDBC, "jdbc:mysql://app:two words@db", "app:two words@db"),
            (
                JDBC,
                "jdbc:mysql://db/my app?password=pw",
                "db/my app?password=pw",
            ),
        ];
        for (idx, text, secret) in same_line {
            let caps = PATTERNS[idx]
                .regex
                .captures(text)
                .unwrap_or_else(|| panic!("pattern {idx} must match {text:?}"));
            assert_eq!(&caps[SECRET_GROUP], secret, "pattern {idx} on {text:?}");
        }
    }

    /// What opens a JDBC URL decides where it ends, and exactly one of the
    /// four JDBC patterns claims it, counting a pattern's `accept` check. A quoted URL or XML text leaves its
    /// closing quote or tag outside the secret, and the unquoted pattern
    /// never takes a URL that an opener's pattern declined.
    #[test]
    fn test_one_jdbc_pattern_claims_a_url_by_what_opens_it() {
        let jdbc = [JDBC, JDBC_DOUBLE_QUOTED, JDBC_SINGLE_QUOTED, JDBC_XML_TEXT];
        let cases: &[(&str, Option<(usize, &str)>)] = &[
            (
                "url=jdbc:mysql://u:pw@db/app pool=4",
                Some((JDBC, "u:pw@db/app")),
            ),
            ("jdbc:mysql://u:pw@db/app", Some((JDBC, "u:pw@db/app"))),
            ("a\njdbc:mysql://u:pw@db/app", Some((JDBC, "u:pw@db/app"))),
            (
                r#"url="jdbc:mysql://u:pw@db/app" pool=4"#,
                Some((JDBC_DOUBLE_QUOTED, "u:pw@db/app")),
            ),
            (
                r#"url="jdbc:mysql://app:two words@db" pool=4"#,
                Some((JDBC_DOUBLE_QUOTED, "app:two words@db")),
            ),
            // A backslash escape does not close a double-quoted value.
            (
                r#"{"url": "jdbc:mysql://u:p\"w@db/app"}"#,
                Some((JDBC_DOUBLE_QUOTED, r#"u:p\"w@db/app"#)),
            ),
            (
                "url='jdbc:mysql://u:pw@db/app' pool=4",
                Some((JDBC_SINGLE_QUOTED, "u:pw@db/app")),
            ),
            // Nor does YAML's doubled quote close a single-quoted one.
            (
                "url: 'jdbc:mysql://u:it''s@db/app'",
                Some((JDBC_SINGLE_QUOTED, "u:it''s@db/app")),
            ),
            (
                "<url>jdbc:mysql://u:pw@db/app</url>",
                Some((JDBC_XML_TEXT, "u:pw@db/app")),
            ),
            (
                "<url>jdbc:mysql://db/app?password=two words</url>",
                Some((JDBC_XML_TEXT, "db/app?password=two words")),
            ),
            // A backslash before a marker does not hide it: in an XML
            // attribute it is no escape at all.
            (
                r#"<db url="jdbc:mysql://u:pw\@db/app"/>"#,
                Some((JDBC_DOUBLE_QUOTED, r"u:pw\@db/app")),
            ),
            // An unterminated quote runs to the end of the line.
            (
                r#"url="jdbc:mysql://u:pw@db/app pool=4"#,
                Some((JDBC_DOUBLE_QUOTED, "u:pw@db/app pool=4")),
            ),
            // No marker inside the quotes or the element: no JDBC pattern
            // matches, so none reaches the `@` after the closing quote.
            (
                r#"<db url="jdbc:mysql://db/app" owner="ops@example.com"/>"#,
                None,
            ),
            (
                "<url>jdbc:mysql://db/app</url><owner>ops@example.com</owner>",
                None,
            ),
        ];
        for (input, want) in cases {
            let got: Vec<(usize, &str)> = jdbc
                .iter()
                .flat_map(|&idx| {
                    let pat = &PATTERNS[idx];
                    pat.regex
                        .captures_iter(input)
                        .filter(move |c| pat.accept.is_none_or(|accept| accept(input, c)))
                        .map(move |c| (idx, c.name(SECRET_GROUP).unwrap().as_str()))
                })
                .collect();
            assert_eq!(got, want.iter().copied().collect::<Vec<_>>(), "{input:?}");
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
