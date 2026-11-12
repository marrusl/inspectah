//! What an export tells the operator beyond the tarball itself.
//!
//! Every entry carries a [`ExportWarningKind`], so a consumer never infers
//! severity from the wording. The fields are private, and producers build
//! entries through the two constructors, [`ExportWarning::failure`] and
//! [`ExportWarning::notice`], each of which names the kind. `Deserialize`
//! is derived so a test can parse the header back, which is the one other
//! way to make an entry.
//!
//! The web route serializes the list into the `x-export-warnings` response
//! header as `[{"kind":"failure","text":"..."}, ...]`.

use serde::{Deserialize, Serialize};

/// How serious an export warning is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportWarningKind {
    /// Something the operator asked for is not in the export: a staged RPM
    /// that could not be verified, for example.
    Failure,
    /// The export is what the operator asked for, and this is worth
    /// knowing about it: packages it leaves out by default, for example.
    Notice,
}

/// One entry of an export's warnings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportWarning {
    kind: ExportWarningKind,
    text: String,
}

impl ExportWarning {
    /// Something the operator asked for is missing from the export.
    pub fn failure(text: impl Into<String>) -> Self {
        Self {
            kind: ExportWarningKind::Failure,
            text: text.into(),
        }
    }

    /// Something worth knowing about an export that is otherwise as asked.
    pub fn notice(text: impl Into<String>) -> Self {
        Self {
            kind: ExportWarningKind::Notice,
            text: text.into(),
        }
    }

    /// This entry's kind.
    pub fn kind(&self) -> ExportWarningKind {
        self.kind
    }

    /// This entry's text, for display.
    pub fn text(&self) -> &str {
        &self.text
    }
}
