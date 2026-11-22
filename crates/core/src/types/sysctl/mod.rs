//! The sysctl search path as collected: the `*.conf` files systemd-sysctl
//! reads and the kernel's runtime value of every key they name.
//!
//! The section records what is on disk and what the kernel reports; it
//! owns no decision (no disposition, lock or aggregate). Files reach the
//! image as ordinary config entries. Lines are parsed on demand by the one
//! parser in [`parse`], never stored, so the explanation cannot disagree
//! with the files it explains.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod parse;
pub mod resolve;

pub use parse::{parse_sysctl_lines, sysctl_values_equal};
pub use resolve::{
    RuntimeObservation, SYSCTL_CONF, SYSCTL_CONF_LINK, SysctlCarriage, SysctlExplanation,
    SysctlOutcome, SysctlSetting, UnknownInput, VendorReplacement,
};

/// Where systemd-sysctl reads. Declared highest priority first, so the
/// smallest variant wins a same-name selection: `Etc` replaces a file of
/// the same name in any other directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SysctlDir {
    Etc,
    Run,
    UsrLocalLib,
    UsrLib,
}

impl SysctlDir {
    /// Every directory, highest priority first.
    pub const ALL: [SysctlDir; 4] = [
        SysctlDir::Etc,
        SysctlDir::Run,
        SysctlDir::UsrLocalLib,
        SysctlDir::UsrLib,
    ];

    /// The directory's absolute path on the host.
    pub fn path(self) -> &'static str {
        match self {
            SysctlDir::Etc => "/etc/sysctl.d",
            SysctlDir::Run => "/run/sysctl.d",
            SysctlDir::UsrLocalLib => "/usr/local/lib/sysctl.d",
            SysctlDir::UsrLib => "/usr/lib/sysctl.d",
        }
    }
}

/// What the sysctl search path held at scan time. Collected, not resolved.
/// `KernelBootSection.sysctl` is `None` when collection failed and on an
/// aggregate.
///
/// Deserialization rejects a key present in both `runtime` and
/// `redacted_runtime`, naming it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SysctlSectionWire")]
pub struct SysctlSection {
    /// Every `*.conf` entry in the four directories, sorted by (name, dir).
    pub files: Vec<SysctlFile>,
    /// `sysctl -a` values for every key some assignment or exclusion names,
    /// except those in `redacted_runtime`.
    pub runtime: BTreeMap<SysctlKey, String>,
    /// Keys whose runtime value the redaction pass removed, sorted.
    /// A key in neither field was not in the kernel's output at scan time.
    pub redacted_runtime: Vec<SysctlKey>,
}

/// The unchecked wire form of [`SysctlSection`].
#[derive(Deserialize)]
struct SysctlSectionWire {
    #[serde(default)]
    files: Vec<SysctlFile>,
    #[serde(default)]
    runtime: BTreeMap<SysctlKey, String>,
    #[serde(default)]
    redacted_runtime: Vec<SysctlKey>,
}

/// A sysctl section on the wire that cannot be a collected one.
#[derive(Debug, thiserror::Error)]
pub enum SysctlSectionError {
    /// A key has both a runtime value and a record that redaction removed
    /// it; the collector writes exactly one of the two.
    #[error(
        "sysctl key `{0}` is in both `runtime` and `redacted_runtime`; a snapshot records it in one"
    )]
    KeyInBothRuntimeFields(String),
}

impl TryFrom<SysctlSectionWire> for SysctlSection {
    type Error = SysctlSectionError;

    fn try_from(wire: SysctlSectionWire) -> Result<Self, Self::Error> {
        if let Some(key) = wire
            .redacted_runtime
            .iter()
            .find(|key| wire.runtime.contains_key(*key))
        {
            return Err(SysctlSectionError::KeyInBothRuntimeFields(
                key.as_str().to_string(),
            ));
        }
        Ok(SysctlSection {
            files: wire.files,
            runtime: wire.runtime,
            redacted_runtime: wire.redacted_runtime,
        })
    }
}

/// One `*.conf` entry in a sysctl directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SysctlFile {
    pub dir: SysctlDir,
    /// Basename: the sort key and the shadow key.
    pub name: String,
    pub body: SysctlFileBody,
}

impl SysctlFile {
    /// The entry's path on the host: its directory joined with its name.
    pub fn path(&self) -> String {
        self.file_ref().path()
    }

    /// A reference to this file by directory and name.
    pub fn file_ref(&self) -> SysctlFileRef {
        SysctlFileRef {
            dir: self.dir,
            name: self.name.clone(),
        }
    }

    /// The file's lines, parsed by the one parser. Derived, never stored.
    /// A mask or an outside link has no lines inspectah read.
    pub fn lines(&self) -> Vec<SysctlLine> {
        match &self.body {
            SysctlFileBody::Read { content, .. } => parse_sysctl_lines(content),
            SysctlFileBody::Mask
            | SysctlFileBody::OutsideLink { .. }
            | SysctlFileBody::Withheld { .. } => Vec::new(),
        }
    }
}

/// What a sysctl entry is, and its content when it was read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SysctlFileBody {
    /// A symlink whose final target is `/dev/null`.
    Mask,
    /// A symlink whose final target is outside `/etc` and the four
    /// directories. Not read; still shadows by name.
    OutsideLink { target: String },
    /// A file the read gate refused as a machine-state member. Not read;
    /// still shadows by name, as an outside link does.
    Withheld {
        member: crate::types::machine_state::MachineStateMember,
    },
    /// A file that was read. `link_target` is the final target when the
    /// entry is a symlink (`99-sysctl.conf` -> `/etc/sysctl.conf`).
    /// `redacted` is set when the redaction pass replaced anything in
    /// `content`; false until redaction runs.
    Read {
        link_target: Option<String>,
        content: String,
        #[serde(default)]
        redacted: bool,
    },
}

/// A sysctl file named by directory and basename.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SysctlFileRef {
    pub dir: SysctlDir,
    pub name: String,
}

impl SysctlFileRef {
    /// The file's path on the host: its directory joined with its name.
    pub fn path(&self) -> String {
        format!("{}/{}", self.dir.path(), self.name)
    }
}

/// One line of a sysctl file, by directory, basename and 1-based line
/// number.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SysctlLineRef {
    pub dir: SysctlDir,
    pub name: String,
    pub line_no: u32,
}

/// One parsed line of a sysctl file. Line numbers are 1-based physical
/// lines; skipped lines (blank, `#`, `;`) produce nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SysctlLine {
    /// `name = value`; a leading `-` sets `ignore_failure`.
    Assign {
        line_no: u32,
        name: SysctlName,
        value: String,
        ignore_failure: bool,
    },
    /// `-name` with no `=`: an exclusion.
    Exclude { line_no: u32, name: SysctlName },
    /// Any other line, kept so the report can show it.
    Unparsed { line_no: u32, text: String },
}

/// The name on an assignment or exclusion line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SysctlName {
    Key(SysctlKey),
    /// Holds `*`, `?` or `[`. Reported as written; not expanded.
    Glob(String),
}

/// A key in the dotted form `sysctl -a` prints. Built only by the
/// normalizing constructor and by the checked deserializer, so two
/// spellings of one key cannot both exist.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SysctlKey(String);

impl SysctlKey {
    /// Normalizing constructor: the only way to build a key from text.
    ///
    /// systemd's `sysctl_normalize` (`src/basic/sysctl-util.c`, v252 and
    /// v257), then the dotted rendering `sysctl -a` prints:
    ///
    /// 1. `sysctl.d(5)` accepts `.` or `/` as the separator and decides by
    ///    the first one: after a `/` the name is already a `/proc/sys`
    ///    path, after a `.` dots and slashes are interchanged to get one.
    /// 2. The path is simplified as `path_simplify` does: repeated
    ///    separators collapse, `.` components and a trailing separator go.
    /// 3. A leading separator goes, unless it is the whole path.
    /// 4. The path is rendered dotted by interchanging dots and slashes
    ///    again, so a literal `.` in a component becomes `/`.
    ///
    /// So `-/vm/swappiness` excludes `vm.swappiness`, as systemd reads it.
    pub fn normalize(name: &str) -> SysctlKey {
        let first_separator = name.chars().find(|c| matches!(c, '.' | '/'));
        let path = if first_separator == Some('.') {
            swap_separators(name)
        } else {
            name.to_string()
        };
        let simplified = simplify_path(&path);
        let relative = match simplified.strip_prefix('/') {
            Some(rest) if !rest.is_empty() => rest,
            _ => simplified.as_str(),
        };
        SysctlKey(swap_separators(relative))
    }

    /// The key in dotted form.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Interchange `.` and `/`: between a `/proc/sys` path and its dotted
/// rendering, either way.
fn swap_separators(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '.' => '/',
            '/' => '.',
            other => other,
        })
        .collect()
}

/// systemd's `path_simplify` (`src/basic/path-util.c`): drop empty and
/// `.` components, keep one leading `/` on an absolute path, and give `.`
/// for a relative path left with no component. `..` is kept, as systemd
/// keeps it. systemd stops simplifying at a component over `NAME_MAX`
/// bytes; no `/proc/sys` name is that long, so that case is not followed.
fn simplify_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let joined = path
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/");
    if path.starts_with('/') {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

/// A string on the wire that is not a key's canonical spelling.
#[derive(Debug, thiserror::Error)]
#[error(
    "sysctl key `{key}` is not in the dotted form `sysctl -a` prints; it normalizes to `{canonical}`"
)]
pub struct SysctlKeyError {
    /// The string as found on the wire.
    pub key: String,
    /// What the string normalizes to.
    pub canonical: String,
}

impl TryFrom<String> for SysctlKey {
    type Error = SysctlKeyError;

    /// Accepts only canonical spellings: a string that normalizing would
    /// change is another spelling of a key, and accepting it would let the
    /// runtime map hold one key twice.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let normalized = SysctlKey::normalize(&value);
        if normalized.0 == value {
            Ok(normalized)
        } else {
            Err(SysctlKeyError {
                key: value,
                canonical: normalized.0,
            })
        }
    }
}

impl From<SysctlKey> for String {
    fn from(key: SysctlKey) -> String {
        key.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sysctl_file_body_serde() {
        let cases = [
            (SysctlFileBody::Mask, r#"{"kind":"mask"}"#),
            (
                SysctlFileBody::OutsideLink {
                    target: "/opt/tuning/sysctl.conf".into(),
                },
                r#"{"kind":"outside_link","target":"/opt/tuning/sysctl.conf"}"#,
            ),
            (
                SysctlFileBody::Read {
                    link_target: Some("/etc/sysctl.conf".into()),
                    content: "vm.swappiness = 10\n".into(),
                    redacted: true,
                },
                r#"{"kind":"read","link_target":"/etc/sysctl.conf","content":"vm.swappiness = 10\n","redacted":true}"#,
            ),
        ];
        for (body, json) in cases {
            assert_eq!(serde_json::to_string(&body).unwrap(), json);
            let back: SysctlFileBody = serde_json::from_str(json).unwrap();
            assert_eq!(back, body);
        }

        let without_redacted: SysctlFileBody =
            serde_json::from_str(r#"{"kind":"read","link_target":null,"content":""}"#).unwrap();
        assert_eq!(
            without_redacted,
            SysctlFileBody::Read {
                link_target: None,
                content: String::new(),
                redacted: false,
            }
        );
    }
}
