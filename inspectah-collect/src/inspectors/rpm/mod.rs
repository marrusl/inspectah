pub mod classifier;
pub mod modules;
pub mod parser;
pub mod repos;
pub mod source_repos;

use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::rpm::{FileOwnershipEntry, PackageEntry, PackageState, RpmSection};
use inspectah_core::types::system::SourceSystem;
use inspectah_core::types::warnings::Warning;
use std::collections::{HashMap, HashSet};

/// RPM query format string for NEVRA parsing.
const RPM_QA_FORMAT: &str = "%{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}";

/// RPM query format for file ownership — sentinel format.
///
/// `%{NAME}` is a scalar tag and cannot be inside `[...]` alongside
/// array tags (`%{FILENAMES}`). RPM requires all tags inside brackets
/// to be arrays of the same length, so mixing scalar + array produces
/// `error: incorrect format: array iterator used with different sized arrays`.
///
/// The sentinel format puts `%{NAME}` outside brackets as a `@@`-prefixed
/// header line, with `%{FILENAMES}` iterated inside brackets below it:
///
/// ```text
/// @@tzdata
/// /usr/share/doc/tzdata
/// /usr/share/doc/tzdata/NEWS
/// @@setup
/// /etc/bashrc
/// /etc/profile
/// ```
const RPM_FILE_OWNERSHIP_FORMAT: &str = "@@%{NAME}\\n[%{FILENAMES}\\n]";

struct SupplementaryData {
    repo_files: Vec<inspectah_core::types::rpm::RepoFile>,
    gpg_keys: Vec<inspectah_core::types::rpm::RepoFile>,
    module_streams: Vec<inspectah_core::types::rpm::EnabledModuleStream>,
    version_locks: Vec<inspectah_core::types::rpm::VersionLockEntry>,
    rpm_va: Vec<inspectah_core::types::rpm::RpmVaEntry>,
}

#[derive(Debug, Clone, PartialEq)]
struct LeafClassification {
    leaf_packages: Option<Vec<String>>,
    auto_packages: Option<Vec<String>>,
    leaf_dep_tree: serde_json::Value,
}

impl LeafClassification {
    fn authoritative(
        leaf_packages: Vec<String>,
        auto_packages: Vec<String>,
        leaf_dep_tree: serde_json::Value,
    ) -> Self {
        Self {
            leaf_packages: Some(leaf_packages),
            auto_packages: Some(auto_packages),
            leaf_dep_tree,
        }
    }

    fn unavailable() -> Self {
        Self {
            leaf_packages: None,
            auto_packages: None,
            leaf_dep_tree: empty_leaf_dep_tree(),
        }
    }
}

fn empty_leaf_dep_tree() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

fn canonical_package_id(pkg: &PackageEntry) -> String {
    format!("{}.{}", pkg.name, pkg.arch)
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
    /// Runs `rpm -qa --queryformat '@@%{NAME}\n[%{FILENAMES}\n]'` which
    /// produces a sentinel format: each package starts with a `@@name`
    /// header line, followed by its file paths (one per line). Groups
    /// results by package name into `FileOwnershipEntry` structs.
    fn query_file_ownership(&self, exec: &dyn Executor) -> Vec<FileOwnershipEntry> {
        let result = exec.run("rpm", &["-qa", "--queryformat", RPM_FILE_OWNERSHIP_FORMAT]);
        if !result.success() {
            return Vec::new();
        }

        let mut pkg_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut current_package: Option<String> = None;
        for line in result.stdout.lines() {
            if let Some(name) = line.strip_prefix("@@") {
                current_package = Some(name.to_string());
            } else if !line.is_empty()
                && let Some(ref pkg) = current_package
            {
                pkg_map
                    .entry(pkg.clone())
                    .or_default()
                    .push(line.to_string());
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

    fn inspect(
        &self,
        ctx: &InspectionContext<'_>,
        progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        use inspectah_core::types::progress::{MetricKind, ProgressEvent, StepId, StepOutcome};
        use std::time::Instant;

        let exec = ctx.executor;
        let inspector_id = InspectorId::Rpm;
        let inspector_start = Instant::now();

        // 1. Query packages
        progress.emit(ProgressEvent::StepStarted {
            inspector: inspector_id,
            step: StepId::QueryingPackages,
        });
        let step_start = Instant::now();
        let host_packages = self.query_packages(exec);
        if host_packages.is_empty() {
            return Err(InspectorError::Failed {
                reason: "rpm -qa returned no packages".into(),
            });
        }
        let elapsed = step_start.elapsed();
        eprintln!(
            "[timing] RPM query packages (rpm -qa): {:.1}s ({} packages)",
            elapsed.as_secs_f64(),
            host_packages.len()
        );
        progress.emit(ProgressEvent::Metric {
            inspector: inspector_id,
            kind: MetricKind::PackagesFound,
            value: host_packages.len(),
        });
        progress.emit(ProgressEvent::StepFinished {
            inspector: inspector_id,
            step: StepId::QueryingPackages,
            outcome: StepOutcome::Complete,
        });

        // 2. Build baseline and classify
        progress.emit(ProgressEvent::StepStarted {
            inspector: inspector_id,
            step: StepId::ClassifyingPackages,
        });
        let step_start = Instant::now();
        let baseline = self.build_baseline(ctx.baseline_data);
        let classification = classifier::classify_packages(&host_packages, &baseline);
        let version_changes = classification.version_changes;

        // 3. All classified host packages go to packages_added
        // (BaseImageOnly is no longer assigned to host packages by the classifier)
        let mut packages_added = classification.packages;

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
        let elapsed = step_start.elapsed();
        eprintln!(
            "[timing] RPM classify packages: {:.1}s ({} added, {} base-only)",
            elapsed.as_secs_f64(),
            packages_added.len(),
            base_image_only.len()
        );
        progress.emit(ProgressEvent::StepFinished {
            inspector: inspector_id,
            step: StepId::ClassifyingPackages,
            outcome: StepOutcome::Complete,
        });

        // 3b. Source repo attribution per added package.
        progress.emit(ProgressEvent::StepStarted {
            inspector: inspector_id,
            step: StepId::ResolvingSourceRepos,
        });
        let step_start = Instant::now();
        if !packages_added.is_empty() {
            source_repos::populate_source_repos(exec, &mut packages_added);
        }
        let repo_count = packages_added
            .iter()
            .map(|p| &p.source_repo)
            .filter(|r| !r.is_empty())
            .collect::<std::collections::HashSet<_>>()
            .len();
        let elapsed = step_start.elapsed();
        eprintln!(
            "[timing] RPM source repo resolution: {:.1}s ({} repos mapped)",
            elapsed.as_secs_f64(),
            repo_count
        );
        progress.emit(ProgressEvent::Metric {
            inspector: inspector_id,
            kind: MetricKind::ReposMapped,
            value: repo_count,
        });
        progress.emit(ProgressEvent::StepFinished {
            inspector: inspector_id,
            step: StepId::ResolvingSourceRepos,
            outcome: StepOutcome::Complete,
        });

        // 4. Compute baseline_suppressed from ALL packages_added (leaf + auto).
        // This runs at the inspector level so it covers every package, not just
        // the leaf subset, and survives degraded leaf classification.
        let baseline_suppressed: Option<Vec<String>> = ctx.baseline_data.map(|bl| {
            let mut suppressed: Vec<String> = packages_added
                .iter()
                .map(canonical_package_id)
                .filter(|id| bl.packages.contains_key(id))
                .collect();
            suppressed.sort();
            suppressed
        });

        // 5. Classify leaf vs auto packages (subtract baseline so dep trees
        //    only count genuinely new packages, not base-image residents).
        progress.emit(ProgressEvent::StepStarted {
            inspector: inspector_id,
            step: StepId::ResolvingDepTree,
        });
        let step_start = Instant::now();
        let baseline_name_set: HashSet<String> = ctx
            .baseline_data
            .map(|b| b.packages.keys().cloned().collect())
            .unwrap_or_default();
        let leaf_classification = classify_leaf_auto(exec, &packages_added, &baseline_name_set);
        let dep_tree_outcome = if leaf_classification.leaf_packages.is_none() {
            StepOutcome::Degraded {
                reason: "dependency classification unavailable".into(),
            }
        } else {
            StepOutcome::Complete
        };
        let elapsed = step_start.elapsed();
        eprintln!(
            "[timing] RPM dep tree resolution (dnf repoquery): {:.1}s ({} packages queried)",
            elapsed.as_secs_f64(),
            packages_added.len()
        );
        progress.emit(ProgressEvent::StepFinished {
            inspector: inspector_id,
            step: StepId::ResolvingDepTree,
            outcome: dep_tree_outcome,
        });

        // 6. Collect supplementary data
        progress.emit(ProgressEvent::StepStarted {
            inspector: inspector_id,
            step: StepId::VerifyingIntegrity,
        });
        let step_start = Instant::now();
        let supp = self.collect_supplementary(exec, ctx.source_system);
        let elapsed = step_start.elapsed();
        eprintln!(
            "[timing] RPM supplementary data (repos, modules, rpm -Va): {:.1}s",
            elapsed.as_secs_f64()
        );
        progress.emit(ProgressEvent::StepFinished {
            inspector: inspector_id,
            step: StepId::VerifyingIntegrity,
            outcome: StepOutcome::Complete,
        });

        // 7. Query file ownership for Wave 2 inspectors (sentinel format)
        progress.emit(ProgressEvent::StepStarted {
            inspector: inspector_id,
            step: StepId::MappingFileOwnership,
        });
        let step_start = Instant::now();
        let file_ownership = self.query_file_ownership(exec);
        let elapsed = step_start.elapsed();
        eprintln!(
            "[timing] RPM file ownership query: {:.1}s ({} packages mapped)",
            elapsed.as_secs_f64(),
            file_ownership.len()
        );
        progress.emit(ProgressEvent::StepFinished {
            inspector: inspector_id,
            step: StepId::MappingFileOwnership,
            outcome: StepOutcome::Complete,
        });

        eprintln!(
            "[timing] RPM inspector total: {:.1}s",
            inspector_start.elapsed().as_secs_f64()
        );

        // 8. Build baseline_package_names for downstream consumers
        let baseline_package_names = ctx.baseline_data.map(|b| {
            let mut names: Vec<String> = b
                .packages
                .values()
                .map(|pkg| pkg.name.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            names.sort();
            names
        });

        // 9. Build warnings
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

        // 10. Build RpmSection
        let section = RpmSection {
            packages_added,
            base_image_only,
            version_changes,
            rpm_va: supp.rpm_va,
            repo_files: supp.repo_files,
            gpg_keys: supp.gpg_keys,
            module_streams: supp.module_streams,
            version_locks: supp.version_locks,
            file_ownership,
            no_baseline,
            baseline_package_names,
            leaf_packages: leaf_classification.leaf_packages,
            auto_packages: leaf_classification.auto_packages,
            leaf_dep_tree: leaf_classification.leaf_dep_tree,
            baseline_suppressed,
            ..Default::default()
        };

        Ok(InspectorOutput {
            section: SectionData::Rpm(section),
            warnings,
            redaction_hints: Vec::new(),
        })
    }
}

/// Query `dnf repoquery --userinstalled` to get canonical `name.arch`
/// identities for explicitly installed packages. Returns `None` if dnf is
/// unavailable (non-zero exit).
fn query_user_installed(exec: &dyn Executor) -> Option<HashSet<String>> {
    let result = exec.run(
        "dnf",
        &[
            "repoquery",
            "--userinstalled",
            "--queryformat",
            "%{name}.%{arch}\n",
        ],
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
/// For each package in `added_ids`, queries its transitive dependencies as canonical
/// `name.arch` identities and filters to only those also in `added_ids`.
/// Returns `None` if dnf is unavailable or any package query fails after the initial probe.
fn classify_deps_dnf(
    exec: &dyn Executor,
    added_ids: &HashSet<String>,
) -> Option<HashMap<String, HashSet<String>>> {
    if added_ids.is_empty() {
        return Some(HashMap::new());
    }

    let mut package_ids: Vec<&String> = added_ids.iter().collect();
    package_ids.sort();

    // Probe with first package to check if dnf is available.
    let first = package_ids[0];
    let probe = exec.run(
        "dnf",
        &[
            "repoquery",
            "--requires",
            "--resolve",
            "--recursive",
            "--installed",
            "--queryformat",
            "%{name}.%{arch}\n",
            first,
        ],
    );
    if !probe.success() {
        return None;
    }

    let mut depends_on: HashMap<String, HashSet<String>> = HashMap::new();
    for package_id in added_ids {
        depends_on.insert(package_id.clone(), HashSet::new());
    }

    // Parse first result.
    parse_dnf_deps(&probe.stdout, first, added_ids, &mut depends_on);

    // Query remaining packages.
    for package_id in &package_ids[1..] {
        let result = exec.run(
            "dnf",
            &[
                "repoquery",
                "--requires",
                "--resolve",
                "--recursive",
                "--installed",
                "--queryformat",
                "%{name}.%{arch}\n",
                package_id,
            ],
        );
        if !result.success() {
            return None;
        }
        parse_dnf_deps(&result.stdout, package_id, added_ids, &mut depends_on);
    }

    Some(depends_on)
}

/// Classify `packages_added` into leaf (user-intent) and auto (transitive dependency)
/// sets using canonical `name.arch` identities. If dependency classification is
/// unavailable or incomplete, returns explicit degraded-mode metadata instead of
/// successful-looking fallback data.
///
/// Baseline suppression is handled at the inspector level (not here) so that ALL
/// packages_added — leaf and auto alike — are checked against the baseline, and
/// the suppressed set survives even when leaf classification degrades.
fn classify_leaf_auto(
    exec: &dyn Executor,
    packages_added: &[PackageEntry],
    baseline_names: &HashSet<String>,
) -> LeafClassification {
    let added_ids: HashSet<String> = packages_added.iter().map(canonical_package_id).collect();

    let user_installed = query_user_installed(exec);
    let Some(depends_on) = classify_deps_dnf(exec, &added_ids) else {
        return LeafClassification::unavailable();
    };

    let (mut leaf, mut auto): (Vec<String>, Vec<String>) = if let Some(ref ui) = user_installed {
        let leaf_set: HashSet<&String> = ui.intersection(&added_ids).collect();
        if leaf_set.is_empty() && !added_ids.is_empty() {
            // Fallback to graph-based when userinstalled has no overlap with added
            graph_based_split(&added_ids, &depends_on)
        } else {
            let mut l = Vec::new();
            let mut a = Vec::new();
            for package_id in &added_ids {
                if leaf_set.contains(package_id) {
                    l.push(package_id.clone());
                } else {
                    a.push(package_id.clone());
                }
            }
            (l, a)
        }
    } else {
        graph_based_split(&added_ids, &depends_on)
    };

    leaf.sort();
    auto.retain(|pkg| !baseline_names.contains(pkg));
    auto.sort();

    // Build per-leaf dep tree: for each leaf, list its auto dependencies.
    let auto_set: HashSet<&str> = auto.iter().map(|s| s.as_str()).collect();
    let mut dep_tree = serde_json::Map::new();
    for lf in &leaf {
        let mut filtered: Vec<String> = depends_on
            .get(lf)
            .map(|deps| {
                deps.iter()
                    .filter(|d| auto_set.contains(d.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        filtered.sort();
        dep_tree.insert(lf.clone(), serde_json::json!(filtered));
    }

    LeafClassification::authoritative(leaf, auto, serde_json::Value::Object(dep_tree))
}

/// Graph-based fallback: package identities depended on by other added packages are
/// auto; package identities not depended on by anything are leaf.
fn graph_based_split(
    added_ids: &HashSet<String>,
    depends_on: &HashMap<String, HashSet<String>>,
) -> (Vec<String>, Vec<String>) {
    let mut depended_on: HashSet<String> = HashSet::new();
    for deps in depends_on.values() {
        for dep in deps {
            if added_ids.contains(dep) {
                depended_on.insert(dep.clone());
            }
        }
    }
    let mut leaf = Vec::new();
    let mut auto = Vec::new();
    for package_id in added_ids {
        if depended_on.contains(package_id) {
            auto.push(package_id.clone());
        } else {
            leaf.push(package_id.clone());
        }
    }
    (leaf, auto)
}

/// Parse dnf dependency output lines and record which added package identity
/// `package_id` depends on.
fn parse_dnf_deps(
    stdout: &str,
    package_id: &str,
    added_ids: &HashSet<String>,
    depends_on: &mut HashMap<String, HashSet<String>>,
) {
    for line in stdout.lines() {
        let dep = line.trim();
        if dep.is_empty() || dep == package_id {
            continue;
        }
        if added_ids.contains(dep)
            && let Some(deps) = depends_on.get_mut(package_id)
        {
            deps.insert(dep.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::traits::progress::{NullProgress, VecProgress};
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::progress::{MetricKind, ProgressEvent, StepId, StepOutcome};

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
        // File ownership output: sentinel format (@@name header + paths).
        // Covers /etc (for owned_paths) and non-/etc (for completeness).
        let file_ownership_output = "\
@@bash
/etc/profile.d/bash_completion.sh
/usr/bin/bash
@@httpd
/etc/httpd/conf/httpd.conf
/etc/httpd/conf.d/ssl.conf
/usr/sbin/httpd
@@vim-enhanced
/usr/bin/vim
@@tzdata
/usr/share/zoneinfo/UTC
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
        assert!(
            inspector
                .applicable_to()
                .contains(&SourceSystemKind::PackageBased)
        );
        assert!(
            inspector
                .applicable_to()
                .contains(&SourceSystemKind::RpmOstree)
        );
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
        let output = RpmInspector::new().inspect(&ctx, &NullProgress).unwrap();
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
            assert!(
                rpm.packages_added
                    .iter()
                    .all(|p| p.state == PackageState::Added)
            );

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
        assert!(
            output
                .warnings
                .iter()
                .any(|w| w.message.contains("no baseline"))
        );
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
        let output = RpmInspector::new().inspect(&ctx, &NullProgress).unwrap();
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
        let result = RpmInspector::new().inspect(&ctx, &NullProgress);
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
        let output = RpmInspector::new().inspect(&ctx, &NullProgress).unwrap();

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
            !output
                .warnings
                .iter()
                .any(|w| w.message.contains("no baseline")),
            "should not warn about no baseline when baseline is provided"
        );
    }

    // --- query_user_installed tests ---

    #[test]
    fn query_user_installed_parses_canonical_ids() {
        let exec = MockExecutor::new().with_command(
            "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
            ExecResult {
                stdout: "vim.x86_64\nhtop.noarch\nnginx.aarch64\n".into(),
                exit_code: 0,
                stderr: String::new(),
            },
        );
        let result = query_user_installed(&exec);
        assert!(result.is_some());
        let names = result.unwrap();
        assert_eq!(names.len(), 3);
        assert!(names.contains("vim.x86_64"));
        assert!(names.contains("htop.noarch"));
        assert!(names.contains("nginx.aarch64"));
    }

    #[test]
    fn query_user_installed_returns_none_on_failure() {
        let exec = MockExecutor::new().with_command(
            "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
            ExecResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "dnf not found".into(),
            },
        );
        let result = query_user_installed(&exec);
        assert!(result.is_none());
    }

    // --- classify_leaf_auto tests ---

    fn build_leaf_classification_executor(rpm_qa_output: &str) -> MockExecutor {
        build_rpm_mock_executor().with_command(
            &format!("rpm -qa --queryformat {}\n", RPM_QA_FORMAT),
            ExecResult {
                stdout: rpm_qa_output.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
    }

    fn make_test_entry(name: &str, arch: &str) -> PackageEntry {
        PackageEntry {
            name: name.to_string(),
            arch: arch.to_string(),
            ..PackageEntry::default()
        }
    }

    #[test]
    fn classify_leaf_auto_uses_canonical_name_arch_identity() {
        let exec = MockExecutor::new()
            .with_command(
                "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
                ExecResult {
                    exit_code: 0,
                    stdout: "vim.x86_64\nsome-other-pkg.x86_64\n".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n glibc.x86_64",
                ExecResult {
                    exit_code: 0,
                    stdout: "".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n vim.x86_64",
                ExecResult {
                    exit_code: 0,
                    stdout: "glibc.x86_64\n".into(),
                    stderr: String::new(),
                },
            );

        let added = vec![
            make_test_entry("vim", "x86_64"),
            make_test_entry("glibc", "x86_64"),
        ];

        let classification = classify_leaf_auto(&exec, &added, &HashSet::new());

        assert_eq!(
            classification.leaf_packages,
            Some(vec!["vim.x86_64".to_string()])
        );
        assert_eq!(
            classification.auto_packages,
            Some(vec!["glibc.x86_64".to_string()])
        );
        let vim_deps = classification
            .leaf_dep_tree
            .get("vim.x86_64")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(vim_deps.len(), 1);
        assert_eq!(vim_deps[0].as_str().unwrap(), "glibc.x86_64");
    }

    #[test]
    fn classify_leaf_auto_falls_back_to_graph_when_userinstalled_has_no_overlap() {
        let exec = MockExecutor::new()
            .with_command(
                "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
                ExecResult {
                    exit_code: 0,
                    stdout: "unrelated-pkg.x86_64\n".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n glibc.x86_64",
                ExecResult {
                    exit_code: 0,
                    stdout: "".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n vim.x86_64",
                ExecResult {
                    exit_code: 0,
                    stdout: "glibc.x86_64\n".into(),
                    stderr: String::new(),
                },
            );

        let added = vec![
            make_test_entry("vim", "x86_64"),
            make_test_entry("glibc", "x86_64"),
        ];

        let classification = classify_leaf_auto(&exec, &added, &HashSet::new());

        assert_eq!(
            classification.leaf_packages,
            Some(vec!["vim.x86_64".to_string()])
        );
        assert_eq!(
            classification.auto_packages,
            Some(vec!["glibc.x86_64".to_string()])
        );
        assert_eq!(
            classification.leaf_dep_tree,
            serde_json::json!({"vim.x86_64": ["glibc.x86_64"]})
        );
    }

    #[test]
    fn rpm_inspector_marks_leaf_classification_unavailable_on_total_dnf_failure() {
        let exec = build_leaf_classification_executor(
            "\
0:glibc-2.34-100.el9.x86_64
0:vim-9.0.1592-1.el9.x86_64
",
        )
        .with_command(
            "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
            ExecResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "dnf not found".into(),
            },
        )
        .with_command(
            "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n glibc.x86_64",
            ExecResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "dnf not found".into(),
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

        let output = RpmInspector::new().inspect(&ctx, &NullProgress).unwrap();
        let SectionData::Rpm(rpm) = &output.section else {
            panic!("expected SectionData::Rpm");
        };

        assert_eq!(rpm.leaf_packages, None);
        assert_eq!(rpm.auto_packages, None);
        assert_eq!(rpm.leaf_dep_tree, serde_json::json!({}));
    }

    #[test]
    fn rpm_inspector_marks_leaf_classification_unavailable_on_late_repoquery_failure() {
        let exec = build_leaf_classification_executor(
            "\
0:glibc-2.34-100.el9.x86_64
0:vim-9.0.1592-1.el9.x86_64
",
        )
        .with_command(
            "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
            ExecResult {
                exit_code: 0,
                stdout: "vim.x86_64\n".into(),
                stderr: String::new(),
            },
        )
        .with_command(
            "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n glibc.x86_64",
            ExecResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        )
        .with_command(
            "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n vim.x86_64",
            ExecResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "repoquery failed".into(),
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

        let output = RpmInspector::new().inspect(&ctx, &NullProgress).unwrap();
        let SectionData::Rpm(rpm) = &output.section else {
            panic!("expected SectionData::Rpm");
        };

        assert_eq!(rpm.leaf_packages, None);
        assert_eq!(rpm.auto_packages, None);
        assert_eq!(rpm.leaf_dep_tree, serde_json::json!({}));
    }

    #[test]
    fn classify_leaf_auto_keeps_multiarch_packages_distinct() {
        let exec = MockExecutor::new()
            .with_command(
                "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
                ExecResult {
                    exit_code: 0,
                    stdout: "openssl.x86_64\n".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n glibc.x86_64",
                ExecResult {
                    exit_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n openssl.i686",
                ExecResult {
                    exit_code: 0,
                    stdout: "glibc.x86_64\n".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n openssl.x86_64",
                ExecResult {
                    exit_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                },
            );

        let added = vec![
            make_test_entry("openssl", "x86_64"),
            make_test_entry("openssl", "i686"),
            make_test_entry("glibc", "x86_64"),
        ];

        let classification = classify_leaf_auto(&exec, &added, &HashSet::new());

        assert_eq!(
            classification.leaf_packages,
            Some(vec!["openssl.x86_64".to_string()])
        );
        assert_eq!(
            classification.auto_packages,
            Some(vec!["glibc.x86_64".to_string(), "openssl.i686".to_string()])
        );
        assert_eq!(
            classification.leaf_dep_tree,
            serde_json::json!({"openssl.x86_64": []})
        );
    }

    // --- classify_deps_dnf tests ---

    #[test]
    fn classify_deps_dnf_builds_arch_aware_graph() {
        let exec = MockExecutor::new()
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n glibc.x86_64",
                ExecResult {
                    exit_code: 0,
                    stdout: "".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n vim.x86_64",
                ExecResult {
                    exit_code: 0,
                    stdout: "glibc.x86_64\nncurses.x86_64\n".into(),
                    stderr: String::new(),
                },
            );

        let added_names: HashSet<String> = ["vim.x86_64", "glibc.x86_64"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let deps = classify_deps_dnf(&exec, &added_names).expect("graph should be available");
        assert!(deps.get("vim.x86_64").unwrap().contains("glibc.x86_64"));
        assert!(!deps.get("vim.x86_64").unwrap().contains("ncurses.x86_64"));
    }

    #[test]
    fn classify_deps_dnf_returns_none_on_failure() {
        let exec = MockExecutor::new().with_command(
            "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n glibc.x86_64",
            ExecResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "dnf not found".into(),
            },
        );

        let added_names: HashSet<String> = ["vim.x86_64", "glibc.x86_64"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let deps = classify_deps_dnf(&exec, &added_names);
        assert!(deps.is_none());
    }

    #[test]
    fn query_user_installed_skips_blank_lines() {
        let exec = MockExecutor::new().with_command(
            "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
            ExecResult {
                exit_code: 0,
                stdout: "\nvim.x86_64\n\nhtop.noarch\n\n".into(),
                stderr: String::new(),
            },
        );
        let result = query_user_installed(&exec);
        assert!(result.is_some());
        let names = result.unwrap();
        assert_eq!(names.len(), 2);
        assert!(names.contains("vim.x86_64"));
        assert!(names.contains("htop.noarch"));
    }

    #[test]
    fn test_classify_leaf_auto_does_not_handle_baseline_suppression() {
        // After the refactor, classify_leaf_auto no longer takes a baseline
        // parameter and does not suppress baseline-present packages.
        // Baseline suppression is handled at the inspector level.
        let exec = MockExecutor::new()
            .with_command(
                "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
                ExecResult {
                    exit_code: 0,
                    stdout: "vim.x86_64\nkernel.x86_64\n".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n glibc.x86_64",
                ExecResult { exit_code: 0, stdout: "".into(), stderr: String::new() },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n kernel.x86_64",
                ExecResult { exit_code: 0, stdout: "".into(), stderr: String::new() },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n vim.x86_64",
                ExecResult { exit_code: 0, stdout: "glibc.x86_64\n".into(), stderr: String::new() },
            );

        let added = vec![
            make_test_entry("vim", "x86_64"),
            make_test_entry("kernel", "x86_64"),
            make_test_entry("glibc", "x86_64"),
        ];

        let classification = classify_leaf_auto(&exec, &added, &HashSet::new());

        // Both vim and kernel stay in leaf (empty baseline — no suppression)
        let mut expected_leaf = vec!["kernel.x86_64".to_string(), "vim.x86_64".to_string()];
        expected_leaf.sort();
        assert_eq!(classification.leaf_packages, Some(expected_leaf));
        assert_eq!(
            classification.auto_packages,
            Some(vec!["glibc.x86_64".to_string()])
        );
    }

    #[test]
    fn test_baseline_suppressed_includes_auto_packages() {
        use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};

        // Scenario:
        // - vim.x86_64 is user-installed (leaf)
        // - glibc.x86_64 is a dependency (auto)
        // - kernel.x86_64 is user-installed (leaf)
        // - baseline contains kernel AND glibc
        // Result: baseline_suppressed should contain BOTH kernel.x86_64
        //         AND glibc.x86_64 (not just kernel, which is the only
        //         one in the leaf set)
        let exec = build_leaf_classification_executor(
            "\
0:vim-9.0.1592-1.el9.x86_64
0:kernel-5.14.0-503.el9.x86_64
0:glibc-2.34-100.el9.x86_64
",
        )
        .with_command(
            "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
            ExecResult {
                exit_code: 0,
                stdout: "vim.x86_64\nkernel.x86_64\n".into(),
                stderr: String::new(),
            },
        )
        .with_command(
            "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n glibc.x86_64",
            ExecResult { exit_code: 0, stdout: "".into(), stderr: String::new() },
        )
        .with_command(
            "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n kernel.x86_64",
            ExecResult { exit_code: 0, stdout: "".into(), stderr: String::new() },
        )
        .with_command(
            "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n vim.x86_64",
            ExecResult { exit_code: 0, stdout: "glibc.x86_64\n".into(), stderr: String::new() },
        );

        let mut baseline_packages = HashMap::new();
        baseline_packages.insert(
            "kernel.x86_64".into(),
            BaselinePackageEntry {
                name: "kernel".into(),
                arch: "x86_64".into(),
                version: "5.14.0".into(),
                release: "362.el9".into(),
                epoch: Some("0".into()),
            },
        );
        baseline_packages.insert(
            "glibc.x86_64".into(),
            BaselinePackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                version: "2.34".into(),
                release: "100.el9".into(),
                epoch: Some("0".into()),
            },
        );

        let baseline_data = BaselineData {
            image_digest: "sha256:abc123".into(),
            packages: baseline_packages,
            extracted_at: "2026-01-01T00:00:00Z".into(),
        };

        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: Some(&baseline_data),
        };

        let output = RpmInspector::new().inspect(&ctx, &NullProgress).unwrap();
        let SectionData::Rpm(rpm) = &output.section else {
            panic!("expected SectionData::Rpm");
        };

        // Both kernel (leaf) AND glibc (auto) should be in baseline_suppressed
        let suppressed = rpm
            .baseline_suppressed
            .as_ref()
            .expect("baseline_suppressed should be Some");
        assert!(
            suppressed.contains(&"kernel.x86_64".to_string()),
            "baseline_suppressed should include leaf package kernel.x86_64"
        );
        assert!(
            suppressed.contains(&"glibc.x86_64".to_string()),
            "baseline_suppressed should include auto package glibc.x86_64"
        );
        assert_eq!(
            suppressed.len(),
            2,
            "exactly kernel + glibc should be suppressed"
        );
    }

    #[test]
    fn test_baseline_suppressed_survives_degraded_leaf_classification() {
        use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};

        // Scenario:
        // - dnf --userinstalled fails (leaf classification unavailable)
        // - dnf repoquery --requires also fails (full dnf failure)
        // - baseline data IS available
        // - Some packages are in the baseline
        // Result: leaf_packages is None, BUT baseline_suppressed is Some([...])
        let exec = build_leaf_classification_executor(
            "\
0:glibc-2.34-100.el9.x86_64
0:vim-9.0.1592-1.el9.x86_64
",
        )
        .with_command(
            "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
            ExecResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "dnf not found".into(),
            },
        )
        .with_command(
            "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n glibc.x86_64",
            ExecResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "dnf not found".into(),
            },
        );

        let mut baseline_packages = HashMap::new();
        baseline_packages.insert(
            "glibc.x86_64".into(),
            BaselinePackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                version: "2.34".into(),
                release: "100.el9".into(),
                epoch: Some("0".into()),
            },
        );

        let baseline_data = BaselineData {
            image_digest: "sha256:abc123".into(),
            packages: baseline_packages,
            extracted_at: "2026-01-01T00:00:00Z".into(),
        };

        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: Some(&baseline_data),
        };

        let output = RpmInspector::new().inspect(&ctx, &NullProgress).unwrap();
        let SectionData::Rpm(rpm) = &output.section else {
            panic!("expected SectionData::Rpm");
        };

        // Leaf classification degraded
        assert_eq!(
            rpm.leaf_packages, None,
            "leaf_packages should be None on dnf failure"
        );
        assert_eq!(
            rpm.auto_packages, None,
            "auto_packages should be None on dnf failure"
        );

        // But baseline_suppressed survives
        let suppressed = rpm
            .baseline_suppressed
            .as_ref()
            .expect("baseline_suppressed should be Some even when leaf classification fails");
        assert!(
            suppressed.contains(&"glibc.x86_64".to_string()),
            "glibc.x86_64 should be in baseline_suppressed"
        );
    }

    #[test]
    fn test_no_baseline_means_no_suppression() {
        let exec = MockExecutor::new()
            .with_command(
                "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
                ExecResult {
                    exit_code: 0,
                    stdout: "vim.x86_64\n".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n glibc.x86_64",
                ExecResult { exit_code: 0, stdout: "".into(), stderr: String::new() },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n vim.x86_64",
                ExecResult { exit_code: 0, stdout: "glibc.x86_64\n".into(), stderr: String::new() },
            );

        let added = vec![
            make_test_entry("vim", "x86_64"),
            make_test_entry("glibc", "x86_64"),
        ];

        // No baseline provided — auto includes all non-leaf added packages
        let classification = classify_leaf_auto(&exec, &added, &HashSet::new());
        assert_eq!(
            classification.leaf_packages,
            Some(vec!["vim.x86_64".to_string()])
        );
        assert_eq!(
            classification.auto_packages,
            Some(vec!["glibc.x86_64".to_string()])
        );
    }

    #[test]
    fn classify_leaf_auto_baseline_removes_auto_and_dep_tree_entries() {
        // Scenario: vim is leaf, glibc + ncurses are auto deps of vim,
        // but glibc is in the baseline. After filtering:
        // - auto_packages should NOT contain glibc
        // - dep tree for vim should NOT list glibc
        let exec = MockExecutor::new()
            .with_command(
                "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
                ExecResult {
                    exit_code: 0,
                    stdout: "vim.x86_64\n".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n glibc.x86_64",
                ExecResult { exit_code: 0, stdout: "".into(), stderr: String::new() },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n ncurses.x86_64",
                ExecResult { exit_code: 0, stdout: "".into(), stderr: String::new() },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n vim.x86_64",
                ExecResult {
                    exit_code: 0,
                    stdout: "glibc.x86_64\nncurses.x86_64\n".into(),
                    stderr: String::new(),
                },
            );

        let added = vec![
            make_test_entry("vim", "x86_64"),
            make_test_entry("glibc", "x86_64"),
            make_test_entry("ncurses", "x86_64"),
        ];

        let baseline: HashSet<String> = ["glibc.x86_64"].iter().map(|s| s.to_string()).collect();
        let classification = classify_leaf_auto(&exec, &added, &baseline);

        assert_eq!(
            classification.leaf_packages,
            Some(vec!["vim.x86_64".to_string()])
        );
        // glibc filtered out by baseline — only ncurses remains
        assert_eq!(
            classification.auto_packages,
            Some(vec!["ncurses.x86_64".to_string()])
        );
        // dep tree for vim should only include ncurses (glibc excluded)
        assert_eq!(
            classification.leaf_dep_tree,
            serde_json::json!({"vim.x86_64": ["ncurses.x86_64"]})
        );
    }

    #[test]
    fn test_baseline_package_names_use_plain_rpm_names() {
        use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};

        // Build baseline with canonical name.arch keys but plain names in entries
        let mut baseline_packages = HashMap::new();
        baseline_packages.insert(
            "firewalld.x86_64".into(),
            BaselinePackageEntry {
                name: "firewalld".into(),
                epoch: Some("0".into()),
                version: "1.3.4".into(),
                release: "1.el9".into(),
                arch: "x86_64".into(),
            },
        );
        baseline_packages.insert(
            "systemd.x86_64".into(),
            BaselinePackageEntry {
                name: "systemd".into(),
                epoch: Some("0".into()),
                version: "252.32".into(),
                release: "1.el9".into(),
                arch: "x86_64".into(),
            },
        );

        let baseline_data = BaselineData {
            image_digest: "sha256:test".into(),
            packages: baseline_packages,
            extracted_at: "2026-05-19T00:00:00Z".into(),
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

        let output = RpmInspector::new().inspect(&ctx, &NullProgress).unwrap();
        let SectionData::Rpm(rpm) = &output.section else {
            panic!("expected SectionData::Rpm");
        };

        // baseline_package_names should contain plain package names, not name.arch
        let baseline_names = rpm
            .baseline_package_names
            .as_ref()
            .expect("baseline_package_names should be Some");

        assert!(
            baseline_names.contains(&"firewalld".to_string()),
            "baseline_package_names should contain plain name 'firewalld'"
        );
        assert!(
            baseline_names.contains(&"systemd".to_string()),
            "baseline_package_names should contain plain name 'systemd'"
        );
        assert!(
            !baseline_names.iter().any(|name| name.contains('.')),
            "baseline_package_names should not contain any names with arch suffix (name.arch)"
        );
    }

    // --- query_file_ownership sentinel format tests ---

    #[test]
    fn test_query_file_ownership_sentinel_format_multi_file_packages() {
        // Verify the sentinel format parser handles multi-file packages,
        // single-file packages, and packages with only non-/etc paths.
        let sentinel_output = "\
@@setup
/etc/bashrc
/etc/profile
/etc/hosts
/etc/services
/usr/share/doc/setup/README
@@httpd
/etc/httpd/conf/httpd.conf
/etc/httpd/conf.d/ssl.conf
/usr/sbin/httpd
/usr/lib64/httpd/modules/mod_ssl.so
@@tzdata
/usr/share/zoneinfo/UTC
";
        let exec = MockExecutor::new().with_command(
            &format!("rpm -qa --queryformat {}", RPM_FILE_OWNERSHIP_FORMAT),
            ExecResult {
                stdout: sentinel_output.into(),
                exit_code: 0,
                ..Default::default()
            },
        );

        let inspector = RpmInspector::new();
        let entries = inspector.query_file_ownership(&exec);

        assert_eq!(entries.len(), 3, "should have 3 packages");

        let setup = entries.iter().find(|e| e.package_name == "setup");
        assert!(setup.is_some(), "setup package should exist");
        let setup_paths = &setup.unwrap().paths;
        assert_eq!(setup_paths.len(), 5, "setup should own 5 files");
        assert!(setup_paths.contains(&"/etc/bashrc".to_string()));
        assert!(setup_paths.contains(&"/etc/profile".to_string()));
        assert!(setup_paths.contains(&"/etc/hosts".to_string()));
        assert!(setup_paths.contains(&"/etc/services".to_string()));
        assert!(setup_paths.contains(&"/usr/share/doc/setup/README".to_string()));

        let httpd = entries.iter().find(|e| e.package_name == "httpd");
        assert!(httpd.is_some(), "httpd package should exist");
        let httpd_paths = &httpd.unwrap().paths;
        assert_eq!(httpd_paths.len(), 4, "httpd should own 4 files");
        assert!(httpd_paths.contains(&"/etc/httpd/conf/httpd.conf".to_string()));
        assert!(httpd_paths.contains(&"/etc/httpd/conf.d/ssl.conf".to_string()));

        let tzdata = entries.iter().find(|e| e.package_name == "tzdata");
        assert!(tzdata.is_some(), "tzdata package should exist");
        assert_eq!(tzdata.unwrap().paths.len(), 1, "tzdata should own 1 file");
    }

    #[test]
    fn test_query_file_ownership_empty_output() {
        let exec = MockExecutor::new().with_command(
            &format!("rpm -qa --queryformat {}", RPM_FILE_OWNERSHIP_FORMAT),
            ExecResult {
                stdout: "".into(),
                exit_code: 0,
                ..Default::default()
            },
        );

        let inspector = RpmInspector::new();
        let entries = inspector.query_file_ownership(&exec);
        assert!(entries.is_empty(), "empty output should produce no entries");
    }

    #[test]
    fn test_query_file_ownership_command_failure() {
        let exec = MockExecutor::new().with_command(
            &format!("rpm -qa --queryformat {}", RPM_FILE_OWNERSHIP_FORMAT),
            ExecResult {
                exit_code: 1,
                stderr: "rpm: command failed".into(),
                ..Default::default()
            },
        );

        let inspector = RpmInspector::new();
        let entries = inspector.query_file_ownership(&exec);
        assert!(
            entries.is_empty(),
            "failed command should produce no entries"
        );
    }

    // --- progress event tests ---

    #[test]
    fn test_rpm_inspector_emits_progress_events() {
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
        let progress = VecProgress::new();
        RpmInspector::new().inspect(&ctx, &progress).unwrap();

        let events = progress.events();

        // Verify all 6 steps started in order
        let step_ids: Vec<&StepId> = events
            .iter()
            .filter_map(|e| match e {
                ProgressEvent::StepStarted { step, .. } => Some(step),
                _ => None,
            })
            .collect();

        assert_eq!(
            step_ids,
            &[
                &StepId::QueryingPackages,
                &StepId::ClassifyingPackages,
                &StepId::ResolvingSourceRepos,
                &StepId::ResolvingDepTree,
                &StepId::VerifyingIntegrity,
                &StepId::MappingFileOwnership,
            ]
        );

        // Verify all 6 steps finished
        let finished_ids: Vec<&StepId> = events
            .iter()
            .filter_map(|e| match e {
                ProgressEvent::StepFinished { step, .. } => Some(step),
                _ => None,
            })
            .collect();
        assert_eq!(finished_ids.len(), 6);

        // Verify PackagesFound metric emitted
        assert!(events.iter().any(|e| matches!(
            e,
            ProgressEvent::Metric {
                kind: MetricKind::PackagesFound,
                ..
            }
        )));

        // Verify ReposMapped metric emitted
        assert!(events.iter().any(|e| matches!(
            e,
            ProgressEvent::Metric {
                kind: MetricKind::ReposMapped,
                ..
            }
        )));
    }

    #[test]
    fn test_rpm_degraded_dep_tree_emits_degraded_step() {
        // Build mock where dnf repoquery --userinstalled fails (exit code 1)
        // AND dnf repoquery --requires also fails — this causes
        // classify_leaf_auto to return leaf_packages: None.
        let exec = build_leaf_classification_executor(
            "\
0:glibc-2.34-100.el9.x86_64
0:vim-9.0.1592-1.el9.x86_64
",
        )
        .with_command(
            "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
            ExecResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "dnf not found".into(),
            },
        )
        .with_command(
            "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n glibc.x86_64",
            ExecResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "dnf not found".into(),
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

        let progress = VecProgress::new();
        RpmInspector::new().inspect(&ctx, &progress).unwrap();

        let events = progress.events();
        assert!(
            events.iter().any(|e| matches!(
                e,
                ProgressEvent::StepFinished {
                    step: StepId::ResolvingDepTree,
                    outcome: StepOutcome::Degraded { .. },
                    ..
                }
            )),
            "dep tree step should emit Degraded outcome when leaf classification is unavailable"
        );
    }
}
