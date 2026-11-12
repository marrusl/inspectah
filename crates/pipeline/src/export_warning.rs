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

impl ExportWarningKind {
    /// The label the CLI prints before an entry's text: `Failure` or
    /// `Notice`, capitalized like the `Warning:` and `Error:` lines it
    /// prints on the same stream.
    fn label(self) -> &'static str {
        match self {
            Self::Failure => "Failure",
            Self::Notice => "Notice",
        }
    }
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

/// `Notice: <text>` or `Failure: <text>`, as the CLI prints an entry on
/// stderr. The label comes from the kind, never from the text.
impl std::fmt::Display for ExportWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind.label(), self.text)
    }
}

/// Reduce a package id to the bytes a staged filename may carry, mapping
/// everything else to `_`.
///
/// The id comes from a scanned snapshot and the warning built from it ends
/// up in an HTTP response header, where a newline is header injection.
/// Mapping every other byte makes the result printable ASCII by
/// construction, rather than leaving the header boundary to check.
fn sanitize_package_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '~' | '^' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// [`sanitize_package_id`], then cut to at most `max` bytes, ending in
/// `...` when cut, for a snapshot-chosen name bound for the warnings
/// header.
pub fn sanitize_bounded(id: &str, max: usize) -> String {
    const ELLIPSIS: &str = "...";
    let mut id = sanitize_package_id(id);
    if id.len() > max {
        // Sanitized ids are ASCII, so any byte index is a char boundary.
        id.truncate(max - ELLIPSIS.len());
        id.push_str(ELLIPSIS);
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CLI line for each kind: the label is the kind's, whatever the
    /// text says.
    #[test]
    fn display_labels_each_entry_by_its_kind() {
        assert_eq!(ExportWarning::notice("a").to_string(), "Notice: a");
        assert_eq!(
            ExportWarning::failure("Notice: b").to_string(),
            "Failure: Notice: b"
        );
    }

    /// Every byte class the export's warnings can carry.
    ///
    /// The id comes from a scanned snapshot and the warning built from
    /// it ends up in an HTTP response header.
    #[test]
    fn sanitize_package_id_table() {
        let cases: &[(&str, &str)] = &[
            ("epel-release.noarch", "epel-release.noarch"),
            ("libstdc++-devel.x86_64", "libstdc++-devel.x86_64"),
            ("foo~rc1^git1.noarch", "foo~rc1^git1.noarch"),
            // The header-injection shape, and the two bytes that carry it.
            ("evil\r\nX-Evil: 1.noarch", "evil__X-Evil__1.noarch"),
            // DEL is the one `serde_json::to_string` does not escape on
            // the way into the response header, so this row is not
            // redundant with that layer -- it is the only thing keeping
            // a byte `HeaderValue` rejects out of the value.
            ("evil\u{7f}.noarch", "evil_.noarch"),
            // Non-ASCII: serde_json emits these raw, and a header value
            // may not hold them. One `_` per *char*, not per byte:
            // '\u{20ac}' is three bytes and becomes one underscore.
            // `validate_staged_name` next door scans bytes, so the two
            // do not agree on granularity -- which does not matter for
            // either one's purpose, but is worth a reader knowing before
            // they assume it does.
            ("\u{03c0}kg.noarch", "_kg.noarch"),
            ("a\u{20ac}b.noarch", "a_b.noarch"),
            ("evil$(id).noarch", "evil__id_.noarch"),
            ("", ""),
        ];
        for (input, want) in cases {
            assert_eq!(sanitize_package_id(input), *want, "{input:?}");
        }
    }
}
