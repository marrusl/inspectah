use super::FindingKind;
use super::aggregate::AggregatePrevalence;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FstabEntry {
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub mount_point: String,
    #[serde(default)]
    pub fstype: String,
    #[serde(default)]
    pub options: String,
    #[serde(default)]
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<AggregatePrevalence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRef {
    #[serde(default)]
    pub mount_point: String,
    #[serde(default)]
    pub credential_path: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountPoint {
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub fstype: String,
    #[serde(default)]
    pub options: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LvmVolume {
    #[serde(default)]
    pub lv_name: String,
    #[serde(default)]
    pub vg_name: String,
    #[serde(default)]
    pub lv_size: String,
}

/// The tmpfiles.d directories a [`VarDirBacking::Tmpfiles`] `conf` is
/// under. Which one it is decides whether the export can carry the
/// fragment, so the prefixes are defined once and matched everywhere.
pub const ETC_TMPFILES_PREFIX: &str = "/etc/tmpfiles.d/";
pub const RUN_TMPFILES_PREFIX: &str = "/run/tmpfiles.d/";
pub const USR_TMPFILES_PREFIX: &str = "/usr/lib/tmpfiles.d/";

/// The leading fields of one tmpfiles.d entry, split and unquoted as
/// `systemd-tmpfiles` reads them. Fields past the group are not kept.
///
/// The export's account checks read lines through it. The collector's
/// `/var` backing index does not: it still matches each line's raw,
/// whitespace-split path, so decoding quoted paths changes what the
/// export names and not what it provisions. systemd reads the type, path, mode, user, group and age fields with
/// `extract_many_words(..., EXTRACT_UNQUOTE | EXTRACT_CUNESCAPE, ...)`:
/// whitespace separates fields, a `"` or `'` quoted run may hold
/// whitespace, and a backslash starts a C escape inside or outside quotes.
/// A line systemd would reject for an unterminated quote or a bad escape
/// in those fields is no entry here either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmpfilesEntry {
    /// The first character of the type field. The `+ ! - = ~ ^`
    /// modifiers that may follow it do not change which kind it is.
    pub entry_type: char,
    pub path: String,
    pub mode: Option<String>,
    pub user: Option<String>,
    pub group: Option<String>,
}

impl TmpfilesEntry {
    /// Parse one line, or `None` for a blank line, a comment, a line with
    /// no path field, and a line whose leading fields systemd cannot
    /// split.
    pub fn parse(line: &str) -> Option<Self> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let mut rest = line;
        let entry_type = next_tmpfiles_field(&mut rest).ok()??.chars().next()?;
        let path = next_tmpfiles_field(&mut rest).ok()??;
        let mode = next_tmpfiles_field(&mut rest).ok()?;
        let user = next_tmpfiles_field(&mut rest).ok()?;
        let group = next_tmpfiles_field(&mut rest).ok()?;
        // The age field is split the same way, so a bad quote there
        // rejects the line too. Its value is not kept.
        next_tmpfiles_field(&mut rest).ok()?;
        Some(Self {
            entry_type,
            path,
            mode,
            user,
            group,
        })
    }
}

/// A tmpfiles.d field systemd cannot split: an unterminated quote, a
/// trailing backslash, or an escape `cunescape` rejects.
#[derive(Debug)]
struct TmpfilesSplitError;

/// The next field of `rest`, unquoted and unescaped, advancing `rest`
/// past it; `Ok(None)` when no field is left. Mirrors systemd's
/// `extract_first_word` with `EXTRACT_UNQUOTE | EXTRACT_CUNESCAPE`.
fn next_tmpfiles_field(rest: &mut &str) -> Result<Option<String>, TmpfilesSplitError> {
    let input = rest.trim_start();
    if input.is_empty() {
        *rest = input;
        return Ok(None);
    }
    let mut out: Vec<u8> = Vec::new();
    let mut quote: Option<char> = None;
    let mut chars = input.char_indices();
    let mut end = input.len();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                let (_, e) = chars.next().ok_or(TmpfilesSplitError)?;
                unescape_one(e, &mut chars, &mut out)?;
            }
            q if quote == Some(q) => quote = None,
            '"' | '\'' if quote.is_none() => quote = Some(c),
            w if quote.is_none() && w.is_whitespace() => {
                end = i;
                break;
            }
            other => {
                let mut buf = [0; 4];
                out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    if quote.is_some() {
        return Err(TmpfilesSplitError);
    }
    *rest = &input[end..];
    String::from_utf8(out)
        .map(Some)
        .map_err(|_| TmpfilesSplitError)
}

/// The noncharacters systemd's `unichar_is_valid` rejects beyond what a
/// `char` already excludes (surrogates and code points past U+10FFFF):
/// U+FDD0..U+FDEF, and U+xFFFE and U+xFFFF in every plane.
fn is_unicode_noncharacter(c: char) -> bool {
    let v = u32::from(c);
    (0xFDD0..=0xFDEF).contains(&v) || v & 0xFFFE == 0xFFFE
}

/// Append the character systemd's `cunescape_one` decodes for the escape
/// starting with `e` (the character after the backslash), reading any
/// further digits from `chars`. A NUL is refused, as systemd refuses it.
fn unescape_one(
    e: char,
    chars: &mut std::str::CharIndices<'_>,
    out: &mut Vec<u8>,
) -> Result<(), TmpfilesSplitError> {
    let mut digits = |n: usize, radix: u32| -> Result<u32, TmpfilesSplitError> {
        (0..n).try_fold(0u32, |acc, _| {
            let d = chars
                .next()
                .and_then(|(_, c)| c.to_digit(radix))
                .ok_or(TmpfilesSplitError)?;
            Ok(acc * radix + d)
        })
    };
    let byte = |v: u32| {
        u8::try_from(v)
            .ok()
            .filter(|b| *b != 0)
            .ok_or(TmpfilesSplitError)
    };
    let simple = match e {
        'a' => Some('\u{7}'),
        'b' => Some('\u{8}'),
        'f' => Some('\u{c}'),
        'n' => Some('\n'),
        'r' => Some('\r'),
        't' => Some('\t'),
        'v' => Some('\u{b}'),
        's' => Some(' '),
        '\\' | '"' | '\'' => Some(e),
        _ => None,
    };
    if let Some(c) = simple {
        let mut buf = [0; 4];
        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        return Ok(());
    }
    match e {
        // A byte, which may be one of several in a UTF-8 sequence.
        'x' => out.push(byte(digits(2, 16)?)?),
        '0'..='7' => {
            let first = e.to_digit(8).ok_or(TmpfilesSplitError)?;
            let rest = digits(2, 8)?;
            out.push(byte(first * 64 + rest)?);
        }
        // `\u` refuses only NUL; `\U` also refuses what systemd's
        // `unichar_is_valid` rejects.
        'u' | 'U' => {
            let n = if e == 'u' { 4 } else { 8 };
            let c = char::from_u32(digits(n, 16)?)
                .filter(|c| *c != '\0')
                .filter(|c| e == 'u' || !is_unicode_noncharacter(*c))
                .ok_or(TmpfilesSplitError)?;
            let mut buf = [0; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
        _ => return Err(TmpfilesSplitError),
    }
    Ok(())
}

/// What created a `/var` directory on the source host.
///
/// Each backed variant carries the provenance the export needs to decide
/// whether that creator reaches the image. A verdict is an observation
/// about the source host, never an instruction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VarDirBacking {
    /// `conf` is the tmpfiles.d fragment (under `/etc`, `/run`, or
    /// `/usr/lib`) and `line` the entry, as written, whose type creates
    /// exactly this directory.
    Tmpfiles {
        conf: String,
        line: String,
    },
    /// `declared_by` holds the full path of every unit file carrying a
    /// `StateDirectory=` for this directory, in search order.
    StateDirectory {
        declared_by: Vec<String>,
    },
    /// `declared_by` holds the full path of every unit file carrying a
    /// `CacheDirectory=` for this directory, in search order.
    CacheDirectory {
        declared_by: Vec<String>,
    },
    /// `declared_by` holds the full path of every unit file carrying a
    /// `LogsDirectory=` for this directory, in search order.
    LogsDirectory {
        declared_by: Vec<String>,
    },
    /// `package` is the bare RPM name owning the directory, matching the
    /// identity `rpm.file_ownership` and `rpm.baseline_package_names` use.
    RpmOwned {
        package: String,
    },
    Unbacked,
}

impl VarDirBacking {
    /// Whether this verdict names a creator an export can carry into the
    /// image: an `/etc` fragment, a `/usr/lib` fragment, a unit, or a
    /// package.
    ///
    /// A `Tmpfiles` verdict whose `conf` is under `/run/tmpfiles.d/` is
    /// not one: the fragment is generated at runtime and reaches no
    /// image, so a `/run` verdict proves nothing about what the image
    /// creates.
    pub fn names_a_carryable_creator(&self) -> bool {
        match self {
            Self::Tmpfiles { conf, .. } => !conf.starts_with(RUN_TMPFILES_PREFIX),
            Self::StateDirectory { declared_by }
            | Self::CacheDirectory { declared_by }
            | Self::LogsDirectory { declared_by } => !declared_by.is_empty(),
            Self::RpmOwned { .. } => true,
            Self::Unbacked => false,
        }
    }
}

/// Separator between the unit names of a directory two unit files both
/// declare.
const DECLARING_UNIT_SEPARATOR: &str = ", ";

/// Name the declaring units for a surface that shows an operator which
/// creator a `/var` directory has.
///
/// Filenames rather than full paths, because that is the name
/// `systemctl` answers to, and all of them rather than the first,
/// because a directory two unit files declare has two creators and
/// naming one of them invites the reader to act on the wrong file.
///
/// Distinct filenames, because the commonest two-declarer case is an
/// `/etc` shadow over the vendor unit of the same name: one unit, two
/// files, and printing the name twice says nothing the reader can use.
pub fn declaring_unit_label(declared_by: &[String]) -> String {
    let mut names: Vec<String> = Vec::new();
    for path in declared_by {
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names.join(DECLARING_UNIT_SEPARATOR)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnbackedVarAdvisory {
    pub disposition: FindingKind,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarDirectory {
    #[serde(default)]
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backing: Option<VarDirBacking>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_uid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_gid: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StorageSection {
    #[serde(default)]
    pub fstab_entries: Vec<FstabEntry>,
    #[serde(default)]
    pub mount_points: Vec<MountPoint>,
    #[serde(default)]
    pub lvm_info: Vec<LvmVolume>,
    #[serde(default)]
    pub var_directories: Vec<VarDirectory>,
    #[serde(default)]
    pub credential_refs: Vec<CredentialRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unbacked_var_advisory: Option<UnbackedVarAdvisory>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each line split as `systemd-tmpfiles` splits it: (type, path,
    /// mode, user, group), or `None` where systemd rejects the line.
    #[test]
    fn tmpfiles_fields_are_unquoted_and_unescaped_as_systemd_reads_them() {
        type Fields<'a> = (
            char,
            &'a str,
            Option<&'a str>,
            Option<&'a str>,
            Option<&'a str>,
        );
        let cases: [(&str, Option<Fields<'_>>); 16] = [
            (
                "d /var/lib/a 0750 appuser appgroup 30d",
                Some((
                    'd',
                    "/var/lib/a",
                    Some("0750"),
                    Some("appuser"),
                    Some("appgroup"),
                )),
            ),
            ("d /var/lib/a", Some(('d', "/var/lib/a", None, None, None))),
            (
                "d \"/var/lib/a b\" 0750 appuser appgroup 30d",
                Some((
                    'd',
                    "/var/lib/a b",
                    Some("0750"),
                    Some("appuser"),
                    Some("appgroup"),
                )),
            ),
            (
                "d '/var/lib/a b' 0750 'appuser' \"appgroup\" -",
                Some((
                    'd',
                    "/var/lib/a b",
                    Some("0750"),
                    Some("appuser"),
                    Some("appgroup"),
                )),
            ),
            (
                "d /var/lib/a\\x20b 0750 appuser appgroup -",
                Some((
                    'd',
                    "/var/lib/a b",
                    Some("0750"),
                    Some("appuser"),
                    Some("appgroup"),
                )),
            ),
            (
                "d /var/lib/a\\sb 0750 app\\165ser - -",
                Some((
                    'd',
                    "/var/lib/a b",
                    Some("0750"),
                    Some("appuser"),
                    Some("-"),
                )),
            ),
            // A quote may start mid-field and the other quote is literal
            // inside it.
            (
                "d /var/lib/\"a 'b\"c 0750 appuser",
                Some(('d', "/var/lib/a 'bc", Some("0750"), Some("appuser"), None)),
            ),
            (
                "d \"/var/lib/a\\\"b\" - appuser",
                Some(('d', "/var/lib/a\"b", Some("-"), Some("appuser"), None)),
            ),
            // Quoting past the age field is the argument, never split.
            (
                "f /var/lib/a 0640 appuser appgroup - \"hello",
                Some((
                    'f',
                    "/var/lib/a",
                    Some("0640"),
                    Some("appuser"),
                    Some("appgroup"),
                )),
            ),
            ("  # comment", None),
            ("", None),
            ("d", None),
            ("d \"/var/lib/a 0750 appuser", None),
            ("d /var/lib/a\\q 0750 appuser", None),
            // A backslash before a space is no C escape, so systemd
            // rejects the line rather than keep the space.
            ("d /var/lib/a\\ b 0750 appuser", None),
            ("d /var/lib/a 0750 appuser appgroup \"30d", None),
        ];
        for (line, want) in cases {
            let got = TmpfilesEntry::parse(line);
            let got = got.as_ref().map(|e| {
                (
                    e.entry_type,
                    e.path.as_str(),
                    e.mode.as_deref(),
                    e.user.as_deref(),
                    e.group.as_deref(),
                )
            });
            assert_eq!(got, want, "{line}");
        }
    }

    /// systemd's `\U` escape refuses the code points `unichar_is_valid`
    /// rejects: surrogates, past U+10FFFF, U+FDD0..U+FDEF, and U+xFFFE
    /// and U+xFFFF in every plane. Its `\u` escape refuses only NUL, so
    /// a BMP noncharacter written that way is kept.
    #[test]
    fn a_unicode_escape_keeps_only_code_points_systemd_accepts() {
        let cases = [
            ("\\U0000FFFF", None),
            ("\\U0000FFFE", None),
            ("\\U0000FDD0", None),
            ("\\U0000FDEF", None),
            ("\\U0001FFFE", None),
            ("\\U0010FFFE", None),
            ("\\U0010FFFF", None),
            ("\\U0000D800", None),
            ("\\U00110000", None),
            ("\\U0001F600", Some("/var/lib/a\u{1F600}")),
            ("\\U0000FDCF", Some("/var/lib/a\u{FDCF}")),
            ("\\U0000FDF0", Some("/var/lib/a\u{FDF0}")),
            ("\\uFFFF", Some("/var/lib/a\u{FFFF}")),
        ];
        for (escape, want) in cases {
            let line = format!("d /var/lib/a{escape} 0750 appuser appgroup -");
            let got = TmpfilesEntry::parse(&line).map(|e| e.path);
            assert_eq!(got.as_deref(), want, "{line}");
        }
    }

    #[test]
    fn test_storage_section_roundtrip() {
        let section = StorageSection {
            fstab_entries: vec![FstabEntry {
                device: "/dev/sda1".into(),
                mount_point: "/boot".into(),
                fstype: "xfs".into(),
                options: "defaults".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: StorageSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }

    #[test]
    fn var_directory_without_ownership_deserializes() {
        let json = r#"{"path":"/var/lib/test"}"#;
        let dir: VarDirectory = serde_json::from_str(json).unwrap();
        assert_eq!(dir.path, "/var/lib/test");
        assert!(dir.owner_uid.is_none());
        assert!(dir.mode.is_none());
    }

    /// The tag and payload keys are the schema 25 wire contract, so they
    /// are written out here rather than snapshotted from the serializer.
    #[test]
    fn var_dir_backing_tags_its_payload() {
        let cases = [
            (
                VarDirBacking::Tmpfiles {
                    conf: "/etc/tmpfiles.d/appone.conf".into(),
                    line: "d /var/lib/appone/cache 0750 appone appone -".into(),
                },
                r#"{"kind":"tmpfiles","conf":"/etc/tmpfiles.d/appone.conf","line":"d /var/lib/appone/cache 0750 appone appone -"}"#,
            ),
            (
                VarDirBacking::StateDirectory {
                    declared_by: vec!["/usr/lib/systemd/system/myapp.service".into()],
                },
                r#"{"kind":"state_directory","declared_by":["/usr/lib/systemd/system/myapp.service"]}"#,
            ),
            (
                // Two declarers, the case the filename could not record:
                // an `/etc` shadow and the vendor unit under it.
                VarDirBacking::CacheDirectory {
                    declared_by: vec![
                        "/usr/lib/systemd/system/httpd.service".into(),
                        "/etc/systemd/system/httpd.service".into(),
                    ],
                },
                r#"{"kind":"cache_directory","declared_by":["/usr/lib/systemd/system/httpd.service","/etc/systemd/system/httpd.service"]}"#,
            ),
            (
                VarDirBacking::LogsDirectory {
                    declared_by: vec!["/usr/lib/systemd/system/nginx.service".into()],
                },
                r#"{"kind":"logs_directory","declared_by":["/usr/lib/systemd/system/nginx.service"]}"#,
            ),
            (
                VarDirBacking::RpmOwned {
                    package: "rpm".into(),
                },
                r#"{"kind":"rpm_owned","package":"rpm"}"#,
            ),
            (VarDirBacking::Unbacked, r#"{"kind":"unbacked"}"#),
        ];
        for (backing, expected) in cases {
            assert_eq!(serde_json::to_string(&backing).unwrap(), expected);
            let parsed: VarDirBacking = serde_json::from_str(expected).unwrap();
            assert_eq!(parsed, backing);
        }
    }
}
