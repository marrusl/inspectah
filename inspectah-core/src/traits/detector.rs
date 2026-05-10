use crate::types::redaction::RedactionHint;

/// Typed detector identity — compiler-enforced, not a freeform string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetectorId {
    PrivateKey,
    Certificate,
    Password,
    ApiToken,
    ShadowEntry,
    ConnectionString,
    EnvironmentSecret,
    WireguardKey,
    WifiPsk,
}

pub trait SecretDetector: Send + Sync {
    fn id(&self) -> DetectorId;
    fn sensitivity(&self) -> Sensitivity;
    fn scan(&self, content: &str, context: &ScanContext) -> Vec<Finding>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensitivity {
    Default,
    Strict,
}

#[derive(Debug, Clone)]
pub struct ScanContext {
    pub path: String,
    pub source: String,
}

/// Typed finding — kind and confidence are enums, not strings.
#[derive(Debug, Clone)]
pub struct Finding {
    pub line: usize,
    pub kind: crate::types::redaction::FindingKind,
    pub confidence: crate::types::redaction::Confidence,
    pub hint: RedactionHint,
}

// Note: Confidence and FindingKind are defined in types::redaction
// and reused here — single source of truth, no duplicate enums.
