use super::aggregate::AggregatePrevalence;
use super::config::ConfigFileEntry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguagePackage {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
}

/// Type alias for backward compatibility.
pub type PipPackage = LanguagePackage;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NonRpmItem {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub confidence: String,
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub r#static: bool,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub shared_libs: Vec<String>,
    #[serde(default)]
    pub system_site_packages: bool,
    #[serde(default)]
    pub packages: Vec<LanguagePackage>,
    #[serde(default)]
    pub has_c_extensions: bool,
    #[serde(default)]
    pub git_remote: String,
    #[serde(default)]
    pub git_commit: String,
    #[serde(default)]
    pub git_branch: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub manifest_files: HashMap<String, String>,
    #[serde(default)]
    pub rpm_filtered: bool,
    pub files: Option<serde_json::Value>,
    #[serde(default)]
    pub content: String,
    pub aggregate: Option<AggregatePrevalence>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub review_status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NonRpmSoftwareSection {
    #[serde(default)]
    pub items: Vec<NonRpmItem>,
    #[serde(default)]
    pub env_files: Vec<ConfigFileEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    #[default]
    Other,
    ElfBinary,
    Jar,
    Script,
    DataFile,
    Config,
    Symlink,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProvenanceSignals {
    #[serde(default)]
    pub file_type: FileType,
    /// Last-modified timestamp (seconds since epoch)
    #[serde(default)]
    pub last_modified: u64,
    /// Filesystem UID
    #[serde(default)]
    pub uid: u32,
    /// Filesystem GID
    #[serde(default)]
    pub gid: u32,
    /// Octal file permissions (e.g., "0755")
    #[serde(default)]
    pub permissions: String,

    // --- Derived signals (spec-required) ---
    /// True when the file's mtime is newer than the system install date.
    /// System install date is derived from `/etc/machine-id` ctime or
    /// the install time of a baseos RPM (e.g., `filesystem` package).
    /// Newer files are likely runtime-generated data, not deployed payload.
    #[serde(default)]
    pub mutable: bool,
    /// True when the file lives on a read-write mount point.
    /// Determined by parsing `/proc/mounts` and matching the file's
    /// path to the longest-prefix mount that has the `rw` option.
    #[serde(default)]
    pub writable_mount: bool,
    /// True when the file path is under any systemd service's
    /// `WorkingDirectory=` (parsed from `/etc/systemd/system/*.service`
    /// and `/usr/lib/systemd/system/*.service` unit files).
    #[serde(default)]
    pub service_working_dir: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UnmanagedFile {
    /// Absolute path on the source host
    #[serde(default)]
    pub path: String,
    /// File size in bytes
    #[serde(default)]
    pub size: u64,
    /// Detected file type
    #[serde(default)]
    pub file_type: FileType,
    /// Provenance signals for review (raw metadata + derived signals)
    #[serde(default)]
    pub provenance: ProvenanceSignals,
    /// Include in export (default true — user toggles in refine)
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    /// True if path is under /var (needs bootc persistence warning).
    /// Note: /var is NOT a scan root — this flag is only set when an
    /// unmanaged file under /opt, /srv, or /usr/local has a symlink
    /// target or runtime-generated path that resolves under /var.
    /// The /var WARNING in the spec is advisory guidance for the refine
    /// UI, not a scan-scope directive.
    #[serde(default)]
    pub under_var: bool,
    /// Resolved symlink target (only set when file_type == Symlink).
    /// Advisory only — bundling recreates the link rather than following it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub link_target: String,
    /// Aggregate prevalence (populated in aggregate mode)
    pub aggregate: Option<AggregatePrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UnmanagedFileSection {
    #[serde(default)]
    pub items: Vec<UnmanagedFile>,
    /// Total size of all cataloged files in bytes
    #[serde(default)]
    pub total_size: u64,
    /// Total number of cataloged files
    #[serde(default)]
    pub total_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nonrpm_section_roundtrip() {
        let mut manifest_files = HashMap::new();
        manifest_files.insert("package.json".to_string(), "{}".to_string());

        let section = NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                path: "/opt/app/bin".to_string(),
                name: "custom-app".to_string(),
                method: "binary".to_string(),
                confidence: "high".to_string(),
                include: true,
                locked: false,
                acknowledged: false,
                lang: "c".to_string(),
                r#static: true,
                version: "1.0.0".to_string(),
                shared_libs: vec!["/lib64/libc.so.6".to_string()],
                system_site_packages: false,
                packages: vec![LanguagePackage {
                    name: "example-pkg".to_string(),
                    version: "1.2.3".to_string(),
                }],
                has_c_extensions: false,
                git_remote: String::new(),
                git_commit: String::new(),
                git_branch: String::new(),
                manifest_files: manifest_files.clone(),
                rpm_filtered: true,
                files: None,
                content: String::new(),
                aggregate: None,
                review_status: String::new(),
                notes: String::new(),
            }],
            env_files: vec![],
        };

        let json = serde_json::to_string(&section).unwrap();
        let parsed: NonRpmSoftwareSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }

    #[test]
    fn test_unmanaged_file_roundtrip() {
        let file = UnmanagedFile {
            path: "/opt/splunk/bin/splunkd".into(),
            size: 52428800,
            file_type: FileType::ElfBinary,
            provenance: ProvenanceSignals {
                file_type: FileType::ElfBinary,
                last_modified: 1700000000,
                uid: 0,
                gid: 0,
                permissions: "0755".into(),
                mutable: false,
                writable_mount: false,
                service_working_dir: false,
            },
            include: true,
            under_var: false,
            ..Default::default()
        };
        let json = serde_json::to_string(&file).unwrap();
        let deser: UnmanagedFile = serde_json::from_str(&json).unwrap();
        assert_eq!(file, deser);
    }

    #[test]
    fn test_provenance_signals_derived_fields_roundtrip() {
        let signals = ProvenanceSignals {
            file_type: FileType::DataFile,
            last_modified: 1700000000,
            uid: 1000,
            gid: 1000,
            permissions: "0644".into(),
            mutable: true,
            writable_mount: true,
            service_working_dir: true,
        };
        let json = serde_json::to_string(&signals).unwrap();
        let deser: ProvenanceSignals = serde_json::from_str(&json).unwrap();
        assert_eq!(signals, deser);
    }

    #[test]
    fn test_unmanaged_file_section_roundtrip() {
        let section = UnmanagedFileSection {
            items: vec![UnmanagedFile {
                path: "/opt/app/server".into(),
                size: 1024,
                file_type: FileType::ElfBinary,
                ..Default::default()
            }],
            total_size: 1024,
            total_count: 1,
        };
        let json = serde_json::to_string(&section).unwrap();
        let deser: UnmanagedFileSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, deser);
    }

    #[test]
    fn test_unmanaged_file_defaults_from_empty_json() {
        let deser: UnmanagedFile = serde_json::from_str("{}").unwrap();
        assert_eq!(deser.path, "");
        assert_eq!(deser.size, 0);
        assert_eq!(deser.file_type, FileType::Other);
        assert!(deser.include); // default_true
        assert!(!deser.under_var);
        assert!(!deser.provenance.mutable);
        assert!(!deser.provenance.writable_mount);
        assert!(!deser.provenance.service_working_dir);
    }
}
