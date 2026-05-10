pub mod classifier;
pub mod modules;
pub mod parser;
pub mod repos;

use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::system::SourceSystem;
use inspectah_core::types::warnings::Warning;
use std::collections::HashMap;

/// RPM query format string — matches Go's `%{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}`.
const RPM_QA_FORMAT: &str = "%{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}";

pub struct RpmInspector;

impl RpmInspector {
    pub fn new() -> Self {
        Self
    }

    /// Query all installed packages via `rpm -qa --queryformat`.
    fn query_packages(&self, exec: &dyn Executor) -> Vec<PackageEntry> {
        let format_arg = format!("{}\n", RPM_QA_FORMAT);
        let result = exec.run("rpm", &["-qa", "--queryformat", &format_arg]);
        if !result.success() {
            return Vec::new();
        }
        parser::parse_rpm_qa(&result.stdout)
    }

    /// Build baseline lookup from the source system context.
    /// For Phase 1: baseline is empty (no baseline = all Added).
    /// Full baseline subtraction from booted_image is Phase 2+.
    fn build_baseline(
        &self,
        _source: &SourceSystem,
        _rpm_state: &Option<inspectah_core::traits::inspector::RpmState>,
    ) -> HashMap<String, PackageEntry> {
        // Phase 1: no baseline subtraction
        HashMap::new()
    }

    /// Collect repo data, GPG keys, module streams, version locks, and
    /// optionally rpm -Va output (package-mode only).
    fn collect_supplementary(
        &self,
        exec: &dyn Executor,
        source: &SourceSystem,
        repo_files: &mut Vec<inspectah_core::types::rpm::RepoFile>,
        gpg_keys: &mut Vec<inspectah_core::types::rpm::RepoFile>,
        module_streams: &mut Vec<inspectah_core::types::rpm::EnabledModuleStream>,
        version_locks: &mut Vec<inspectah_core::types::rpm::VersionLockEntry>,
        rpm_va: &mut Vec<inspectah_core::types::rpm::RpmVaEntry>,
    ) {
        // Repo files
        *repo_files = repos::collect_repo_files(exec);

        // GPG keys from repo content
        for repo in repo_files.iter() {
            let keys = repos::extract_gpg_keys(&repo.content, exec);
            gpg_keys.extend(keys);
        }

        // Module streams + version locks
        *module_streams = modules::parse_module_streams(exec);
        *version_locks = modules::parse_version_locks(exec);

        // rpm -Va (package-mode only — verifies file integrity against RPM db)
        if matches!(source, SourceSystem::PackageBased { .. }) {
            let va_result = exec.run("rpm", &["-Va"]);
            // rpm -Va returns exit code > 0 when differences found — that's expected
            if !va_result.stdout.is_empty() {
                *rpm_va = modules::parse_rpm_va(&va_result.stdout);
            }
        }
    }
}

impl Default for RpmInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl Inspector for RpmInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Rpm
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[
            SourceSystemKind::PackageBased,
            SourceSystemKind::RpmOstree,
            SourceSystemKind::Bootc,
        ]
    }

    fn inspect(&self, ctx: &InspectionContext) -> Result<InspectorOutput, InspectorError> {
        let exec = ctx.executor.as_ref();

        // 1. Query packages
        let host_packages = self.query_packages(exec);
        if host_packages.is_empty() {
            return Err(InspectorError::Failed {
                reason: "rpm -qa returned no packages".into(),
            });
        }

        // 2. Build baseline and classify
        let baseline = self.build_baseline(&ctx.source, &ctx.rpm_state);
        let classified = classifier::classify_packages(&host_packages, &baseline);

        // 3. Split classified packages into added / base_image_only
        let (packages_added, base_image_only): (Vec<_>, Vec<_>) = classified
            .into_iter()
            .partition(|p| p.state != PackageState::BaseImageOnly);

        // 4. Collect supplementary data
        let mut repo_files = Vec::new();
        let mut gpg_keys = Vec::new();
        let mut module_streams = Vec::new();
        let mut version_locks = Vec::new();
        let mut rpm_va = Vec::new();

        self.collect_supplementary(
            exec,
            &ctx.source,
            &mut repo_files,
            &mut gpg_keys,
            &mut module_streams,
            &mut version_locks,
            &mut rpm_va,
        );

        // 5. Build warnings
        let mut warnings = Vec::new();
        let no_baseline = baseline.is_empty();
        if no_baseline {
            warnings.push(Warning {
                inspector: "rpm".into(),
                message: "no baseline available — all packages classified as added".into(),
                ..Default::default()
            });
        }

        // 6. Build RpmSection
        let section = RpmSection {
            packages_added,
            base_image_only,
            rpm_va,
            repo_files,
            gpg_keys,
            module_streams,
            version_locks,
            no_baseline,
            ..Default::default()
        };

        Ok(InspectorOutput {
            section: SectionData::Rpm(section),
            warnings,
            redaction_hints: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::types::os::OsRelease;

    fn test_os_release() -> OsRelease {
        OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        }
    }

    /// Build a MockExecutor with canned RPM data for inspector tests.
    fn build_rpm_mock_executor() -> MockExecutor {
        let rpm_qa_output = "\
0:bash-5.2.26-3.el9.x86_64
0:vim-enhanced-9.0.1592-1.el9.x86_64
0:httpd-2.4.57-5.el9.x86_64
(none):tzdata-2024a-1.el9.noarch
0:gpg-pubkey-fd431d51-4ae0493b.x86_64
";
        MockExecutor::new()
            .with_command(
                &format!("rpm -qa --queryformat {}\n", RPM_QA_FORMAT),
                ExecResult {
                    stdout: rpm_qa_output.into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/etc/yum.repos.d", vec!["redhat.repo", "epel.repo"])
            .with_file(
                "/etc/yum.repos.d/redhat.repo",
                "[rhel-9-baseos]\nname=RHEL 9 BaseOS\n",
            )
            .with_file(
                "/etc/yum.repos.d/epel.repo",
                "[epel]\nname=EPEL 9\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n",
            )
            .with_file(
                "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9",
                "-----BEGIN PGP PUBLIC KEY BLOCK-----\ntest-key-data\n",
            )
            .with_dir("/etc/dnf/modules.d", vec!["nodejs.module"])
            .with_file(
                "/etc/dnf/modules.d/nodejs.module",
                "name=nodejs\nstream=18\nprofiles=default\n",
            )
            // rpm -Va returns some verification diffs (package-mode only)
            .with_command(
                "rpm -Va",
                ExecResult {
                    stdout: "S.5....T.  c /etc/httpd/conf/httpd.conf\n".into(),
                    exit_code: 1, // rpm -Va returns non-zero when diffs found
                    ..Default::default()
                },
            )
    }

    #[test]
    fn test_rpm_inspector_trait() {
        let inspector = RpmInspector::new();
        assert_eq!(inspector.id(), InspectorId::Rpm);
        assert!(inspector
            .applicable_to()
            .contains(&SourceSystemKind::PackageBased));
        assert!(inspector
            .applicable_to()
            .contains(&SourceSystemKind::RpmOstree));
        assert!(inspector
            .applicable_to()
            .contains(&SourceSystemKind::Bootc));
    }

    #[test]
    fn test_rpm_inspector_produces_section_data() {
        let mock = build_rpm_mock_executor();
        let ctx = InspectionContext {
            executor: Box::new(mock),
            source: SourceSystem::PackageBased {
                os_release: test_os_release(),
            },
            rpm_state: None,
        };
        let output = RpmInspector::new().inspect(&ctx).unwrap();
        if let SectionData::Rpm(rpm) = &output.section {
            // gpg-pubkey filtered, 4 real packages remain — all Added (no baseline)
            assert_eq!(rpm.packages_added.len(), 4);
            assert!(rpm.base_image_only.is_empty());
            assert!(rpm.no_baseline);

            // Verify specific packages
            let names: Vec<&str> = rpm.packages_added.iter().map(|p| p.name.as_str()).collect();
            assert!(names.contains(&"bash"));
            assert!(names.contains(&"vim-enhanced"));
            assert!(names.contains(&"httpd"));
            assert!(names.contains(&"tzdata"));
            assert!(!names.contains(&"gpg-pubkey")); // filtered

            // All classified as Added
            assert!(rpm
                .packages_added
                .iter()
                .all(|p| p.state == PackageState::Added));

            // Supplementary data
            assert_eq!(rpm.repo_files.len(), 2);
            assert_eq!(rpm.gpg_keys.len(), 1);
            assert_eq!(rpm.module_streams.len(), 1);
            assert_eq!(rpm.module_streams[0].module_name, "nodejs");

            // rpm -Va collected for package-mode
            assert_eq!(rpm.rpm_va.len(), 1);
            assert_eq!(rpm.rpm_va[0].path, "/etc/httpd/conf/httpd.conf");
        } else {
            panic!("expected SectionData::Rpm");
        }

        // Should have a no-baseline warning
        assert!(output
            .warnings
            .iter()
            .any(|w| w.message.contains("no baseline")));
    }

    #[test]
    fn test_rpm_inspector_bootc_skips_rpm_va() {
        let rpm_qa_output = "0:bash-5.2.26-3.el9.x86_64\n";
        let mock = MockExecutor::new().with_command(
            &format!("rpm -qa --queryformat {}\n", RPM_QA_FORMAT),
            ExecResult {
                stdout: rpm_qa_output.into(),
                exit_code: 0,
                ..Default::default()
            },
        );
        let ctx = InspectionContext {
            executor: Box::new(mock),
            source: SourceSystem::Bootc {
                os_release: test_os_release(),
                booted_image: "registry.redhat.io/rhel9/rhel-bootc:9.4".into(),
                staged_image: None,
            },
            rpm_state: None,
        };
        let output = RpmInspector::new().inspect(&ctx).unwrap();
        if let SectionData::Rpm(rpm) = &output.section {
            assert!(rpm.rpm_va.is_empty(), "bootc should skip rpm -Va");
        } else {
            panic!("expected SectionData::Rpm");
        }
    }

    #[test]
    fn test_rpm_inspector_fails_on_empty_packages() {
        let mock = MockExecutor::new().with_command(
            &format!("rpm -qa --queryformat {}\n", RPM_QA_FORMAT),
            ExecResult {
                stdout: "".into(),
                exit_code: 0,
                ..Default::default()
            },
        );
        let ctx = InspectionContext {
            executor: Box::new(mock),
            source: SourceSystem::PackageBased {
                os_release: test_os_release(),
            },
            rpm_state: None,
        };
        let result = RpmInspector::new().inspect(&ctx);
        assert!(matches!(result, Err(InspectorError::Failed { .. })));
    }
}
