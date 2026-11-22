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
    /// Whether a match whose secret is a known NSS or PAM word (`files`,
    /// `sss`, `required`) is dropped as a false positive. True for the
    /// generic `password:` patterns, which `nsswitch.conf` and PAM
    /// stacks match by accident; false for a key that only ever holds a
    /// secret, where `files` is as good a password as any.
    pub filter_nss_pam_tokens: bool,
    /// Whether a match on a `.repo` file's `[section]` header whose id is
    /// one dnf accepts is dropped. True only for the generic `key: value`
    /// pattern: a repo id may spell a credential key (a COPR id's
    /// `token:`), and its characters (letters, digits, `-_.:`) can hold no
    /// match of any other pattern but the AWS key id, which stays live.
    pub spares_repo_id_headers: bool,
}

/// Name of the capture group holding the secret itself.
///
/// A pattern that matches context around the secret (a key and its
/// delimiter, a URI's scheme and user, the byte before an AWS key
/// id) puts the secret in `(?P<secret>...)`. The engine replaces only that
/// group, filters on that group's text, and leaves the rest of the match as
/// written. A pattern without the group is replaced whole, which is right
/// only when the whole match is the secret (a PEM block, a hash).
pub(crate) const SECRET_GROUP: &str = "secret";

/// Returns true if `b` can be part of an AWS access key id, so a key id
/// next to it is part of a longer run and not a key id.
fn is_aws_key_byte(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit()
}

/// The key names the generic credential pattern ([`GENERIC_CREDENTIAL`])
/// and the credential name test ([`is_credential_name`]) share, as a regex
/// alternation, so what is detected in a file and what is detected by name
/// on the kernel command line cannot drift apart.
macro_rules! credential_keys {
    () => {
        "password|passwd|db_password|secret|api_key|api_secret|token|credential|private_key|access_key|secret_key|auth_key|encryption_key|signing_key|master_key|service_key|authtok"
    };
}

/// The generic `key=value` credential pattern's source; see its entry in
/// [`PATTERNS`].
const GENERIC_CREDENTIAL: &str = concat!(
    r"(?i)(?:^|\b|_)(?:",
    credential_keys!(),
    r")[ \t]*[=:][ \t]*(?P<secret>\S+)"
);

/// A name that ends in a credential key, at the boundary
/// [`GENERIC_CREDENTIAL`] requires before one.
static CREDENTIAL_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(r"(?i)(?:^|\b|_)(?:", credential_keys!(), r")$"))
        .expect("the credential name pattern compiles")
});

/// Returns true if `name` is one the generic credential pattern treats as
/// a credential key when it comes before an `=`: `password`,
/// `rd.iscsi.password`, `db_password`, `api_key`.
pub(crate) fn is_credential_name(name: &str) -> bool {
    CREDENTIAL_NAME.is_match(name)
}

/// The index in [`PATTERNS`] of the generic credential pattern, whose
/// finding kind, remediation and token a credential found by name take.
pub(crate) static GENERIC_CREDENTIAL_PATTERN: LazyLock<usize> = LazyLock::new(|| {
    PATTERNS
        .iter()
        .position(|pat| pat.regex.as_str() == GENERIC_CREDENTIAL)
        .expect("PATTERNS holds the generic credential pattern")
});

/// All compiled patterns. `LazyLock` ensures they compile exactly once.
pub(crate) static PATTERNS: LazyLock<Vec<SecretPattern>> = LazyLock::new(|| {
    vec![
        // PEM private key blocks (full BEGIN...END matching with dotall).
        // Any label ending in PRIVATE KEY, as the machine-state class's
        // whole-file key rule reads it: `PRIVATE KEY` (PKCS#8),
        // `ENCRYPTED PRIVATE KEY`, `RSA`, `EC`, `DSA`, `OPENSSH`; and
        // OpenPGP secret key armor, `PGP PRIVATE KEY BLOCK` and PGP 2's
        // `PGP SECRET KEY BLOCK`, which that rule reads too.
        SecretPattern {
            regex: Regex::new(
                r"(?s)-----BEGIN\s+(?:(?:[A-Z0-9]+ )*PRIVATE KEY|PGP (?:PRIVATE|SECRET) KEY BLOCK)-----.*?-----END\s+(?:(?:[A-Z0-9]+ )*PRIVATE KEY|PGP (?:PRIVATE|SECRET) KEY BLOCK)-----",
            )
            .unwrap(),
            finding_kind: FindingKind::PrivateKey,
            filter_nss_pam_tokens: false,
            spares_repo_id_headers: false,
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
            filter_nss_pam_tokens: false,
            spares_repo_id_headers: false,
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
            filter_nss_pam_tokens: false,
            spares_repo_id_headers: false,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            accept: None,
            remediation: "Remove password hash or use a secrets manager",
        },
        // Generic password/credential in key=value config.
        // Covers password=, passwd=, db_password=, secret=, api_key=, token=,
        // credential=, private_key=, access_key=, secret_key=, *_key= (compound
        // forms only, since bare "key=" is too broad for config files), and
        // SSSD's *_authtok= bind passwords (whose full value the SSSD
        // pattern below takes).
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
            regex: Regex::new(GENERIC_CREDENTIAL).unwrap(),
            finding_kind: FindingKind::Password,
            filter_nss_pam_tokens: true,
            spares_repo_id_headers: true,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            accept: None,
            remediation:
                "Use environment variables or a secrets manager instead of inline passwords",
        },
        // AWS access key ID (AKIA...). The boundary character before the
        // key is context, not secret: replacing it ate the `=` before the
        // key. The boundary after it is checked by `accept`, not matched:
        // matching it consumed the separator the next key's match starts
        // on, so of `AKIA...\nAKIA...` only the first was found.
        SecretPattern {
            regex: Regex::new(r"(?:^|[^A-Z0-9])(?P<secret>AKIA[0-9A-Z]{16})").unwrap(),
            accept: Some(|content, caps| {
                caps.name(SECRET_GROUP).is_some_and(|key| {
                    !content.as_bytes().get(key.end()).is_some_and(|&b| is_aws_key_byte(b))
                })
            }),
            finding_kind: FindingKind::AwsKey,
            filter_nss_pam_tokens: false,
            spares_repo_id_headers: false,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
                        remediation: "Rotate the AWS access key and use IAM roles",
        },
        // JDBC URL carrying a credential: an `@` (a user and password in
        // the authority) or a non-empty `password=` anywhere after the
        // scheme. The secret is everything after `scheme://` through the
        // last such marker on the line and on to the next whitespace, so the
        // authority, host, database and query string go together. Naming
        // only part of it left the rest to parsing a URL whose password can
        // hold `?`, `@` or nothing before the `:` (`jdbc:mysql://:pw@db`,
        // `jdbc:mysql://u:pa?ss@db`), and each part it got wrong shipped.
        // Only the scheme stays. No class here admits a line break, so a URL
        // with no credential cannot pair with an `@` on a later line; spaces
        // and tabs are allowed, as a host's malformed URL can hold them.
        SecretPattern {
            regex: Regex::new(r"jdbc:[a-z]+://(?P<secret>[^\r\n]*(?:@|password=[^&\s])\S*)").unwrap(),
            finding_kind: FindingKind::JdbcPassword,
            filter_nss_pam_tokens: false,
            spares_repo_id_headers: false,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            accept: None,
            remediation: "Use connection pooling with externalized credentials",
        },
        // PostgreSQL connection URI with password. Only the password is
        // replaced; the scheme, user and `@host` stay. The user and
        // password classes exclude CR and LF: with `[^:]+` / `[^@]+` a
        // password-less URI paired with a `contact: ops@host` line below
        // it. They exclude nothing else, so a password with a space or tab
        // in it (`postgres://u:two words@db`) is still redacted, and so is
        // one with a literal `/`, at the cost of a password-less URI whose
        // query holds an `@`: in `postgres://host:5432/db?u=a@b` the port,
        // path and query up to the `@` are taken as the password. The two
        // read the same.
        SecretPattern {
            regex: Regex::new(r"postgres(?:ql)?://[^:\r\n]+:(?P<secret>[^@\r\n]+)@").unwrap(),
            finding_kind: FindingKind::PostgresPassword,
            filter_nss_pam_tokens: false,
            spares_repo_id_headers: false,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            accept: None,
            remediation: "Use .pgpass or environment variables for PostgreSQL credentials",
        },
        // MongoDB connection URI with password; one line, as Postgres.
        SecretPattern {
            regex: Regex::new(r"mongodb(?:\+srv)?://[^:\r\n]+:(?P<secret>[^@\r\n]+)@").unwrap(),
            finding_kind: FindingKind::MongodbPassword,
            filter_nss_pam_tokens: false,
            spares_repo_id_headers: false,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            accept: None,
            remediation: "Use environment variables for MongoDB credentials",
        },
        // Redis connection URI with password; one line, as Postgres.
        SecretPattern {
            regex: Regex::new(r"redis://:(?P<secret>[^@\r\n]+)@").unwrap(),
            finding_kind: FindingKind::RedisPassword,
            filter_nss_pam_tokens: false,
            spares_repo_id_headers: false,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            accept: None,
            remediation: "Use environment variables for Redis credentials",
        },
        // WireGuard private key (base64, 44 chars including trailing =)
        SecretPattern {
            regex: Regex::new(r"(?i)(?:PrivateKey|PreSharedKey)[ \t]*=[ \t]*(?P<secret>[A-Za-z0-9+/]{43}=)").unwrap(),
            finding_kind: FindingKind::WireguardKey,
            filter_nss_pam_tokens: false,
            spares_repo_id_headers: false,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            accept: None,
            remediation: "Regenerate WireGuard keys and use wg-quick with externalized key files",
        },
        // WiFi PSK in wpa_supplicant or NetworkManager config
        SecretPattern {
            regex: Regex::new(r"(?i)(?:psk|wpa-psk|wifi\.psk)[ \t]*=[ \t]*(?P<secret>\S+)").unwrap(),
            finding_kind: FindingKind::WifiPsk,
            filter_nss_pam_tokens: false,
            spares_repo_id_headers: false,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            accept: None,
            remediation: "Use 802.1X or externalize WiFi credentials",
        },
        // A double-quoted value under a double-quoted credential key, as in
        // JSON's `"password": "x"`, which the generic pattern above does
        // not reach: a quote sits between its key and the `:`. The secret is
        // the whole value, quotes included, through the next whitespace
        // after its closing quote, so it never takes less than `\S+` would
        // from the opening quote, and more when the quoted text holds a
        // space. A backslash keeps the next character inside the quotes; an
        // unterminated value runs to the end of the line. Taking the quotes
        // breaks the JSON, which costs syntax, not a secret: where a value
        // ends is not read off its punctuation until redaction parses the
        // file's format. An empty `""` is left alone, as an empty value is
        // above. This pattern is last, so it changes no other pattern's index
        // or its tie order.
        SecretPattern {
            regex: Regex::new(
                r#"(?i)(?:^|\b|_)(?:password|passwd|db_password|secret|api_key|api_secret|token|credential|private_key|access_key|secret_key|auth_key|encryption_key|signing_key|master_key|service_key|authtok)"[ \t]*:[ \t]*(?P<secret>"(?:[^"\\\r\n]|\\[^\r\n]?)+"?\S*)"#,
            )
            .unwrap(),
            finding_kind: FindingKind::Password,
            filter_nss_pam_tokens: true,
            spares_repo_id_headers: false,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            accept: None,
            remediation:
                "Use environment variables or a secrets manager instead of inline passwords",
        },
        // A double- or single-quoted value under an unquoted credential key,
        // as in YAML's `password: "two words"` or a shell's
        // `DB_PASSWORD='x y'`. The generic pattern's `\S+` stops at the
        // first space inside the quotes and shipped the rest. Same keys and
        // delimiter as the generic pattern; the secret is the whole value,
        // quotes included, through the next whitespace after its closing
        // quote, so it never takes less than `\S+` from the opening quote
        // and overlapping secrets absorb one another. A backslash keeps the
        // next character inside double quotes, and `''` stays inside single
        // quotes, as YAML writes a quote there. An unterminated value runs
        // to the end of the line; an empty `""` or `''` is left alone, and
        // `""hunter2` is left to the generic pattern. Keeping the quotes in
        // the secret means a quoted `files` is not read as the NSS keyword.
        // This pattern is last, so it changes no other pattern's index or
        // its tie order.
        SecretPattern {
            regex: Regex::new(
                r#"(?i)(?:^|\b|_)(?:password|passwd|db_password|secret|api_key|api_secret|token|credential|private_key|access_key|secret_key|auth_key|encryption_key|signing_key|master_key|service_key|authtok)[ \t]*[=:][ \t]*(?P<secret>"(?:[^"\\\r\n]|\\[^\r\n]?)+"?\S*|'(?:[^'\r\n]|'')+'?\S*)"#,
            )
            .unwrap(),
            finding_kind: FindingKind::Password,
            filter_nss_pam_tokens: true,
            spares_repo_id_headers: false,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            accept: None,
            remediation:
                "Use environment variables or a secrets manager instead of inline passwords",
        },
        // SSSD credential settings (`ldap_default_authtok` and any other
        // `*_authtok`). SSSD's INI value is the rest of the line after
        // `=`, spaces, tabs and quotes included, so the secret runs to the
        // last non-blank character, where the generic pattern above stops
        // at the first blank. Anchored at the start of the line, so a
        // commented-out line is not matched, and the key must end in
        // `authtok`, so `ldap_default_authtok_type` is not. Any value is a
        // password here, so the NSS and PAM word filter does not apply.
        SecretPattern {
            regex: Regex::new(
                r"(?im)^[ \t]*[a-z0-9_]*authtok[ \t]*=[ \t]*(?P<secret>[^\r\n]*[^ \t\r\n])",
            )
            .unwrap(),
            finding_kind: FindingKind::Password,
            filter_nss_pam_tokens: false,
            spares_repo_id_headers: false,
            detection_method: DetectionMethod::Pattern,
            confidence: Confidence::High,
            accept: None,
            remediation: "Provision the SSSD bind password on the deployed host instead of carrying it",
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
        // SSSD's LDAP bind password; its `_type` setting names no secret.
        let caps = pat
            .regex
            .captures("ldap_default_authtok = BindPass123")
            .expect("ldap_default_authtok must match");
        assert_eq!(&caps[SECRET_GROUP], "BindPass123");
        assert!(
            !pat.regex.is_match("ldap_default_authtok_type = password"),
            "the authtok type is not a secret"
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
    const JSON_PASSWORD: usize = 11;
    const QUOTED_PASSWORD: usize = 12;
    const SSSD_AUTHTOK: usize = 13;

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
            (JSON_PASSWORD, "\"password\": \"\nnext\"".into()),
            (JSON_PASSWORD, "\"password\":\n\"hunter2\"".into()),
            (QUOTED_PASSWORD, "password: \"\nnext\"".into()),
            (QUOTED_PASSWORD, "password:\n'hunter2'".into()),
            (WIREGUARD_KEY, format!("PrivateKey =\n{wg_key}")),
            (WIREGUARD_KEY, format!("PrivateKey\n= {wg_key}")),
            (SSSD_AUTHTOK, "ldap_default_authtok =\n[domain/b]".into()),
            (
                SSSD_AUTHTOK,
                "ldap_default_authtok = \t \nid_provider = ldap".into(),
            ),
            (SSSD_AUTHTOK, "ldap_default_authtok\n= secret".into()),
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
            (JSON_PASSWORD, "\"password\": \"hunter2".into()),
            (QUOTED_PASSWORD, "password: \"hunter2".into()),
            (WIREGUARD_KEY, format!("PrivateKey = {wg_key}")),
            (SSSD_AUTHTOK, "ldap_default_authtok = secret".into()),
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

    /// The JSON pattern's secret is the whole quoted value and on to the
    /// next whitespace, so it never takes less than `\S+` from the opening
    /// quote. An empty value and an unquoted one are not taken.
    #[test]
    fn test_a_json_credential_value_takes_its_quotes() {
        let cases: &[(&str, Option<&str>)] = &[
            (r#"{"password": "x"}"#, Some(r#""x"}"#)),
            (r#""password": "two words", "#, Some(r#""two words","#)),
            (r#""db_password":"a\"b c""#, Some(r#""a\"b c""#)),
            (r#""token": "\/x"}"#, Some(r#""\/x"}"#)),
            (r#""token": "ab"cd ef"#, Some(r#""ab"cd"#)),
            // Unterminated: to the end of the line.
            ("\"secret\": \"a b\nnext", Some("\"a b")),
            (r#"{"password": ""}"#, None),
            (r#""password": hunter2"#, None),
            (r#""password": 4"#, None),
            (r#""password_hint": "x""#, None),
        ];
        for (input, want) in cases {
            let got = PATTERNS[JSON_PASSWORD]
                .regex
                .captures(input)
                .map(|c| c.name(SECRET_GROUP).unwrap().as_str());
            assert_eq!(got, *want, "{input:?}");
        }
    }

    /// The quoted pattern's secret is the whole quoted value and on to the
    /// next whitespace, so it never takes less than `\S+` from the opening
    /// quote. An empty value, an unquoted one and a quoted key are not
    /// taken.
    #[test]
    fn test_a_quoted_credential_value_takes_its_quotes() {
        let cases: &[(&str, Option<&str>)] = &[
            (r#"password: "two words""#, Some(r#""two words""#)),
            ("password='two words'", Some("'two words'")),
            (r#"secret = "a b c""#, Some(r#""a b c""#)),
            (r#"DB_PASSWORD="x y" tail=1"#, Some(r#""x y""#)),
            (r#"token: "a\" b""#, Some(r#""a\" b""#)),
            ("password: 'it''s a b'", Some("'it''s a b'")),
            (r#"password="ab"cd ef"#, Some(r#""ab"cd"#)),
            ("password: \"a\tb\"", Some("\"a\tb\"")),
            // Unterminated: to the end of the line.
            ("password: \"a b\nnext", Some("\"a b")),
            (r#"password: """#, None),
            ("password: ''", None),
            (r#"DB_PASSWORD=""hunter2"#, None),
            ("password: hunter2", None),
            (r#""password": "x""#, None),
            (r#"password_hint: "x""#, None),
        ];
        for (input, want) in cases {
            let got = PATTERNS[QUOTED_PASSWORD]
                .regex
                .captures(input)
                .map(|c| c.name(SECRET_GROUP).unwrap().as_str());
            assert_eq!(got, *want, "{input:?}");
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
