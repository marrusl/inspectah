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
    /// `unit` is the filename of the unit carrying the `StateDirectory=`.
    StateDirectory {
        unit: String,
    },
    /// `unit` is the filename of the unit carrying the `CacheDirectory=`.
    CacheDirectory {
        unit: String,
    },
    /// `unit` is the filename of the unit carrying the `LogsDirectory=`.
    LogsDirectory {
        unit: String,
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
            Self::StateDirectory { .. }
            | Self::CacheDirectory { .. }
            | Self::LogsDirectory { .. }
            | Self::RpmOwned { .. } => true,
            Self::Unbacked => false,
        }
    }
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

    /// The tag and payload keys are the schema 23 wire contract, so they
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
                    unit: "myapp.service".into(),
                },
                r#"{"kind":"state_directory","unit":"myapp.service"}"#,
            ),
            (
                VarDirBacking::CacheDirectory {
                    unit: "httpd.service".into(),
                },
                r#"{"kind":"cache_directory","unit":"httpd.service"}"#,
            ),
            (
                VarDirBacking::LogsDirectory {
                    unit: "nginx.service".into(),
                },
                r#"{"kind":"logs_directory","unit":"nginx.service"}"#,
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
