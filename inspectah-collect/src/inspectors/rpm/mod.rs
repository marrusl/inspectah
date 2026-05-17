pub mod classifier;
pub mod modules;
pub mod parser;
pub mod repos;
pub mod source_repos;

use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::rpm::{FileOwnershipEntry, PackageEntry, PackageState, RpmSection};
use inspectah_core::types::system::SourceSystem;
use inspectah_core::types::warnings::Warning;
use std::collections::{HashMap, HashSet};

/// RPM query format string — matches Go's `%{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}`.
const RPM_QA_FORMAT: &str = "%{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}";

/// RPM query format for file ownership — produces `name\tpath` per owned file.
/// The `[]` brackets iterate the FILENAMES array tag.
const RPM_FILE_OWNERSHIP_FORMAT: &str = "[%{NAME}\\t%{FILENAMES}\\n]";

struct SupplementaryData {
    repo_files: Vec<inspectah_core::types::rpm::RepoFile>,
    gpg_keys: Vec<inspectah_core::types::rpm::RepoFile>,
    module_streams: Vec<inspectah_core::types::rpm::EnabledModuleStream>,
    version_locks: Vec<inspectah_core::types::rpm::VersionLockEntry>,
    rpm_va: Vec<inspectah_core::types::rpm::RpmVaEntry>,
}

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

    /// Build baseline lookup from extracted baseline data.
    ///
    /// Converts `BaselinePackageEntry` (core types) to the classifier's
    /// `PackageEntry` format, keyed by `name.arch` for O(1) lookup.
    ///
    /// When `baseline` is `None`, returns an empty HashMap (all packages
    /// classified as Added — preserves Phase 1 behavior).
    fn build_baseline(
        &self,
        baseline: Option<&inspectah_core::baseline::BaselineData>,
    ) -> HashMap<String, PackageEntry> {
        let baseline = match baseline {
            Some(b) => b,
            None => return HashMap::new(),
        };

        baseline
            .packages
            .values()
            .map(|bp| {
                let key = format!("{}.{}", bp.name, bp.arch);
                let pkg = PackageEntry {
                    name: bp.name.clone(),
                    epoch: bp.epoch.clone().unwrap_or_default(),
                    version: bp.version.clone(),
                    release: bp.release.clone(),
                    arch: bp.arch.clone(),
                    state: PackageState::BaseImageOnly,
                    include: false,
                    ..Default::default()
                };
                (key, pkg)
            })
            .collect()
    }

    /// Query file ownership for all installed packages.
    ///
    /// Runs `rpm -qa --queryformat '[%{NAME}\t%{FILENAMES}\n]'` to produce
    /// `package_name\tfilepath` per line. Groups results by package name
    /// into `FileOwnershipEntry` structs. Matches Go's `BuildRpmOwnedPaths`
    /// but retains per-package attribution for `path_to_package`.
    fn query_file_ownership(&self, exec: &dyn Executor) -> Vec<FileOwnershipEntry> {
        let result = exec.run("rpm", &["-qa", "--queryformat", RPM_FILE_OWNERSHIP_FORMAT]);
        if !result.success() {
            return Vec::new();
        }

        let mut pkg_map: HashMap<String, Vec<String>> = HashMap::new();
        for line in result.stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some((name, path)) = line.split_once('\t') {
                let name = name.trim();
                let path = path.trim();
                if !name.is_empty() && !path.is_empty() {
                    pkg_map
                        .entry(name.to_string())
                        .or_default()
                        .push(path.to_string());
                }
            }
        }

        pkg_map
            .into_iter()
            .map(|(package_name, paths)| FileOwnershipEntry {
                package_name,
                paths,
            })
            .collect()
    }

    fn collect_supplementary(
        &self,
        exec: &dyn Executor,
        source: &SourceSystem,
    ) -> SupplementaryData {
        let repo_files = repos::collect_repo_files(exec);

        let mut gpg_keys = Vec::new();
        for repo in &repo_files {
            gpg_keys.extend(repos::extract_gpg_keys(&repo.content, exec));
        }

        let module_streams = modules::parse_module_streams(exec);
        let version_locks = modules::parse_version_locks(exec);

        let rpm_va = if matches!(source, SourceSystem::PackageBased { .. }) {
            let va_result = exec.run("rpm", &["-Va"]);
            if va_result.stdout.is_empty() {
                Vec::new()
            } else {
                modules::parse_rpm_va(&va_result.stdout)
            }
        } else {
            Vec::new()
        };

        SupplementaryData {
            repo_files,
            gpg_keys,
            module_streams,
            version_locks,
            rpm_va,
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

    fn inspect(&self, ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
        let exec = ctx.executor;

        // 1. Query packages
        let host_packages = self.query_packages(exec);
        if host_packages.is_empty() {
            return Err(InspectorError::Failed {
                reason: "rpm -qa returned no packages".into(),
            });
        }

        // 2. Build baseline and classify
        let baseline = self.build_baseline(ctx.baseline_data);
        let classified = classifier::classify_packages(&host_packages, &baseline);

        // 3. All classified host packages go to packages_added
        // (BaseImageOnly is no longer assigned to host packages by the classifier)
        let mut packages_added = classified;

        // 3a. Build base_image_only from baseline entries not found on host
        let host_keys: std::collections::HashSet<String> = packages_added
            .iter()
            .map(|p| format!("{}.{}", p.name, p.arch))
            .collect();
        let base_image_only: Vec<PackageEntry> = match ctx.baseline_data {
            Some(bl) => bl
                .packages
                .iter()
                .filter(|(key, _)| !host_keys.contains(key.as_str()))
                .map(|(_, bp)| PackageEntry {
                    name: bp.name.clone(),
                    epoch: bp.epoch.clone().unwrap_or_default(),
                    version: bp.version.clone(),
                    release: bp.release.clone(),
                    arch: bp.arch.clone(),
                    state: PackageState::BaseImageOnly,
                    include: false,
                    ..Default::default()
                })
                .collect(),
            None => Vec::new(),
        };

        // 3b. Source repo attribution per added package (matches Go Step 2b).
        if !packages_added.is_empty() {
            source_repos::populate_source_repos(exec, &mut packages_added);
        }

        // 4. Collect supplementary data
        let supp = self.collect_supplementary(exec, ctx.source_system);

        // 5. Query file ownership for Wave 2 inspectors
        let file_ownership = self.query_file_ownership(exec);

        // 6. Build baseline_package_names for Go snapshot backward compat
        let baseline_package_names = ctx.baseline_data.map(|b| {
            b.packages.keys().cloned().collect::<Vec<_>>()
        });

        // 7. Build warnings
        let mut warnings = Vec::new();
        let no_baseline = ctx.baseline_data.is_none();
        if no_baseline {
            warnings.push(Warning {
                inspector: "rpm".into(),
                message: "no baseline available — all packages classified as added".into(),
                ..Default::default()
            });
        }
        if file_ownership.is_empty() {
            warnings.push(Warning {
                inspector: "rpm".into(),
                message: "rpm file ownership query returned no data — \
                          RPM-owned file detection unavailable for Wave 2 inspectors"
                    .into(),
                ..Default::default()
            });
        }

        // 8. Build RpmSection
        let section = RpmSection {
            packages_added,
            base_image_only,
            rpm_va: supp.rpm_va,
            repo_files: supp.repo_files,
            gpg_keys: supp.gpg_keys,
            module_streams: supp.module_streams,
            version_locks: supp.version_locks,
            file_ownership,
            no_baseline,
            baseline_package_names,
            ..Default::default()
        };

        Ok(InspectorOutput {
            section: SectionData::Rpm(section),
            warnings,
            redaction_hints: Vec::new(),
        })
    }
}

/// Query `dnf repoquery --userinstalled` to get the set of user-explicitly-installed
/// package names. Returns `None` if dnf is unavailable (non-zero exit).
fn query_user_installed(exec: &dyn Executor) -> Option<HashSet<String>> {
    let result = exec.run(
        "dnf",
        &["repoquery", "--userinstalled", "--queryformat", "%{name}\n"],
    );
    if !result.success() {
        return None;
    }
    let names: HashSet<String> = result
        .stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    Some(names)
}

/// Build a dependency graph from `dnf repoquery --requires --resolve --recursive --installed`.
/// For each package in `added_names`, queries its transitive dependencies and filters to
/// only those also in `added_names`. Returns `(depends_on, ok)` where `ok` is false if
/// dnf is unavailable.
fn classify_deps_dnf(
    exec: &dyn Executor,
    added_names: &HashSet<String>,
) -> (HashMap<String, HashSet<String>>, bool) {
    if added_names.is_empty() {
        return (HashMap::new(), true);
    }

    let mut name_list: Vec<&String> = added_names.iter().collect();
    name_list.sort();

    // Probe with first package to check if dnf is available.
    let first = name_list[0];
    let probe = exec.run(
        "dnf",
        &[
            "repoquery",
            "--requires",
            "--resolve",
            "--recursive",
            "--installed",
            "--queryformat",
            "%{name}\n",
            first,
        ],
    );
    if !probe.success() {
        return (HashMap::new(), false);
    }

    let mut depends_on: HashMap<String, HashSet<String>> = HashMap::new();
    for name in added_names {
        depends_on.insert(name.clone(), HashSet::new());
    }

    // Parse first result.
    parse_dnf_deps(&probe.stdout, first, added_names, &mut depends_on);

    // Query remaining packages.
    for pkg_name in &name_list[1..] {
        let result = exec.run(
            "dnf",
            &[
                "repoquery",
                "--requires",
                "--resolve",
                "--recursive",
                "--installed",
                "--queryformat",
                "%{name}\n",
                pkg_name,
            ],
        );
        if result.success() {
            parse_dnf_deps(&result.stdout, pkg_name, added_names, &mut depends_on);
        }
    }

    (depends_on, true)
}

/// Parse dnf dependency output lines and record which added packages `pkg_name` depends on.
fn parse_dnf_deps(
    stdout: &str,
    pkg_name: &str,
    added_names: &HashSet<String>,
    depends_on: &mut HashMap<String, HashSet<String>>,
) {
    for line in stdout.lines() {
        let dep = line.trim();
        if dep.is_empty() || dep == pkg_name {
            continue;
        }
        if added_names.contains(dep) {
            if let Some(deps) = depends_on.get_mut(pkg_name) {
                deps.insert(dep.to_string());
            }
        }
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
        // File ownership output: package_name\tfilepath per line.
        // Covers /etc (for owned_paths) and non-/etc (for completeness).
        let file_ownership_output = "\
bash\t/etc/profile.d/bash_completion.sh
bash\t/usr/bin/bash
httpd\t/etc/httpd/conf/httpd.conf
httpd\t/etc/httpd/conf.d/ssl.conf
httpd\t/usr/sbin/httpd
vim-enhanced\t/usr/bin/vim
tzdata\t/usr/share/zoneinfo/UTC
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
            .with_command(
                &format!("rpm -qa --queryformat {}", RPM_FILE_OWNERSHIP_FORMAT),
                ExecResult {
                    stdout: file_ownership_output.into(),
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
        assert!(inspector.applicable_to().contains(&SourceSystemKind::Bootc));
    }

    #[test]
    fn test_rpm_inspector_produces_section_data() {
        let exec = build_rpm_mock_executor();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
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

            // File ownership collected
            assert!(
                !rpm.file_ownership.is_empty(),
                "file_ownership should be populated"
            );
            let httpd_ownership = rpm
                .file_ownership
                .iter()
                .find(|e| e.package_name == "httpd");
            assert!(
                httpd_ownership.is_some(),
                "httpd should have ownership data"
            );
            let httpd_paths = &httpd_ownership.unwrap().paths;
            assert!(httpd_paths.contains(&"/etc/httpd/conf/httpd.conf".to_string()));
            assert!(httpd_paths.contains(&"/etc/httpd/conf.d/ssl.conf".to_string()));
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
        let exec = MockExecutor::new().with_command(
            &format!("rpm -qa --queryformat {}\n", RPM_QA_FORMAT),
            ExecResult {
                stdout: rpm_qa_output.into(),
                exit_code: 0,
                ..Default::default()
            },
        );
        let source = SourceSystem::Bootc {
            os_release: test_os_release(),
            booted_image: "registry.redhat.io/rhel9/rhel-bootc:9.4".into(),
            staged_image: None,
        };
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
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
        let exec = MockExecutor::new().with_command(
            &format!("rpm -qa --queryformat {}\n", RPM_QA_FORMAT),
            ExecResult {
                stdout: "".into(),
                exit_code: 0,
                ..Default::default()
            },
        );
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };
        let result = RpmInspector::new().inspect(&ctx);
        assert!(matches!(result, Err(InspectorError::Failed { .. })));
    }

    // --- build_baseline tests ---

    #[test]
    fn test_build_baseline_none_returns_empty() {
        let inspector = RpmInspector::new();
        let result = inspector.build_baseline(None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_build_baseline_converts_baseline_data() {
        use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};

        let mut packages = std::collections::HashMap::new();
        packages.insert(
            "bash".to_string(),
            BaselinePackageEntry {
                name: "bash".to_string(),
                epoch: Some("0".to_string()),
                version: "5.2.26".to_string(),
                release: "3.el9".to_string(),
                arch: "x86_64".to_string(),
            },
        );
        packages.insert(
            "kernel".to_string(),
            BaselinePackageEntry {
                name: "kernel".to_string(),
                epoch: None,
                version: "5.14.0".to_string(),
                release: "503.el9".to_string(),
                arch: "x86_64".to_string(),
            },
        );

        let baseline_data = BaselineData {
            image_digest: "sha256:abc123".to_string(),
            packages,
            extracted_at: "2026-05-17T00:00:00Z".to_string(),
        };

        let inspector = RpmInspector::new();
        let result = inspector.build_baseline(Some(&baseline_data));

        assert_eq!(result.len(), 2);

        // bash keyed by name.arch
        let bash = result.get("bash.x86_64").expect("bash.x86_64 should exist");
        assert_eq!(bash.name, "bash");
        assert_eq!(bash.epoch, "0");
        assert_eq!(bash.version, "5.2.26");
        assert_eq!(bash.release, "3.el9");
        assert_eq!(bash.state, PackageState::BaseImageOnly);
        assert!(!bash.include);

        // kernel with None epoch -> empty string
        let kernel = result
            .get("kernel.x86_64")
            .expect("kernel.x86_64 should exist");
        assert_eq!(kernel.name, "kernel");
        assert_eq!(kernel.epoch, "");
        assert_eq!(kernel.version, "5.14.0");
        assert_eq!(kernel.state, PackageState::BaseImageOnly);
        assert!(!kernel.include);
    }

    #[test]
    fn test_rpm_inspector_with_baseline_classifies_correctly() {
        use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};

        // Baseline has bash and vim-enhanced at specific versions
        // Keys use name.arch format (matching real baseline extractor output)
        let mut packages = std::collections::HashMap::new();
        packages.insert(
            "bash.x86_64".to_string(),
            BaselinePackageEntry {
                name: "bash".to_string(),
                epoch: Some("0".to_string()),
                version: "5.2.26".to_string(),
                release: "3.el9".to_string(),
                arch: "x86_64".to_string(),
            },
        );
        packages.insert(
            "vim-enhanced.x86_64".to_string(),
            BaselinePackageEntry {
                name: "vim-enhanced".to_string(),
                epoch: Some("0".to_string()),
                version: "9.0.1592".to_string(),
                release: "1.el9".to_string(),
                arch: "x86_64".to_string(),
            },
        );

        let baseline_data = BaselineData {
            image_digest: "sha256:abc123".to_string(),
            packages,
            extracted_at: "2026-05-17T00:00:00Z".to_string(),
        };

        let exec = build_rpm_mock_executor();
        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: Some(&baseline_data),
        };
        let output = RpmInspector::new().inspect(&ctx).unwrap();

        if let SectionData::Rpm(rpm) = &output.section {
            // All host packages stay in packages_added (same-EVR = Added, not BaseImageOnly)
            assert_eq!(rpm.packages_added.len(), 4);
            let added_names: Vec<&str> =
                rpm.packages_added.iter().map(|p| p.name.as_str()).collect();
            assert!(added_names.contains(&"bash"));
            assert!(added_names.contains(&"vim-enhanced"));
            assert!(added_names.contains(&"httpd"));
            assert!(added_names.contains(&"tzdata"));

            // base_image_only: baseline packages NOT on host — both baseline
            // packages (bash, vim-enhanced) ARE on the host, so this is empty
            assert!(
                rpm.base_image_only.is_empty(),
                "all baseline packages are on host, so base_image_only should be empty"
            );

            // no_baseline should be false (we have baseline data)
            assert!(
                !rpm.no_baseline,
                "no_baseline should be false when baseline is provided"
            );
        } else {
            panic!("expected SectionData::Rpm");
        }

        // Should NOT have the no-baseline warning
        assert!(
            !output.warnings.iter().any(|w| w.message.contains("no baseline")),
            "should not warn about no baseline when baseline is provided"
        );
    }

    // --- query_user_installed tests ---

    #[test]
    fn query_user_installed_parses_dnf_output() {
        let exec = MockExecutor::new().with_command(
            "dnf repoquery --userinstalled --queryformat %{name}\n",
            ExecResult {
                exit_code: 0,
                stdout: "vim\nhtop\nnginx\n".into(),
                stderr: String::new(),
            },
        );
        let result = query_user_installed(&exec);
        assert!(result.is_some());
        let names = result.unwrap();
        assert_eq!(names.len(), 3);
        assert!(names.contains("vim"));
        assert!(names.contains("htop"));
        assert!(names.contains("nginx"));
    }

    #[test]
    fn query_user_installed_returns_none_on_failure() {
        let exec = MockExecutor::new().with_command(
            "dnf repoquery --userinstalled --queryformat %{name}\n",
            ExecResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "dnf not found".into(),
            },
        );
        let result = query_user_installed(&exec);
        assert!(result.is_none());
    }

    // --- classify_deps_dnf tests ---

    #[test]
    fn classify_deps_dnf_builds_graph() {
        // vim depends on glibc (which is also in added_names) and ncurses (not in added_names)
        let exec = MockExecutor::new()
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}\n glibc",
                ExecResult {
                    exit_code: 0,
                    stdout: "".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}\n vim",
                ExecResult {
                    exit_code: 0,
                    stdout: "glibc\nncurses\n".into(),
                    stderr: String::new(),
                },
            );

        let added_names: HashSet<String> = ["vim", "glibc"].iter().map(|s| s.to_string()).collect();
        let (deps, ok) = classify_deps_dnf(&exec, &added_names);
        assert!(ok);
        // vim depends on glibc (glibc is in added_names)
        assert!(deps.get("vim").unwrap().contains("glibc"));
        // ncurses is NOT in added_names, so not tracked
        assert!(!deps.get("vim").unwrap().contains("ncurses"));
    }

    #[test]
    fn classify_deps_dnf_returns_false_on_failure() {
        // sorted order: glibc comes first, probe fails
        let exec = MockExecutor::new().with_command(
            "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}\n glibc",
            ExecResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "dnf not found".into(),
            },
        );

        let added_names: HashSet<String> =
            ["vim", "glibc"].iter().map(|s| s.to_string()).collect();
        let (deps, ok) = classify_deps_dnf(&exec, &added_names);
        assert!(!ok);
        assert!(deps.is_empty());
    }

    #[test]
    fn query_user_installed_skips_blank_lines() {
        let exec = MockExecutor::new().with_command(
            "dnf repoquery --userinstalled --queryformat %{name}\n",
            ExecResult {
                exit_code: 0,
                stdout: "\nvim\n\nhtop\n\n".into(),
                stderr: String::new(),
            },
        );
        let result = query_user_installed(&exec);
        assert!(result.is_some());
        let names = result.unwrap();
        assert_eq!(names.len(), 2);
        assert!(names.contains("vim"));
        assert!(names.contains("htop"));
    }
}
