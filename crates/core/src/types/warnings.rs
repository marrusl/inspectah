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

/// The `extra` key holding a warning's stable machine code, for warnings
/// a consumer must find again. The message is prose for people and may be
/// reworded; the code is what consumers match on.
pub const WARNING_CODE_KEY: &str = "code";

impl Warning {
    /// This warning's stable machine code, if it carries one.
    pub fn code(&self) -> Option<&str> {
        self.extra.get(WARNING_CODE_KEY)?.as_str()
    }
}
