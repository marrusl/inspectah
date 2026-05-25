use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Typed warning severity — not a freeform string.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningSeverity {
    Info,
    #[default]
    Warning,
    Error,
}

/// Typed warning with extra field support.
/// The flatten catches unknown keys for forward compatibility.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Warning {
    #[serde(default)]
    pub inspector: String,
    #[serde(default)]
    pub message: String,
    pub severity: Option<WarningSeverity>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
