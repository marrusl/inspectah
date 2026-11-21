use super::FindingKind;
use super::aggregate::AggregatePrevalence;
use serde::{Deserialize, Serialize};

/// The `-f` flag of `semanage fcontext`; `All` emits no flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FcontextFileType {
    All,
    RegularFile,
    Directory,
    CharDevice,
    BlockDevice,
    Socket,
    Symlink,
    NamedPipe,
}

impl FcontextFileType {
    /// `semanage fcontext -f` letter, or `None` for `All`.
    pub fn semanage_flag(self) -> Option<char> {
        match self {
            Self::All => None,
            Self::RegularFile => Some('f'),
            Self::Directory => Some('d'),
            Self::CharDevice => Some('c'),
            Self::BlockDevice => Some('b'),
            Self::Socket => Some('s'),
            Self::Symlink => Some('l'),
            Self::NamedPipe => Some('p'),
        }
    }

    /// Parses the `-l -C` type column ("all files", "regular file", ...).
    pub fn parse_column(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "all files" => Some(Self::All),
            "regular file" => Some(Self::RegularFile),
            "directory" => Some(Self::Directory),
            "character device" => Some(Self::CharDevice),
            "block device" => Some(Self::BlockDevice),
            "socket" => Some(Self::Socket),
            "symbolic link" => Some(Self::Symlink),
            "named pipe" => Some(Self::NamedPipe),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelinuxContext {
    pub user: String,
    pub role: String,
    pub type_: String,
    /// MLS level, `None` when the policy prints no fourth field.
    pub level: Option<String>,
}

/// A custom local fcontext rule, in whichever collected order the source
/// preserved (see [`FcontextSource`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FcontextRule {
    Label {
        spec: String,
        file_type: FcontextFileType,
        context: SelinuxContext,
        #[serde(default)]
        disposition: FindingKind,
        aggregate: Option<AggregatePrevalence>,
    },
    /// `<<None>>` in the context column: do not relabel.
    Unlabel {
        spec: String,
        file_type: FcontextFileType,
        #[serde(default)]
        disposition: FindingKind,
        aggregate: Option<AggregatePrevalence>,
    },
    /// `subject = labeled_like` in the equivalence section; renders as
    /// `semanage fcontext -a -e <labeled_like> <subject>`.
    Equivalence {
        subject: String,
        labeled_like: String,
        #[serde(default)]
        disposition: FindingKind,
        aggregate: Option<AggregatePrevalence>,
    },
    /// A line the parser could not place. Display-only; the renderer prints
    /// it inside the FIXME so the operator can transcribe it.
    Unparsed { raw: String },
}

/// Where `SelinuxSection.fcontext_rules`'s order came from.
///
/// The policy store files hold rules in insertion order, which is also the
/// order the policy resolves overlapping local expressions by (recency).
/// `semanage fcontext -l -C` prints the same local table sorted by
/// expression instead, which can silently invert which rule wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FcontextSource {
    PolicyStore,
    #[default]
    SemanageList,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SelinuxPortLabel {
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub port: String,
    #[serde(default, rename = "type")]
    pub label_type: String,
    #[serde(default)]
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub aggregate: Option<AggregatePrevalence>,
}

/// An SELinux boolean the administrator set persistently on the host, with
/// `setsebool -P` or `semanage boolean -m`, as the policy store's
/// `booleans.local` records it.
///
/// A runtime-only change (`setsebool` without `-P`) is not one: the host
/// itself loses it at its next boot, so the image does not carry it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SelinuxBoolean {
    pub name: String,
    /// The persistent value: `true` is on.
    pub value: bool,
}

impl SelinuxBoolean {
    /// The value as `setsebool` and `semanage boolean -l` write it.
    pub fn value_word(&self) -> &'static str {
        if self.value { "on" } else { "off" }
    }
}

/// Whether `name` can be an SELinux boolean's name: a letter, then
/// letters, digits, `_` and `-`, with single `.`s between them for a name
/// declared inside a CIL block.
///
/// This is libsepol's CIL name rule (`cil_verify_name` in
/// `libsepol/cil/src/cil_verify.c`: a letter first, then alphanumerics, `_`
/// and `-`, and `.` where names are qualified), which also matches
/// checkpolicy's identifier token. A name that fails it is no boolean any
/// policy defines, and it is never a shell glob or an option word.
pub fn is_boolean_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && !name.ends_with('.')
        && !name.contains("..")
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Whether libsemanage accepts `name` as a policy module's name: a letter,
/// then letters, digits, `_` and `-`, with a `.` allowed only before one
/// of those, so never first, last or doubled.
///
/// This is `semanage_module_validate_name` in
/// `libsemanage/src/modules.c` (SELinux 3.6), whose comment gives it as
/// `^[a-zA-Z](\.?[a-zA-Z0-9_-])*$`; libsemanage checks every name it
/// installs, disables or extracts against it, so a store entry that fails
/// it is no module `semodule` can act on. A name that passes holds no
/// whitespace, shell syntax, glob or option word, which is what lets it
/// stand unquoted in a command suggestion.
pub fn is_module_name(name: &str) -> bool {
    let valid = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-');
    let mut bytes = name.bytes();
    if !bytes.next().is_some_and(|b| b.is_ascii_alphabetic()) {
        return false;
    }
    while let Some(b) = bytes.next() {
        if valid(b) || (b == b'.' && bytes.next().is_some_and(valid)) {
            continue;
        }
        return false;
    }
    true
}

/// A boolean the hosts of a fleet set persistently to different values.
///
/// Only a fleet aggregate has these. The merge moves a boolean here, out
/// of [`SelinuxSection::boolean_overrides`], when one host sets it on and
/// another off, so no export runs both `setsebool` commands and lets host
/// order pick the image's value. Both host lists are nonempty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelinuxBooleanConflict {
    pub name: String,
    /// The hosts that set it on, sorted.
    pub on_hosts: Vec<String>,
    /// The hosts that set it off, sorted.
    pub off_hosts: Vec<String>,
}

/// Where a module turned off with `semodule -d` is installed in the
/// policy store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleOrigin {
    /// At a local priority: a custom module, which the image does not have
    /// unless it is carried.
    Local,
    /// Only at the base policy's or a package's priority, so the image's
    /// own policy packages provide it.
    Packaged,
}

/// A policy module turned off on the host with `semodule -d`, which
/// libsemanage records in the store's `modules/disabled/`. The module
/// stays installed; the policy is built without it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DisabledModule {
    pub name: String,
    pub origin: ModuleOrigin,
}

/// Security & access control section (SELinux and FIPS).
///
/// PAM configs and audit rules are not here: they are `config.files`
/// entries with category `Pam` and `Audit`, which is what gives them a
/// disposition (spec § 3.6).
///
/// Display name: "Security & Access Control". The JSON key is `"selinux"`
/// because that is what the collectors emit.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SelinuxSection {
    #[serde(default)]
    pub mode: String,
    /// The modules installed at a local priority and not disabled.
    #[serde(default)]
    pub custom_modules: Vec<String>,
    /// The modules turned off with `semodule -d`, by name, local ones
    /// included. A disabled local module is here and not in
    /// `custom_modules`.
    #[serde(default)]
    pub disabled_modules: Vec<DisabledModule>,
    /// The booleans set persistently on the host, in the store's order.
    /// In a fleet aggregate, only the ones no two hosts disagree on.
    #[serde(default)]
    pub boolean_overrides: Vec<SelinuxBoolean>,
    /// The booleans a fleet's hosts set to different values. Empty for a
    /// single host.
    #[serde(default)]
    pub boolean_conflicts: Vec<SelinuxBooleanConflict>,
    #[serde(default)]
    pub fcontext_rules: Vec<FcontextRule>,
    #[serde(default)]
    pub fcontext_source: FcontextSource,
    #[serde(default)]
    pub fips_mode: bool,
    #[serde(default)]
    pub port_labels: Vec<SelinuxPortLabel>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant's `semanage fcontext -f` letter.
    ///
    /// The letters come from the `-f {a,f,d,c,b,s,l,p}` synopsis of
    /// `semanage-fcontext(8)` and its gloss ("a" all files, "f" regular file,
    /// "d" directory, "c" character device, "b" block device, "s" socket,
    /// "l" symbolic link, "p" named pipe), not from the function under test.
    /// Six of the seven are corroborated inside this repo by the
    /// `file_contexts.local` flag forms the collector parses
    /// (`collect/src/inspectors/selinux.rs:392-398`: `-d`, `-c`, `-b`, `-s`,
    /// `-l`, `-p`); only `f` rests on the man page alone, because the store
    /// file spells regular file `--`.
    ///
    /// `All` is `None` rather than `Some('a')` on purpose: "all files" is the
    /// default, and § 5.1 emits `-f` only when the type is not `All`.
    #[test]
    fn test_semanage_flag_letters() {
        let cases: &[(FcontextFileType, Option<char>)] = &[
            (FcontextFileType::All, None),
            (FcontextFileType::RegularFile, Some('f')),
            (FcontextFileType::Directory, Some('d')),
            (FcontextFileType::CharDevice, Some('c')),
            (FcontextFileType::BlockDevice, Some('b')),
            (FcontextFileType::Socket, Some('s')),
            (FcontextFileType::Symlink, Some('l')),
            (FcontextFileType::NamedPipe, Some('p')),
        ];
        for (file_type, expected) in cases {
            assert_eq!(
                file_type.semanage_flag(),
                *expected,
                "wrong -f letter for {file_type:?}"
            );
        }
    }

    #[test]
    fn test_selinux_section_roundtrip() {
        let section = SelinuxSection {
            mode: "enforcing".to_string(),
            custom_modules: vec!["myapp".to_string()],
            disabled_modules: vec![DisabledModule {
                name: "apache".to_string(),
                origin: ModuleOrigin::Packaged,
            }],
            boolean_overrides: vec![SelinuxBoolean {
                name: "httpd_can_network_connect".to_string(),
                value: true,
            }],
            boolean_conflicts: vec![SelinuxBooleanConflict {
                name: "virt_use_nfs".to_string(),
                on_hosts: vec!["host-a".to_string()],
                off_hosts: vec!["host-b".to_string()],
            }],
            fcontext_rules: vec![FcontextRule::Label {
                spec: "/opt/app(/.*)?".to_string(),
                file_type: FcontextFileType::All,
                context: SelinuxContext {
                    user: "system_u".to_string(),
                    role: "object_r".to_string(),
                    type_: "httpd_sys_content_t".to_string(),
                    level: Some("s0".to_string()),
                },
                disposition: FindingKind::included(),
                aggregate: None,
            }],
            fcontext_source: FcontextSource::PolicyStore,
            fips_mode: false,
            port_labels: vec![SelinuxPortLabel {
                protocol: "tcp".to_string(),
                port: "8080".to_string(),
                label_type: "http_port_t".to_string(),
                disposition: FindingKind::included(),
                locked: false,
                aggregate: None,
            }],
        };

        let json = serde_json::to_string(&section).unwrap();
        let parsed: SelinuxSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }

    /// The boolean name grammar: libsepol's CIL name rule.
    #[test]
    fn boolean_names_follow_the_policy_name_grammar() {
        let cases: &[(&str, bool)] = &[
            ("httpd_can_network_connect", true),
            ("virt_use_nfs", true),
            ("a", true),
            ("secure-mode", true),
            ("block.inner_bool", true),
            ("", false),
            ("*", false),
            ("?", false),
            ("[ab]", false),
            ("httpd_*", false),
            ("httpd_?", false),
            ("httpd_[ab]", false),
            ("-P", false),
            ("#foo", false),
            ("_leading", false),
            ("9lives", false),
            ("trailing.", false),
            ("double..dot", false),
            ("has space", false),
            ("semi;colon", false),
            ("dollar$", false),
        ];
        for (name, expected) in cases {
            assert_eq!(is_boolean_name(name), *expected, "{name:?}");
        }
    }

    #[test]
    fn module_names_follow_libsemanage_module_name_grammar() {
        let cases: &[(&str, bool)] = &[
            ("myapp", true),
            ("permissive_rhcd_t", true),
            ("extra_varrun", true),
            ("a", true),
            ("my-app", true),
            ("a.b", true),
            ("a._", true),
            ("a.-b", true),
            ("A9", true),
            ("", false),
            ("9lives", false),
            ("_leading", false),
            ("-d", false),
            (".hidden", false),
            ("trailing.", false),
            ("double..dot", false),
            ("apache;id", false),
            ("y$(id)", false),
            ("x`id`", false),
            ("has space", false),
            ("nl\nRUN id", false),
            ("glob*", false),
            ("caf\u{e9}", false),
        ];
        for (name, expected) in cases {
            assert_eq!(is_module_name(name), *expected, "{name:?}");
        }
    }
}
