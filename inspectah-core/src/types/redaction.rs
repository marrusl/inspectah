use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    PrivateKey { format: String },
    Certificate,
    ApiToken { provider: Option<String> },
    Password { context: String },
    ConnectionString,
    ShadowEntry { status: ShadowStatus },
    EnvironmentSecret,
    GenericCredential,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowStatus {
    Locked,
    Disabled,
    NoPassword,
    #[default]
    HasHash,
}

impl ShadowStatus {
    pub fn is_secret(&self) -> bool {
        matches!(self, Self::HasHash | Self::NoPassword)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state")]
pub enum RedactionState {
    #[serde(rename = "fully_redacted")]
    FullyRedacted {
        redacted_by: String,
        config_hash: String,
    },
    #[serde(rename = "partially_redacted")]
    PartiallyRedacted {
        redacted_by: String,
        config_hash: String,
        unresolved_count: u32,
        #[serde(default)]
        unresolved_hints: Vec<RedactionHint>,
    },
    #[serde(rename = "sensitive_retained")]
    SensitiveRetained {
        redacted_by: String,
        config_hash: String,
        unresolved_count: u32,
        #[serde(default)]
        unresolved_hints: Vec<RedactionHint>,
    },
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "raw")]
    Raw,
}

/// Typed redaction classification — strings only at the serde/export edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionKind {
    Excluded,
    Flagged,
    Inline,
}

/// Typed detection method — how the finding was identified.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionMethod {
    #[default]
    Pattern,
    Heuristic,
    PathBased,
}

/// Typed finding classification — what kind of secret was found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    PrivateKey,
    Certificate,
    PasswordHash,
    Password,
    AwsKey,
    JdbcPassword,
    PostgresPassword,
    MongodbPassword,
    RedisPassword,
    WireguardKey,
    WifiPsk,
    ShadowHash,
    NoPassword,
    GenericCredential,
}

/// Confidence level — reused across hints, findings, and detector output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    #[default]
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionHint {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub reason: String,
    pub confidence: Option<Confidence>,
}

/// Redaction finding with typed classification fields.
/// Go-compatible via serde rename_all — strings at the export edge only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RedactionFinding {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub source: String,
    pub kind: RedactionKind,
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub remediation: String,
    pub line: Option<i32>,
    pub replacement: Option<String>,
    pub detection_method: DetectionMethod,
    pub confidence: Option<Confidence>,
    pub finding_kind: Option<FindingKind>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_status_locked_is_not_secret() {
        let status = ShadowStatus::Locked;
        assert!(!status.is_secret());
    }

    #[test]
    fn test_shadow_status_has_hash_is_secret() {
        let status = ShadowStatus::HasHash;
        assert!(status.is_secret());
    }

    #[test]
    fn test_redaction_state_roundtrip() {
        let state = RedactionState::FullyRedacted {
            redacted_by: "inspectah 0.8.0".into(),
            config_hash: "abc123".into(),
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: RedactionState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, parsed);
    }

    #[test]
    fn test_redaction_finding_go_compat() {
        let json = r#"{
            "path": "/etc/shadow",
            "source": "file",
            "kind": "excluded",
            "pattern": "shadow_hash",
            "remediation": "regenerate",
            "detection_method": "pattern"
        }"#;
        let finding: RedactionFinding = serde_json::from_str(json).unwrap();
        assert_eq!(finding.path, "/etc/shadow");
        assert_eq!(finding.kind, RedactionKind::Excluded);
        assert_eq!(finding.detection_method, DetectionMethod::Pattern);
    }

    #[test]
    fn sensitive_retained_roundtrip() {
        let state = RedactionState::SensitiveRetained {
            redacted_by: "inspectah 0.8.0".into(),
            config_hash: "abc123".into(),
            unresolved_count: 2,
            unresolved_hints: vec![],
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: RedactionState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, parsed);
    }

    #[test]
    fn test_finding_kind_serde_roundtrip() {
        assert_eq!(
            serde_json::to_string(&FindingKind::PrivateKey).unwrap(),
            r#""private_key""#
        );
        assert_eq!(
            serde_json::to_string(&FindingKind::ShadowHash).unwrap(),
            r#""shadow_hash""#
        );
        assert_eq!(
            serde_json::to_string(&RedactionKind::Excluded).unwrap(),
            r#""excluded""#
        );
        let parsed: FindingKind = serde_json::from_str(r#""no_password""#).unwrap();
        assert_eq!(parsed, FindingKind::NoPassword);
    }
}
