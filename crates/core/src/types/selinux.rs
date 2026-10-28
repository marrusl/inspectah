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
    },
    /// `<<None>>` in the context column: do not relabel.
    Unlabel {
        spec: String,
        file_type: FcontextFileType,
        #[serde(default)]
        disposition: FindingKind,
    },
    /// `subject = labeled_like` in the equivalence section; renders as
    /// `semanage fcontext -a -e <labeled_like> <subject>`.
    Equivalence {
        subject: String,
        labeled_like: String,
        #[serde(default)]
        disposition: FindingKind,
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

/// A file that the security inspector carries forward for materialization
/// in the config tree (audit rules, PAM configs).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CarryForwardFile {
    /// Relative path (e.g. "etc/audit/rules.d/custom.rules").
    #[serde(default)]
    pub path: String,
    /// File content to materialize.
    #[serde(default)]
    pub content: String,
}

/// Security & access control section (SELinux, FIPS, PAM, audit rules).
///
/// Display name: "Security & Access Control". The JSON key is `"selinux"`
/// because that is what the collectors emit.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SelinuxSection {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub custom_modules: Vec<String>,
    #[serde(default)]
    pub boolean_overrides: Vec<serde_json::Value>,
    #[serde(default)]
    pub fcontext_rules: Vec<FcontextRule>,
    #[serde(default)]
    pub fcontext_source: FcontextSource,
    #[serde(default)]
    pub audit_rules: Vec<CarryForwardFile>,
    #[serde(default)]
    pub fips_mode: bool,
    #[serde(default)]
    pub pam_configs: Vec<CarryForwardFile>,
    #[serde(default)]
    pub port_labels: Vec<SelinuxPortLabel>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selinux_section_roundtrip() {
        let section = SelinuxSection {
            mode: "enforcing".to_string(),
            custom_modules: vec!["myapp".to_string()],
            boolean_overrides: vec![
                serde_json::json!({"name": "httpd_can_network_connect", "state": true}),
            ],
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
            }],
            fcontext_source: FcontextSource::PolicyStore,
            audit_rules: vec![CarryForwardFile {
                path: "etc/audit/rules.d/custom.rules".to_string(),
                content: "-w /etc/shadow -p wa".to_string(),
            }],
            fips_mode: false,
            pam_configs: vec![CarryForwardFile {
                path: "etc/pam.d/custom-sshd".to_string(),
                content: "auth required pam_unix.so".to_string(),
            }],
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
}
