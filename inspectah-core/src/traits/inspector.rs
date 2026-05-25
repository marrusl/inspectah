use crate::baseline::BaselineData;
use crate::traits::progress::ProgressSink;
use crate::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use crate::types::redaction::RedactionHint;
use crate::types::rpm::{EnabledModuleStream, PackageEntry, RpmVaEntry};
use crate::types::system::SourceSystem;
use crate::types::warnings::Warning;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

pub trait Inspector: Send + Sync {
    fn id(&self) -> InspectorId;
    fn applicable_to(&self) -> &[SourceSystemKind];
    fn inspect(
        &self,
        ctx: &InspectionContext<'_>,
        progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError>;
}

/// Borrowed references into executor + source system state.
/// Enables scoped-thread execution where multiple InspectionContext
/// values share one executor.
pub struct InspectionContext<'a> {
    pub source_system: &'a SourceSystem,
    pub executor: &'a dyn crate::traits::executor::Executor,
    pub rpm_state: Option<&'a RpmState>,
    pub baseline_data: Option<&'a BaselineData>,
}

/// Read-only RPM state provided to non-RPM inspectors during two-phase collection.
///
/// Immutable after construction in `handle_result()`. No interior mutability.
/// Data flow: shell command -> RPM inspector output -> handle_result() extraction
/// -> owned_paths HashSet + path_to_package reverse index.
///
/// **RPM failure propagation:**
/// - `ctx.rpm_state: None` -> RPM inspector failed entirely. Wave 2 inspectors
///   MUST return `Err(InspectorError::Failed)`.
/// - `ctx.rpm_state: Some(state)` where `state.owned_paths.is_empty()` -> RPM
///   succeeded but no ownership data. Wave 2 inspectors proceed normally.
/// - The distinction matters: None = "no data, can't trust classifications";
///   Some(empty) = "confirmed no RPM-owned paths."
#[derive(Debug, Clone, Default)]
pub struct RpmState {
    pub installed_packages: HashSet<String>,
    pub owned_paths: HashSet<PathBuf>,
    pub packages: Vec<PackageEntry>,
    pub verification_results: Vec<RpmVaEntry>,
    pub module_streams: Vec<EnabledModuleStream>,
    pub path_to_package: HashMap<PathBuf, usize>,
}

impl RpmState {
    /// Access the set of installed package names.
    pub fn installed_packages(&self) -> &HashSet<String> {
        &self.installed_packages
    }

    /// Access the full package list.
    pub fn packages(&self) -> &[PackageEntry] {
        &self.packages
    }

    /// Access the set of RPM-owned paths (filtered to /etc during construction).
    pub fn owned_paths(&self) -> &HashSet<PathBuf> {
        &self.owned_paths
    }

    /// O(1) check whether a path is RPM-owned.
    pub fn is_rpm_owned(&self, path: &Path) -> bool {
        self.owned_paths.contains(path)
    }

    /// Look up the PackageEntry that owns a given path.
    /// Returns None if the path is not in the reverse index.
    pub fn package_for_path(&self, path: &Path) -> Option<&PackageEntry> {
        self.path_to_package
            .get(path)
            .and_then(|&idx| self.packages.get(idx))
    }

    /// Access RPM verification results (rpm -Va output).
    pub fn verification_results(&self) -> &[RpmVaEntry] {
        &self.verification_results
    }

    /// Access enabled DNF module streams.
    pub fn module_streams(&self) -> &[EnabledModuleStream] {
        &self.module_streams
    }
}

/// Typed section output — the compiler proves inspectors emit valid section shapes.
#[derive(Debug, Clone)]
pub struct InspectorOutput {
    pub section: SectionData,
    pub warnings: Vec<Warning>,
    pub redaction_hints: Vec<RedactionHint>,
}

#[derive(Debug, Clone)]
pub enum InspectorError {
    Skipped {
        reason: String,
    },
    Degraded {
        partial: Box<InspectorOutput>,
        reason: String,
    },
    Failed {
        reason: String,
    },
}

impl fmt::Display for InspectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Skipped { reason } => write!(f, "skipped: {reason}"),
            Self::Degraded { reason, .. } => write!(f, "degraded: {reason}"),
            Self::Failed { reason } => write!(f, "failed: {reason}"),
        }
    }
}

impl std::error::Error for InspectorError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::rpm::{PackageEntry, PackageState, RpmVaEntry};

    #[test]
    fn test_inspector_error_display() {
        let err = InspectorError::Skipped {
            reason: "not applicable".into(),
        };
        assert!(format!("{err}").contains("not applicable"));

        let err = InspectorError::Failed {
            reason: "rpm db corrupt".into(),
        };
        assert!(format!("{err}").contains("rpm db corrupt"));
    }

    #[test]
    fn test_degraded_carries_partial_output() {
        use crate::types::completeness::SectionData;
        use crate::types::rpm::RpmSection;
        let output = InspectorOutput {
            section: SectionData::Rpm(RpmSection::default()),
            warnings: vec![],
            redaction_hints: vec![],
        };
        let err = InspectorError::Degraded {
            partial: Box::new(output.clone()),
            reason: "partial rpm db".into(),
        };
        if let InspectorError::Degraded { partial, .. } = err {
            assert_eq!(partial.warnings.len(), 0);
        }
    }

    // ── RpmState capability method tests ──

    /// Helper: build an RpmState with /etc paths owned and a package index.
    fn build_test_rpm_state() -> RpmState {
        let packages = vec![
            PackageEntry {
                name: "httpd".into(),
                version: "2.4.57".into(),
                state: PackageState::Added,
                ..Default::default()
            },
            PackageEntry {
                name: "sshd".into(),
                version: "9.0".into(),
                state: PackageState::Added,
                ..Default::default()
            },
        ];

        let mut owned_paths = HashSet::new();
        owned_paths.insert(PathBuf::from("/etc/httpd/conf/httpd.conf"));
        owned_paths.insert(PathBuf::from("/etc/ssh/sshd_config"));

        let mut path_to_package = HashMap::new();
        path_to_package.insert(PathBuf::from("/etc/httpd/conf/httpd.conf"), 0);
        path_to_package.insert(PathBuf::from("/etc/ssh/sshd_config"), 1);

        let mut installed = HashSet::new();
        installed.insert("httpd".into());
        installed.insert("sshd".into());

        RpmState {
            installed_packages: installed,
            owned_paths,
            packages,
            verification_results: vec![RpmVaEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                flags: "S.5....T.".into(),
                package: Some("httpd".into()),
            }],
            module_streams: vec![],
            path_to_package,
        }
    }

    #[test]
    fn test_rpm_state_owned_paths_filters_etc() {
        // owned_paths should only contain /etc paths — /usr and /var excluded.
        // This tests the contract that owned_paths is pre-filtered during construction.
        let mut owned = HashSet::new();
        owned.insert(PathBuf::from("/etc/httpd/conf/httpd.conf"));
        // Do NOT insert /usr or /var paths — they are filtered at construction time.

        let state = RpmState {
            owned_paths: owned,
            ..Default::default()
        };

        assert!(
            state
                .owned_paths()
                .contains(&PathBuf::from("/etc/httpd/conf/httpd.conf"))
        );
        assert!(
            !state
                .owned_paths()
                .contains(&PathBuf::from("/usr/bin/httpd"))
        );
        assert!(
            !state
                .owned_paths()
                .contains(&PathBuf::from("/var/log/httpd/access.log"))
        );
    }

    #[test]
    fn test_rpm_state_is_rpm_owned_true() {
        let state = build_test_rpm_state();
        assert!(state.is_rpm_owned(Path::new("/etc/httpd/conf/httpd.conf")));
        assert!(state.is_rpm_owned(Path::new("/etc/ssh/sshd_config")));
    }

    #[test]
    fn test_rpm_state_is_rpm_owned_false() {
        let state = build_test_rpm_state();
        assert!(!state.is_rpm_owned(Path::new("/etc/unknown/file.conf")));
        assert!(!state.is_rpm_owned(Path::new("/usr/bin/httpd")));
    }

    #[test]
    fn test_rpm_state_package_for_path() {
        let state = build_test_rpm_state();
        let pkg = state.package_for_path(Path::new("/etc/httpd/conf/httpd.conf"));
        assert!(pkg.is_some());
        assert_eq!(pkg.unwrap().name, "httpd");

        let pkg = state.package_for_path(Path::new("/etc/ssh/sshd_config"));
        assert!(pkg.is_some());
        assert_eq!(pkg.unwrap().name, "sshd");
    }

    #[test]
    fn test_rpm_state_package_for_path_unknown() {
        let state = build_test_rpm_state();
        assert!(
            state
                .package_for_path(Path::new("/etc/nonexistent"))
                .is_none()
        );
    }

    #[test]
    fn test_rpm_state_empty() {
        let state = RpmState::default();
        assert!(state.packages().is_empty());
        assert!(state.owned_paths().is_empty());
        assert!(state.installed_packages().is_empty());
        assert!(state.verification_results().is_empty());
        assert!(state.module_streams().is_empty());
        assert!(!state.is_rpm_owned(Path::new("/etc/anything")));
        assert!(state.package_for_path(Path::new("/etc/anything")).is_none());
    }

    #[test]
    fn test_rpm_state_none_vs_empty() {
        // None (RPM failed) is distinguishable from Some(Default::default())
        // (RPM succeeded, no data). This is a contract test for Wave 2 dispatch.
        let none_state: Option<&RpmState> = None;
        let empty_state = RpmState::default();
        let some_empty: Option<&RpmState> = Some(&empty_state);

        // None means "RPM failed, no data at all"
        assert!(none_state.is_none());
        // Some(empty) means "RPM succeeded, confirmed no data"
        assert!(some_empty.is_some());
        assert!(some_empty.unwrap().packages().is_empty());
        assert!(some_empty.unwrap().owned_paths().is_empty());
    }
}
