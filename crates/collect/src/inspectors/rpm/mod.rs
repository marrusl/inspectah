use inspectah_core::types::FindingKind;
pub mod classifier;
pub mod modules;
pub mod parser;
pub mod repoless;
pub mod repos;
mod rich_dep;
pub mod source_repos;

use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::rpm::{
    FileOwnershipEntry, InstalledGroup, PackageEntry, PackageState, RpmSection,
};
use inspectah_core::types::system::SourceSystem;
use inspectah_core::types::warnings::Warning;
use rich_dep::{DepExpr, OrChoice, Providers};
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
                    disposition: FindingKind::excluded(),
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

        let exec = ctx.executor;
        let inspector_id = InspectorId::Rpm;

        // 1. Query packages
        progress.emit(ProgressEvent::StepStarted {
            inspector: inspector_id,
            step: StepId::QueryingPackages,
        });
        let host_packages = self.query_packages(exec);
        if host_packages.is_empty() {
            return Err(InspectorError::Failed {
                reason: "rpm -qa returned no packages".into(),
            });
        }
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
                    disposition: FindingKind::excluded(),
                    ..Default::default()
                })
                .collect(),
            None => Vec::new(),
        };
        progress.emit(ProgressEvent::StepFinished {
            inspector: inspector_id,
            step: StepId::ClassifyingPackages,
            outcome: StepOutcome::Complete,
        });

        // 3b. Compute baseline_suppressed from ALL packages_added BEFORE
        // filtering. This runs at the inspector level so it covers every
        // package (leaf + auto), and survives degraded leaf classification.
        let baseline_suppressed: Option<Vec<String>> = ctx.baseline_data.map(|bl| {
            let mut suppressed: Vec<String> = packages_added
                .iter()
                .map(canonical_package_id)
                .filter(|id| bl.packages.contains_key(id))
                .collect();
            suppressed.sort();
            suppressed
        });

        // 3c. Build baseline_name_set and filter packages_added to the delta.
        // Base-image packages come from known repos and their dep trees are
        // resolved implicitly by DNF's --recursive --installed flag — querying
        // them explicitly is pure waste (500 dnf invocations vs ~50).
        let baseline_name_set: HashSet<String> = ctx
            .baseline_data
            .map(|b| b.packages.keys().cloned().collect())
            .unwrap_or_default();
        if !baseline_name_set.is_empty() {
            packages_added.retain(|p| !baseline_name_set.contains(&canonical_package_id(p)));
        }
        // 3d. Source repo attribution per added package (delta only).
        progress.emit(ProgressEvent::StepStarted {
            inspector: inspector_id,
            step: StepId::ResolvingSourceRepos,
        });
        if !packages_added.is_empty() {
            source_repos::populate_source_repos(exec, &mut packages_added);
        }
        let repo_count = packages_added
            .iter()
            .map(|p| &p.source_repo)
            .filter(|r| !r.is_empty())
            .collect::<std::collections::HashSet<_>>()
            .len();
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

        // 3e. Query file ownership for Wave 2 inspectors (sentinel format).
        //     Runs before the dependency tree, which resolves file-path
        //     requirements (`/usr/bin/perl`) to their owners from it.
        progress.emit(ProgressEvent::StepStarted {
            inspector: inspector_id,
            step: StepId::MappingFileOwnership,
        });
        let file_ownership = self.query_file_ownership(exec);
        progress.emit(ProgressEvent::StepFinished {
            inspector: inspector_id,
            step: StepId::MappingFileOwnership,
            outcome: StepOutcome::Complete,
        });

        // 4. Classify leaf vs auto packages (delta only — subtract baseline
        //    so dep trees only count genuinely new packages).
        progress.emit(ProgressEvent::StepStarted {
            inspector: inspector_id,
            step: StepId::ResolvingDepTree,
        });
        let leaf_classification =
            classify_leaf_auto(exec, &packages_added, &baseline_name_set, &file_ownership);
        let dep_tree_outcome = if leaf_classification.leaf_packages.is_none() {
            StepOutcome::Degraded {
                reason: "dependency classification unavailable".into(),
            }
        } else {
            StepOutcome::Complete
        };
        progress.emit(ProgressEvent::StepFinished {
            inspector: inspector_id,
            step: StepId::ResolvingDepTree,
            outcome: dep_tree_outcome,
        });

        // 5. Collect supplementary data
        progress.emit(ProgressEvent::StepStarted {
            inspector: inspector_id,
            step: StepId::VerifyingIntegrity,
        });
        let supp = self.collect_supplementary(exec, ctx.source_system);
        progress.emit(ProgressEvent::StepFinished {
            inspector: inspector_id,
            step: StepId::VerifyingIntegrity,
            outcome: StepOutcome::Complete,
        });

        // 7. Build baseline_package_names for downstream consumers
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

        // 8. Collect installed dnf groups
        let mut installed_groups = collect_installed_groups(exec);

        // 8a. Filter group members to only installed packages
        if let Some(ref mut groups) = installed_groups {
            let installed_names: HashSet<&str> =
                host_packages.iter().map(|p| p.name.as_str()).collect();
            for group in groups.iter_mut() {
                group
                    .members
                    .retain(|name| installed_names.contains(name.as_str()));
            }
        }

        // 9. Build warnings
        let mut warnings = Vec::new();
        if file_ownership.is_empty() {
            warnings.push(Warning {
                inspector: "rpm".into(),
                message: "rpm file ownership query returned no data — \
                          RPM-owned file detection unavailable for Wave 2 inspectors"
                    .into(),
                ..Default::default()
            });
        }

        // 9a. Normalize source_repo short names to full repo IDs so
        //     package tables and config trees use the same identifier.
        source_repos::normalize_source_repos(&mut packages_added, &supp.repo_files);

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
            baseline_package_names,
            leaf_packages: leaf_classification.leaf_packages,
            auto_packages: leaf_classification.auto_packages,
            leaf_dep_tree: leaf_classification.leaf_dep_tree,
            baseline_suppressed,
            installed_groups,
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

fn collect_installed_groups(exec: &dyn Executor) -> Option<Vec<InstalledGroup>> {
    let result = exec.run("env", &["LC_ALL=C", "dnf", "group", "list", "--installed"]);
    if result.exit_code != 0 {
        return None;
    }

    let mut group_names = Vec::new();
    let mut in_installed = false;
    for line in result.stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Installed") {
            in_installed = true;
            continue;
        }
        if trimmed.starts_with("Available") || trimmed.is_empty() {
            if in_installed {
                break;
            }
            continue;
        }
        if in_installed && !trimmed.is_empty() {
            group_names.push(trimmed.to_string());
        }
    }

    if group_names.is_empty() {
        return Some(Vec::new());
    }

    // Batch all group names into one `dnf group info` call.
    let mut args: Vec<&str> = vec!["LC_ALL=C", "dnf", "group", "info"];
    args.extend(group_names.iter().map(|s| s.as_str()));
    let info_result = exec.run("env", &args);
    if info_result.exit_code != 0 {
        return Some(Vec::new());
    }

    // Parse the combined output by splitting on `Group:` headers.
    let groups = parse_batched_group_info(&info_result.stdout);

    Some(groups)
}

fn parse_group_info_packages(stdout: &str) -> Vec<String> {
    let mut packages = Vec::new();
    let mut in_package_section = false;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Mandatory Packages:")
            || trimmed.starts_with("Default Packages:")
            || trimmed.starts_with("Optional Packages:")
        {
            in_package_section = true;
            continue;
        }
        if trimmed.is_empty() || trimmed.ends_with(':') {
            in_package_section = false;
            continue;
        }
        if in_package_section {
            let name = trimmed.trim_start_matches("  ");
            if !name.is_empty() {
                packages.push(name.to_string());
            }
        }
    }
    packages.sort();
    packages.dedup();
    packages
}

/// Parse batched `dnf group info group1 group2 ...` output into
/// per-group `InstalledGroup` structs by splitting on `Group:` headers.
///
/// DNF outputs each group block starting with `Group: <name>`, followed
/// by Description, package lists, etc. We split on these headers and
/// feed each block to `parse_group_info_packages`.
fn parse_batched_group_info(stdout: &str) -> Vec<InstalledGroup> {
    let mut groups = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_block = String::new();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("Group:") {
            // Flush previous group.
            if let Some(prev_name) = current_name.take() {
                let packages = parse_group_info_packages(&current_block);
                groups.push(InstalledGroup {
                    name: prev_name,
                    members: packages,
                    ..Default::default()
                });
            }
            current_name = Some(name.trim().to_string());
            current_block.clear();
            continue;
        }
        if current_name.is_some() {
            current_block.push_str(line);
            current_block.push('\n');
        }
    }
    // Flush last group.
    if let Some(name) = current_name {
        let packages = parse_group_info_packages(&current_block);
        groups.push(InstalledGroup {
            name,
            members: packages,
            ..Default::default()
        });
    }

    groups
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

/// Result of dependency graph construction, carrying whether the graph
/// already contains transitive closure (DNF `--recursive`) or only
/// direct dependencies (rpm `-qR`).
struct DepGraphResult {
    depends_on: HashMap<String, HashSet<String>>,
    transitive: bool,
}

/// RPM query format for the dependencies of each added package — sentinel
/// format, one tagged line per dependency.
///
/// Uses the same `@@name` sentinel pattern as `RPM_FILE_OWNERSHIP_FORMAT`.
/// Each dependency tag is an array, so it goes inside its own `[...]`;
/// arrays of different lengths cannot share one. The leading tag says
/// which relation the line records (`RelationTag`):
///
/// ```text
/// @@dnf
/// R<TAB>/bin/sh
/// R<TAB>python3-dnf
/// W<TAB>(python3-dbus if NetworkManager)
/// @@httpd
/// R<TAB>/etc/mime.types
/// W<TAB>mod_http2
/// ```
///
/// A package with no entries for a tag prints nothing for that group,
/// measured on rpm 4.14.3 (UBI 8) and 4.16.1.3 (UBI 9), so a header is
/// never glued to a `(none)` marker.
///
/// Suggests (`%{SUGGESTNAME}`) and its reverse, Enhances, are left out on
/// purpose: dnf does not install them by default, so they do not explain
/// why a package is on the host.
const RPM_DEPS_FORMAT: &str = "@@%{NAME}\\n\
                               [R\\t%{REQUIRENAME}\\n]\
                               [W\\t%{RECOMMENDNAME}\\n]\
                               [S\\t%{SUPPLEMENTNAME}\\n]";

/// RPM query format for the installed provides index: one line per
/// provided capability, `name.arch<TAB>capability`.
///
/// `%{PROVIDENAME}` is an array tag. `%{=NAME}` and `%{=ARCH}` use the `=`
/// modifier, which repeats a scalar for every array element; without it rpm
/// rejects the mix of scalar and array tags inside `[...]` (see
/// `RPM_FILE_OWNERSHIP_FORMAT`). The identity comes first because it never
/// contains a tab, so each line splits on its first tab.
///
/// ```text
/// glibc.x86_64<TAB>libc.so.6()(64bit)
/// glibc.x86_64<TAB>glibc(x86-64)
/// openssl-libs.x86_64<TAB>libssl.so.3()(64bit)
/// ```
const RPM_PROVIDES_INDEX_FORMAT: &str = "[%{=NAME}.%{=ARCH}\\t%{PROVIDENAME}\\n]";

/// How a dependency line relates its package to the capability it names.
///
/// All three pull a package in on a default dnf install: rhel-bootc 9 and
/// 10 run dnf with `install_weak_deps` in effect, so Recommends and
/// Supplements are installed alongside hard requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Relation {
    /// `Requires`: the package needs a provider of the capability.
    Requires,
    /// `Recommends`: dnf installs a provider of the capability too.
    Recommends,
    /// `Supplements`: the reverse of Recommends. The package is installed
    /// when the capability is, so the capability's providers pull it in.
    Supplements,
}

impl Relation {
    /// The relation a `RPM_DEPS_FORMAT` line tag stands for.
    fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "R" => Some(Self::Requires),
            "W" => Some(Self::Recommends),
            "S" => Some(Self::Supplements),
            _ => None,
        }
    }
}

/// One package's dependencies, parsed from `RPM_DEPS_FORMAT` output.
type PackageDeps = (String, Vec<(Relation, DepExpr)>);

/// Build a dependency graph from three `rpm` queries and the file
/// ownership the inspector already collected.
///
/// Queries all packages' dependencies (requires, recommends, supplements)
/// in a single batched `rpm -q --queryformat` call using sentinel headers,
/// then resolves capabilities to provider packages through one `rpm -qa`
/// provides index, and file-path capabilities through `file_ownership`
/// (`ProviderIndex`).
///
/// An edge `a -> b` means installing `a` with dnf defaults installs `b`:
/// `a` requires or recommends something `b` provides, or `b` supplements
/// something `a` provides.
///
/// Returns direct-only deps (caller must walk the graph for transitive closure).
/// Returns `None` if either query fails.
fn classify_deps_rpm(
    exec: &dyn Executor,
    added_ids: &HashSet<String>,
    file_ownership: &[FileOwnershipEntry],
) -> Option<HashMap<String, HashSet<String>>> {
    if added_ids.is_empty() {
        return Some(HashMap::new());
    }

    let mut package_ids: Vec<&String> = added_ids.iter().collect();
    package_ids.sort();

    let mut depends_on: HashMap<String, HashSet<String>> = HashMap::new();
    for package_id in added_ids {
        depends_on.insert(package_id.clone(), HashSet::new());
    }

    // Build name→canonical-id map for sentinel header → package_id lookup.
    let name_to_ids: HashMap<&str, Vec<&String>> = {
        let mut map: HashMap<&str, Vec<&String>> = HashMap::new();
        for pid in &package_ids {
            map.entry(name_from_id(pid)).or_default().push(pid);
        }
        map
    };

    // Batch all package names into one rpm query using sentinel format.
    let pkg_names: Vec<&str> = package_ids.iter().map(|id| name_from_id(id)).collect();
    let mut args: Vec<&str> = vec!["-q", "--queryformat", RPM_DEPS_FORMAT];
    args.extend(&pkg_names);

    let result = exec.run("rpm", &args);
    if !result.success() {
        return None;
    }

    let per_pkg_deps = parse_sentinel_deps(&result.stdout);

    // Every capability named anywhere, conditions included, resolved in
    // one pass over the index.
    let wanted: HashSet<&str> = per_pkg_deps
        .iter()
        .flat_map(|(_, deps)| deps.iter().flat_map(|(_, dep)| dep.caps()))
        .collect();
    let index = ProviderIndex::build(exec, &wanted, file_ownership)?;

    for (pkg_name, deps) in &per_pkg_deps {
        let Some(ids) = name_to_ids.get(pkg_name.as_str()) else {
            continue;
        };
        for package_id in ids {
            for (relation, dep) in deps {
                match relation {
                    Relation::Requires | Relation::Recommends => {
                        for prov_id in dep.pulled(&index, OrChoice::First) {
                            if added_ids.contains(prov_id)
                                && name_from_id(prov_id) != *pkg_name
                                && let Some(set) = depends_on.get_mut(package_id.as_str())
                            {
                                set.insert(prov_id.to_string());
                            }
                        }
                    }
                    Relation::Supplements => {
                        if !dep.satisfied(&index) {
                            continue;
                        }
                        for prov_id in dep.pulled(&index, OrChoice::All) {
                            if added_ids.contains(prov_id)
                                && name_from_id(prov_id) != *pkg_name
                                && let Some(set) = depends_on.get_mut(prov_id)
                            {
                                set.insert((*package_id).clone());
                            }
                        }
                    }
                }
            }
        }
    }

    Some(depends_on)
}

/// Parse batched `RPM_DEPS_FORMAT` output into per-package dependency
/// lists. `rpmlib(...)` requirements name rpm features, not packages, and
/// are skipped; so is a rich expression `DepExpr::parse` cannot read.
fn parse_sentinel_deps(stdout: &str) -> Vec<PackageDeps> {
    let mut results: Vec<PackageDeps> = Vec::new();

    for line in stdout.lines() {
        if let Some(name) = line.trim().strip_prefix("@@") {
            results.push((name.to_string(), Vec::new()));
            continue;
        }
        let Some((_, deps)) = results.last_mut() else {
            continue;
        };
        let Some((tag, dep)) = line.split_once('\t') else {
            continue;
        };
        let Some(relation) = Relation::from_tag(tag) else {
            continue;
        };
        let dep = dep.trim();
        if dep.is_empty() || dep.starts_with("rpmlib(") {
            continue;
        }
        if let Some(expr) = DepExpr::parse(dep)
            && !deps.contains(&(relation, expr.clone()))
        {
            deps.push((relation, expr));
        }
    }

    results
}

/// Extract the package name from a canonical `name.arch` identity.
fn name_from_id(id: &str) -> &str {
    id.rsplit_once('.').map_or(id, |(name, _)| name)
}

/// Filter `rpm -qR` output to usable capability names.
///
/// Skips lines starting with `rpmlib(` or `/`, and takes only the first
/// whitespace-separated field (capability name without version constraints).
#[cfg(test)]
fn filter_capabilities(stdout: &str) -> Vec<String> {
    let mut caps = Vec::new();
    for line in stdout.lines() {
        let cap = line.trim();
        if cap.is_empty() || cap.starts_with("rpmlib(") || cap.starts_with('/') {
            continue;
        }
        // Take the first field (before any whitespace version constraint).
        let name = cap.split_whitespace().next().unwrap_or(cap);
        if !name.is_empty() {
            caps.push(name.to_string());
        }
    }
    caps.sort();
    caps.dedup();
    caps
}

/// The installed providers of the capabilities a dependency graph needs.
///
/// Built from one `rpm -qa` provides index plus the file ownership the
/// inspector already collected, so resolving costs one process however
/// many capabilities there are. Each capability maps to its own providers
/// only. Resolving a batch of capabilities with
/// `rpm -q --whatprovides cap1 cap2 ...` cannot do that: the output is a
/// flat list of NEVRAs with nothing saying which capability a line
/// answers, and crediting every provider in a batch to every capability in
/// it made unrelated packages depend on each other.
///
/// Capabilities are matched by name, as `--whatprovides <name>` matches
/// them; version constraints were already stripped. A file-path
/// capability (`/usr/bin/perl`) is provided by a package that lists it in
/// its provides (bash provides `/bin/sh`) or owns the file, the two places
/// `--whatprovides` looks. Paths are compared in their `/usr` form, since
/// a package built without usrmerge records `/bin/...` where a requirement
/// may name `/usr/bin/...`, or the other way round.
///
/// Providers are every installed package, base image included, so a rich
/// dependency's condition can be checked against the whole host; the
/// caller filters edges to the added set.
struct ProviderIndex {
    providers: HashMap<String, HashSet<String>>,
}

impl ProviderIndex {
    /// Look up every capability in `wanted`. Returns `None` if the provides
    /// index query fails, so classification falls back to dnf rather than
    /// reading as a graph with no edges.
    fn build(
        exec: &dyn Executor,
        wanted: &HashSet<&str>,
        file_ownership: &[FileOwnershipEntry],
    ) -> Option<Self> {
        let mut providers: HashMap<String, HashSet<String>> = HashMap::new();
        if wanted.is_empty() {
            return Some(Self { providers });
        }

        let result = exec.run("rpm", &["-qa", "--queryformat", RPM_PROVIDES_INDEX_FORMAT]);
        if !result.success() {
            return None;
        }

        // Wanted paths keyed by their `/usr` form, back to the spelling the
        // dependency used.
        let mut wanted_paths: HashMap<String, Vec<&str>> = HashMap::new();
        for cap in wanted.iter().filter(|c| c.starts_with('/')) {
            wanted_paths.entry(usr_form(cap)).or_default().push(cap);
        }

        // File ownership is keyed by package name, so map names back to
        // canonical ids. Every package provides at least its own name, so
        // the index lists every installed id.
        let mut ids_by_name: HashMap<&str, HashSet<&str>> = HashMap::new();
        for line in result.stdout.lines() {
            let Some((provider_id, cap)) = line.split_once('\t') else {
                continue;
            };
            ids_by_name
                .entry(name_from_id(provider_id))
                .or_default()
                .insert(provider_id);
            let cap = cap.trim();
            let mut credit = |cap: &str| {
                providers
                    .entry(cap.to_string())
                    .or_default()
                    .insert(provider_id.to_string());
            };
            if wanted.contains(cap) {
                credit(cap);
            }
            if cap.starts_with('/') {
                for spelling in wanted_paths.get(&usr_form(cap)).into_iter().flatten() {
                    credit(spelling);
                }
            }
        }

        if !wanted_paths.is_empty() {
            for entry in file_ownership {
                let Some(ids) = ids_by_name.get(entry.package_name.as_str()) else {
                    continue;
                };
                for path in &entry.paths {
                    for spelling in wanted_paths.get(&usr_form(path)).into_iter().flatten() {
                        providers
                            .entry((*spelling).to_string())
                            .or_default()
                            .extend(ids.iter().map(|id| (*id).to_string()));
                    }
                }
            }
        }

        Some(Self { providers })
    }
}

impl Providers for ProviderIndex {
    fn providers(&self, cap: &str) -> HashSet<&str> {
        self.providers
            .get(cap)
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect()
    }
}

/// The `/usr` form of a path, so `/bin/sh` and `/usr/bin/sh` compare equal.
fn usr_form(path: &str) -> String {
    if crate::rpm_ownership::USRMERGE_LEGACY_PREFIXES
        .iter()
        .any(|p| path.starts_with(p))
    {
        format!("/usr{path}")
    } else {
        path.to_string()
    }
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
    file_ownership: &[FileOwnershipEntry],
) -> LeafClassification {
    let added_ids: HashSet<String> = packages_added.iter().map(canonical_package_id).collect();

    let user_installed = query_user_installed(exec);

    // Try rpm-based dep resolution first (fast), fall back to DNF (slow).
    let graph = if let Some(rpm_deps) = classify_deps_rpm(exec, &added_ids, file_ownership) {
        DepGraphResult {
            depends_on: rpm_deps,
            transitive: false,
        }
    } else if let Some(dnf_deps) = classify_deps_dnf(exec, &added_ids) {
        DepGraphResult {
            depends_on: dnf_deps,
            transitive: true,
        }
    } else {
        return LeafClassification::unavailable();
    };

    let depends_on = &graph.depends_on;

    let (mut leaf, mut auto): (Vec<String>, Vec<String>) = if let Some(ref ui) = user_installed {
        let leaf_set: HashSet<&String> = ui.intersection(&added_ids).collect();
        if leaf_set.is_empty() && !added_ids.is_empty() {
            // Fallback to graph-based when userinstalled has no overlap with added
            graph_based_split(&added_ids, depends_on)
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
        graph_based_split(&added_ids, depends_on)
    };

    leaf.sort();
    auto.retain(|pkg| !baseline_names.contains(pkg));
    auto.sort();

    // Build per-leaf dep tree: for each leaf, list its auto dependencies.
    let auto_set: HashSet<&str> = auto.iter().map(|s| s.as_str()).collect();
    let mut dep_tree = serde_json::Map::new();

    if graph.transitive {
        // DNF --recursive already gave transitive closure.
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
    } else {
        // rpm gives only direct deps; walk the graph (BFS) for transitive closure.
        for lf in &leaf {
            let mut reachable: HashSet<String> = HashSet::new();
            let mut stack: Vec<String> = depends_on
                .get(lf)
                .map(|deps| deps.iter().cloned().collect())
                .unwrap_or_default();

            while let Some(dep) = stack.pop() {
                if reachable.contains(&dep) {
                    continue;
                }
                reachable.insert(dep.clone());
                if let Some(next_deps) = depends_on.get(&dep) {
                    for next in next_deps {
                        if !reachable.contains(next) {
                            stack.push(next.clone());
                        }
                    }
                }
            }

            let mut filtered: Vec<String> = reachable
                .into_iter()
                .filter(|d| auto_set.contains(d.as_str()))
                .collect();
            filtered.sort();
            dep_tree.insert(lf.clone(), serde_json::json!(filtered));
        }
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
        assert!(!bash.disposition.is_included());

        // kernel with None epoch -> empty string
        let kernel = result
            .get("kernel.x86_64")
            .expect("kernel.x86_64 should exist");
        assert_eq!(kernel.name, "kernel");
        assert_eq!(kernel.epoch, "");
        assert_eq!(kernel.version, "5.14.0");
        assert_eq!(kernel.state, PackageState::BaseImageOnly);
        assert!(!kernel.disposition.is_included());
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
            // Baseline packages (bash, vim-enhanced) are filtered out of
            // packages_added — only the delta (httpd, tzdata) remains.
            assert_eq!(rpm.packages_added.len(), 2);
            let added_names: Vec<&str> =
                rpm.packages_added.iter().map(|p| p.name.as_str()).collect();
            assert!(added_names.contains(&"httpd"));
            assert!(added_names.contains(&"tzdata"));
            assert!(
                !added_names.contains(&"bash"),
                "bash is in baseline, should be filtered from packages_added"
            );
            assert!(
                !added_names.contains(&"vim-enhanced"),
                "vim-enhanced is in baseline, should be filtered from packages_added"
            );

            // base_image_only: baseline packages NOT on host — both baseline
            // packages (bash, vim-enhanced) ARE on the host, so this is empty
            assert!(
                rpm.base_image_only.is_empty(),
                "all baseline packages are on host, so base_image_only should be empty"
            );

            // baseline_suppressed should list the filtered-out packages
            let suppressed = rpm
                .baseline_suppressed
                .as_ref()
                .expect("baseline_suppressed should be Some");
            assert!(suppressed.contains(&"bash.x86_64".to_string()));
            assert!(suppressed.contains(&"vim-enhanced.x86_64".to_string()));
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

    /// Regression test for the baseline filter bug (commit e324d65).
    ///
    /// Before the fix, `packages_added` contained ALL host packages (including
    /// baseline overlap) when passed to `populate_source_repos` and
    /// `classify_leaf_auto` → `classify_deps_dnf`. This caused expensive DNF
    /// queries on base-image packages that don't need resolution.
    ///
    /// The guard works by stubbing `dnf repoquery --requires --resolve` for
    /// ONLY the 3 delta packages. If someone reverts the `.retain()` filter,
    /// `classify_deps_dnf` will probe `bash.x86_64` first (alphabetically
    /// sorted), find no stub, get exit_code 127, return `None`, and the whole
    /// leaf classification degrades — `leaf_packages` becomes `None`.
    #[test]
    fn test_baseline_filter_prevents_dnf_queries_on_base_packages() {
        use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};

        // 8 host packages: 5 overlap with baseline, 3 are delta-only.
        let rpm_qa_output = "\
0:bash-5.2.26-3.el9.x86_64
0:glibc-2.34-60.el9.x86_64
0:kernel-5.14.0-503.el9.x86_64
0:coreutils-8.32-34.el9.x86_64
0:vim-enhanced-9.0.1592-1.el9.x86_64
0:httpd-2.4.57-5.el9.x86_64
0:nodejs-18.19.0-1.el9.x86_64
0:redis-7.0.12-1.el9.x86_64
";
        let file_ownership_output = "\
@@bash
/etc/profile.d/bash_completion.sh
@@httpd
/etc/httpd/conf/httpd.conf
";

        // Baseline: the 5 packages that overlap with host.
        let mut packages = std::collections::HashMap::new();
        for (name, ver, rel) in [
            ("bash", "5.2.26", "3.el9"),
            ("glibc", "2.34", "60.el9"),
            ("kernel", "5.14.0", "503.el9"),
            ("coreutils", "8.32", "34.el9"),
            ("vim-enhanced", "9.0.1592", "1.el9"),
        ] {
            packages.insert(
                format!("{name}.x86_64"),
                BaselinePackageEntry {
                    name: name.to_string(),
                    epoch: Some("0".to_string()),
                    version: ver.to_string(),
                    release: rel.to_string(),
                    arch: "x86_64".to_string(),
                },
            );
        }

        let baseline_data = BaselineData {
            image_digest: "sha256:regression-test".to_string(),
            packages,
            extracted_at: "2026-05-29T00:00:00Z".to_string(),
        };

        // Build executor with stubs for ONLY the 3 delta packages' dep queries.
        // If the baseline filter is reverted, classify_deps_dnf will probe
        // "bash.x86_64" first (alphabetical sort), find no stub (exit 127),
        // and return None — making leaf_packages None (Degraded).
        let exec = MockExecutor::new()
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
            .with_dir("/etc/yum.repos.d", vec!["redhat.repo"])
            .with_file(
                "/etc/yum.repos.d/redhat.repo",
                "[rhel-9-baseos]\nname=RHEL 9 BaseOS\n",
            )
            .with_dir("/etc/dnf/modules.d", vec![])
            .with_command(
                "rpm -Va",
                ExecResult {
                    exit_code: 0,
                    ..Default::default()
                },
            )
            // dnf repoquery --userinstalled: only delta packages are user-installed
            .with_command(
                "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
                ExecResult {
                    exit_code: 0,
                    stdout: "httpd.x86_64\nnodejs.x86_64\nredis.x86_64\n".into(),
                    stderr: String::new(),
                },
            )
            // dnf repoquery --requires --resolve: stubs for the 3 delta packages ONLY.
            // httpd is alphabetically first among delta — this is the probe.
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n httpd.x86_64",
                ExecResult {
                    exit_code: 0,
                    stdout: "".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n nodejs.x86_64",
                ExecResult {
                    exit_code: 0,
                    stdout: "".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                "dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}.%{arch}\n redis.x86_64",
                ExecResult {
                    exit_code: 0,
                    stdout: "".into(),
                    stderr: String::new(),
                },
            )
            // Source repo attribution: probe is first package alphabetically (httpd),
            // then remaining in a single batch.
            .with_command(
                "dnf repoquery --installed --queryformat %{name} %{from_repo}\n httpd",
                ExecResult {
                    exit_code: 0,
                    stdout: "httpd rhel-9-appstream\n".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                "dnf repoquery --installed --queryformat %{name} %{from_repo}\n nodejs redis",
                ExecResult {
                    exit_code: 0,
                    stdout: "nodejs rhel-9-appstream\nredis epel\n".into(),
                    stderr: String::new(),
                },
            );

        let source = SourceSystem::PackageBased {
            os_release: test_os_release(),
        };
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            baseline_data: Some(&baseline_data),
            rpm_state: None,
        };
        let output = RpmInspector::new().inspect(&ctx, &NullProgress).unwrap();

        if let SectionData::Rpm(rpm) = &output.section {
            // Primary regression guard: leaf_packages is Some (not degraded).
            // If .retain() is reverted, classify_deps_dnf probes "bash.x86_64"
            // (no stub) → exit 127 → returns None → leaf_packages = None.
            assert!(
                rpm.leaf_packages.is_some(),
                "leaf_packages should be Some (dep tree completed); \
                 None means DNF queried base packages without stubs — \
                 baseline filter regression"
            );

            // packages_added should contain only the 3 delta packages.
            assert_eq!(
                rpm.packages_added.len(),
                3,
                "expected 3 delta packages, got {}",
                rpm.packages_added.len()
            );
            let added_names: Vec<&str> =
                rpm.packages_added.iter().map(|p| p.name.as_str()).collect();
            assert!(added_names.contains(&"httpd"));
            assert!(added_names.contains(&"nodejs"));
            assert!(added_names.contains(&"redis"));

            // baseline_suppressed should list the 5 overlapping base packages.
            let suppressed = rpm
                .baseline_suppressed
                .as_ref()
                .expect("baseline_suppressed should be Some");
            assert_eq!(
                suppressed.len(),
                5,
                "expected 5 suppressed base packages, got {}",
                suppressed.len()
            );
            assert!(suppressed.contains(&"bash.x86_64".to_string()));
            assert!(suppressed.contains(&"glibc.x86_64".to_string()));
            assert!(suppressed.contains(&"kernel.x86_64".to_string()));
            assert!(suppressed.contains(&"coreutils.x86_64".to_string()));
            assert!(suppressed.contains(&"vim-enhanced.x86_64".to_string()));

            // Leaf/auto classification should only reference delta packages.
            if let Some(ref leaf) = rpm.leaf_packages {
                for pkg_id in leaf {
                    assert!(
                        !pkg_id.starts_with("bash.")
                            && !pkg_id.starts_with("glibc.")
                            && !pkg_id.starts_with("kernel.")
                            && !pkg_id.starts_with("coreutils.")
                            && !pkg_id.starts_with("vim-enhanced."),
                        "leaf_packages should not reference base package: {pkg_id}"
                    );
                }
            }
            if let Some(ref auto) = rpm.auto_packages {
                for pkg_id in auto {
                    assert!(
                        !pkg_id.starts_with("bash.")
                            && !pkg_id.starts_with("glibc.")
                            && !pkg_id.starts_with("kernel.")
                            && !pkg_id.starts_with("coreutils.")
                            && !pkg_id.starts_with("vim-enhanced."),
                        "auto_packages should not reference base package: {pkg_id}"
                    );
                }
            }
        } else {
            panic!("expected SectionData::Rpm");
        }
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

        let classification = classify_leaf_auto(&exec, &added, &HashSet::new(), &[]);

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

        let classification = classify_leaf_auto(&exec, &added, &HashSet::new(), &[]);

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

        let classification = classify_leaf_auto(&exec, &added, &HashSet::new(), &[]);

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

        let classification = classify_leaf_auto(&exec, &added, &HashSet::new(), &[]);

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
        let classification = classify_leaf_auto(&exec, &added, &HashSet::new(), &[]);
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
        let classification = classify_leaf_auto(&exec, &added, &baseline, &[]);

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
                &StepId::MappingFileOwnership,
                &StepId::ResolvingDepTree,
                &StepId::VerifyingIntegrity,
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

    // -----------------------------------------------------------------------
    // classify_deps_rpm tests
    // -----------------------------------------------------------------------

    #[test]
    fn classify_deps_rpm_builds_arch_aware_graph() {
        // vim depends on glibc (via libc.so.6 capability).
        // glibc has no deps in added set.
        //
        // Batched query: `rpm -q --queryformat <sentinel> glibc vim`
        // (sorted alphabetically by name).
        let sentinel_output = "\
@@glibc
R\trpmlib(CompressedFileNames) <= 3.0.4-1
R\t/sbin/ldconfig
R\tbasesystem
@@vim
R\tlibc.so.6()(64bit)
R\tlibncurses.so.6()(64bit)
R\trpmlib(PayloadIsZstd) <= 5.4.18-1
R\t/usr/bin/sh
";
        let exec = MockExecutor::new()
            .with_command(
                &format!("rpm -q --queryformat {} glibc vim", RPM_DEPS_FORMAT),
                ExecResult {
                    exit_code: 0,
                    stdout: sentinel_output.into(),
                    stderr: String::new(),
                },
            )
            // Provides index. glibc.i686 provides only the 32-bit soname and
            // is not in the added set; it must not be credited for vim.
            .with_command(
                &format!("rpm -qa --queryformat {}", RPM_PROVIDES_INDEX_FORMAT),
                ExecResult {
                    exit_code: 0,
                    stdout: "basesystem.noarch\tbasesystem\n\
                             glibc.x86_64\tglibc(x86-64)\n\
                             glibc.x86_64\tlibc.so.6()(64bit)\n\
                             glibc.i686\tlibc.so.6\n\
                             ncurses-libs.x86_64\tlibncurses.so.6()(64bit)\n\
                             vim.x86_64\tvim(x86-64)\n"
                        .into(),
                    stderr: String::new(),
                },
            );

        let added_ids: HashSet<String> = ["vim.x86_64", "glibc.x86_64"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let deps = classify_deps_rpm(&exec, &added_ids, &[]).expect("graph should be available");
        // vim depends on glibc (via libc.so.6()(64bit) → glibc.x86_64).
        assert!(deps.get("vim.x86_64").unwrap().contains("glibc.x86_64"));
        assert!(!deps.get("vim.x86_64").unwrap().contains("glibc.i686"));
        // ncurses-libs is not in added_ids, so it should not appear.
        assert!(
            !deps
                .get("vim.x86_64")
                .unwrap()
                .iter()
                .any(|d| d.contains("ncurses"))
        );
        // glibc has no deps in the added set (basesystem is not in added_ids).
        assert!(deps.get("glibc.x86_64").unwrap().is_empty());
    }

    #[test]
    fn classify_deps_rpm_maps_each_capability_to_its_own_provider() {
        // Three unrelated packages whose requirements all land in one
        // provider-resolution batch. Each capability has a different
        // provider, and each package must depend only on the provider of
        // its own capability.
        let exec = MockExecutor::new()
            .with_command(
                &format!(
                    "rpm -q --queryformat {} glibc nginx-core openssl-libs pcre2 tree wget",
                    RPM_DEPS_FORMAT
                ),
                ExecResult {
                    exit_code: 0,
                    stdout: "\
@@glibc
@@nginx-core
R\tlibpcre2-8.so.0()(64bit)
@@openssl-libs
@@pcre2
@@tree
R\tlibc.so.6()(64bit)
@@wget
R\tlibssl.so.3()(64bit)
"
                    .into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                &format!("rpm -qa --queryformat {}", RPM_PROVIDES_INDEX_FORMAT),
                ExecResult {
                    exit_code: 0,
                    stdout: "glibc.aarch64\tlibc.so.6()(64bit)\n\
                             pcre2.aarch64\tlibpcre2-8.so.0()(64bit)\n\
                             openssl-libs.aarch64\tlibssl.so.3()(64bit)\n"
                        .into(),
                    stderr: String::new(),
                },
            );

        let added_ids: HashSet<String> = [
            "glibc.aarch64",
            "nginx-core.aarch64",
            "openssl-libs.aarch64",
            "pcre2.aarch64",
            "tree.aarch64",
            "wget.aarch64",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let deps = classify_deps_rpm(&exec, &added_ids, &[]).expect("graph should be available");

        let only = |id: &str| -> HashSet<String> { [id.to_string()].into_iter().collect() };
        assert_eq!(deps["tree.aarch64"], only("glibc.aarch64"));
        assert_eq!(deps["wget.aarch64"], only("openssl-libs.aarch64"));
        assert_eq!(deps["nginx-core.aarch64"], only("pcre2.aarch64"));
    }

    #[test]
    fn classify_leaf_auto_gives_disjoint_leaves_disjoint_closures() {
        // tree needs libacl; nginx-core needs pcre2. Neither leaf's closure
        // may contain the other leaf's dependency.
        let exec = MockExecutor::new()
            .with_command(
                "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
                ExecResult {
                    exit_code: 0,
                    stdout: "tree.aarch64\nnginx-core.aarch64\n".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                &format!(
                    "rpm -q --queryformat {} libacl nginx-core pcre2 tree",
                    RPM_DEPS_FORMAT
                ),
                ExecResult {
                    exit_code: 0,
                    stdout: "\
@@libacl
@@nginx-core
R\tlibpcre2-8.so.0()(64bit)
@@pcre2
@@tree
R\tlibacl.so.1()(64bit)
"
                    .into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                &format!("rpm -qa --queryformat {}", RPM_PROVIDES_INDEX_FORMAT),
                ExecResult {
                    exit_code: 0,
                    stdout: "libacl.aarch64\tlibacl.so.1()(64bit)\n\
                             pcre2.aarch64\tlibpcre2-8.so.0()(64bit)\n"
                        .into(),
                    stderr: String::new(),
                },
            );

        let added = vec![
            make_test_entry("tree", "aarch64"),
            make_test_entry("nginx-core", "aarch64"),
            make_test_entry("libacl", "aarch64"),
            make_test_entry("pcre2", "aarch64"),
        ];
        let result = classify_leaf_auto(&exec, &added, &HashSet::new(), &[]);
        let tree = result.leaf_dep_tree.as_object().expect("dep tree object");
        let closure = |leaf: &str| -> Vec<String> {
            serde_json::from_value(tree[leaf].clone()).expect("closure is a string list")
        };
        assert_eq!(closure("tree.aarch64"), vec!["libacl.aarch64"]);
        assert_eq!(closure("nginx-core.aarch64"), vec!["pcre2.aarch64"]);
    }

    /// Mock the two rpm queries behind the dependency graph, and a
    /// `--userinstalled` answer naming the leaves.
    fn dep_graph_executor(
        names: &str,
        deps_stdout: &str,
        index_stdout: &str,
        userinstalled: &str,
    ) -> MockExecutor {
        MockExecutor::new()
            .with_command(
                "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
                ExecResult {
                    exit_code: 0,
                    stdout: userinstalled.into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                &format!("rpm -q --queryformat {} {}", RPM_DEPS_FORMAT, names),
                ExecResult {
                    exit_code: 0,
                    stdout: deps_stdout.into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                &format!("rpm -qa --queryformat {}", RPM_PROVIDES_INDEX_FORMAT),
                ExecResult {
                    exit_code: 0,
                    stdout: index_stdout.into(),
                    stderr: String::new(),
                },
            )
    }

    fn ownership(package_name: &str, paths: &[&str]) -> FileOwnershipEntry {
        FileOwnershipEntry {
            package_name: package_name.into(),
            paths: paths.iter().map(|p| p.to_string()).collect(),
        }
    }

    fn ids(list: &[&str]) -> HashSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn closure(result: &LeafClassification, leaf: &str) -> Vec<String> {
        let tree = result.leaf_dep_tree.as_object().expect("dep tree object");
        serde_json::from_value(tree[leaf].clone()).expect("closure is a string list")
    }

    #[test]
    fn classify_deps_rpm_resolves_file_requirements_to_their_owners() {
        // Requirement forms measured on RHEL 9.8: git needs /usr/bin/perl,
        // httpd needs /etc/mime.types. Neither path is in any package's
        // provides; only file ownership answers them. bash provides
        // /bin/sh explicitly. A legacy /sbin path matches the /usr form
        // the owner records, and a path with a space stays whole.
        let exec = dep_graph_executor(
            "bash fonts git httpd liberation-sans-fonts mailcap perl-interpreter procps-ng",
            "\
@@bash
@@fonts
R\t/usr/share/fonts/Liberation Sans/LiberationSans.ttf
@@git
R\t/usr/bin/perl
R\t/bin/sh
@@httpd
R\t/etc/mime.types
R\t/sbin/sysctl
@@mailcap
@@perl-interpreter
@@procps-ng
",
            "bash.aarch64\tbash\n\
             bash.aarch64\t/bin/sh\n\
             git.aarch64\tgit\n\
             httpd.aarch64\thttpd\n\
             mailcap.noarch\tmailcap\n\
             perl-interpreter.aarch64\tperl-interpreter\n\
             procps-ng.aarch64\tprocps-ng\n\
             liberation-sans-fonts.noarch\tliberation-sans-fonts\n",
            "",
        );
        let owners = [
            ownership("perl-interpreter", &["/usr/bin/perl"]),
            ownership("mailcap", &["/etc/mime.types"]),
            ownership("bash", &["/usr/bin/bash", "/usr/bin/sh"]),
            ownership("procps-ng", &["/usr/sbin/sysctl"]),
            ownership(
                "liberation-sans-fonts",
                &["/usr/share/fonts/Liberation Sans/LiberationSans.ttf"],
            ),
        ];
        let added = ids(&[
            "bash.aarch64",
            "fonts.aarch64",
            "git.aarch64",
            "httpd.aarch64",
            "mailcap.noarch",
            "perl-interpreter.aarch64",
            "procps-ng.aarch64",
            "liberation-sans-fonts.noarch",
        ]);
        let deps = classify_deps_rpm(&exec, &added, &owners).expect("graph should be available");
        assert_eq!(
            deps["git.aarch64"],
            ids(&["perl-interpreter.aarch64", "bash.aarch64"])
        );
        assert_eq!(
            deps["httpd.aarch64"],
            ids(&["mailcap.noarch", "procps-ng.aarch64"])
        );
        assert_eq!(
            deps["fonts.aarch64"],
            ids(&["liberation-sans-fonts.noarch"])
        );
    }

    #[test]
    fn classify_leaf_auto_follows_recommends_and_file_requirements_per_leaf() {
        // httpd Recommends mod_http2 (dnf installs it on rhel-bootc 9.8 and
        // 10.2), mod_http2 needs libnghttp2, and httpd needs mailcap
        // through /etc/mime.types. git needs perl-interpreter through
        // /usr/bin/perl. The two leaves' closures stay disjoint.
        let exec = dep_graph_executor(
            "git httpd libnghttp2 mailcap mod_http2 perl-interpreter",
            "\
@@git
R\t/usr/bin/perl
@@httpd
R\t/etc/mime.types
W\tmod_http2
@@libnghttp2
@@mailcap
@@mod_http2
R\tlibnghttp2.so.14()(64bit)
@@perl-interpreter
",
            "git.aarch64\tgit\n\
             httpd.aarch64\thttpd\n\
             libnghttp2.aarch64\tlibnghttp2.so.14()(64bit)\n\
             mailcap.noarch\tmailcap\n\
             mod_http2.aarch64\tmod_http2\n\
             perl-interpreter.aarch64\tperl-interpreter\n",
            "git.aarch64\nhttpd.aarch64\n",
        );
        let owners = [
            ownership("perl-interpreter", &["/usr/bin/perl"]),
            ownership("mailcap", &["/etc/mime.types"]),
        ];
        let added: Vec<PackageEntry> = [
            ("git", "aarch64"),
            ("httpd", "aarch64"),
            ("libnghttp2", "aarch64"),
            ("mailcap", "noarch"),
            ("mod_http2", "aarch64"),
            ("perl-interpreter", "aarch64"),
        ]
        .iter()
        .map(|(n, a)| make_test_entry(n, a))
        .collect();

        let result = classify_leaf_auto(&exec, &added, &HashSet::new(), &owners);
        assert_eq!(
            closure(&result, "git.aarch64"),
            vec!["perl-interpreter.aarch64"]
        );
        assert_eq!(
            closure(&result, "httpd.aarch64"),
            vec!["libnghttp2.aarch64", "mailcap.noarch", "mod_http2.aarch64"]
        );
    }

    #[test]
    fn classify_deps_rpm_follows_supplements_in_reverse() {
        // glibc-langpack-en carries a rich Supplements of this form: it is
        // pulled in when glibc (base image, not added) and one of the
        // langpacks are installed. The edge runs from the langpack to it.
        // tuned-profiles-foo supplements a capability nothing installed
        // provides, so nothing pulls it in.
        let exec = dep_graph_executor(
            "glibc-langpack-en langpacks-en tuned-profiles-foo",
            "\
@@glibc-langpack-en
S\t(glibc and (langpacks-en or langpacks-core-en))
@@langpacks-en
@@tuned-profiles-foo
S\tfoo-hardware
",
            "glibc.aarch64\tglibc\n\
             glibc-langpack-en.aarch64\tglibc-langpack-en\n\
             langpacks-en.noarch\tlangpacks-en\n\
             tuned-profiles-foo.noarch\ttuned-profiles-foo\n",
            "",
        );
        let added = ids(&[
            "glibc-langpack-en.aarch64",
            "langpacks-en.noarch",
            "tuned-profiles-foo.noarch",
        ]);
        let deps = classify_deps_rpm(&exec, &added, &[]).expect("graph should be available");
        assert_eq!(
            deps["langpacks-en.noarch"],
            ids(&["glibc-langpack-en.aarch64"])
        );
        assert!(deps["glibc-langpack-en.aarch64"].is_empty());
        assert!(
            deps.values()
                .all(|d| !d.contains("tuned-profiles-foo.noarch"))
        );
    }

    #[test]
    fn classify_deps_rpm_evaluates_rich_dependencies_against_the_host() {
        // Forms measured in RHEL 9.8 rpmdb output. The `if` condition is
        // met by a base-image package (selinux-policy-targeted is not in
        // the added set) or not met at all (no kernel installed). An `or`
        // takes the first operand that is installed.
        let exec = dep_graph_executor(
            "cockpit-ws-selinux cockpit-ws grubby nano vim-minimal",
            "\
@@cockpit-ws
W\t(cockpit-ws-selinux if selinux-policy-targeted)
W\t(grubby if kernel)
R\t(emacs or vim-minimal or nano)
@@cockpit-ws-selinux
@@grubby
@@nano
@@vim-minimal
",
            "cockpit-ws.aarch64\tcockpit-ws\n\
             cockpit-ws-selinux.aarch64\tcockpit-ws-selinux\n\
             grubby.aarch64\tgrubby\n\
             nano.aarch64\tnano\n\
             selinux-policy-targeted.noarch\tselinux-policy-targeted\n\
             vim-minimal.aarch64\tvim-minimal\n",
            "",
        );
        let added = ids(&[
            "cockpit-ws.aarch64",
            "cockpit-ws-selinux.aarch64",
            "grubby.aarch64",
            "nano.aarch64",
            "vim-minimal.aarch64",
        ]);
        let deps = classify_deps_rpm(&exec, &added, &[]).expect("graph should be available");
        assert_eq!(
            deps["cockpit-ws.aarch64"],
            ids(&["cockpit-ws-selinux.aarch64", "vim-minimal.aarch64"])
        );
    }

    #[test]
    fn parse_sentinel_deps_reads_tags_and_skips_rpmlib() {
        let parsed = parse_sentinel_deps(
            "\
@@dnf
R\t/bin/sh
R\t/bin/sh
R\trpmlib(RichDependencies)
W\t(python3-dbus if NetworkManager)
X\tunknown-tag
untagged line
@@gpg-pubkey
",
        );
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "dnf");
        let relations: Vec<Relation> = parsed[0].1.iter().map(|(r, _)| *r).collect();
        assert_eq!(relations, vec![Relation::Requires, Relation::Recommends]);
        assert_eq!(parsed[1], ("gpg-pubkey".to_string(), Vec::new()));
    }

    #[test]
    fn classify_deps_rpm_returns_none_when_provides_index_fails() {
        // A failed index must not read as "no dependencies": returning None
        // sends classification to the dnf fallback instead.
        let exec = MockExecutor::new()
            .with_command(
                &format!("rpm -q --queryformat {} tree", RPM_DEPS_FORMAT),
                ExecResult {
                    exit_code: 0,
                    stdout: "@@tree\nR\tlibc.so.6()(64bit)\n".into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                &format!("rpm -qa --queryformat {}", RPM_PROVIDES_INDEX_FORMAT),
                ExecResult {
                    exit_code: 1,
                    stdout: String::new(),
                    stderr: "error: rpmdb open failed".into(),
                },
            );

        let added_ids: HashSet<String> = ["tree.aarch64"].iter().map(|s| s.to_string()).collect();
        assert!(classify_deps_rpm(&exec, &added_ids, &[]).is_none());
    }

    #[test]
    fn classify_deps_rpm_returns_none_when_rpm_unavailable() {
        let exec = MockExecutor::new().with_command(
            &format!("rpm -q --queryformat {} glibc", RPM_DEPS_FORMAT),
            ExecResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "rpm: command not found".into(),
            },
        );

        let added_ids: HashSet<String> = ["glibc.x86_64"].iter().map(|s| s.to_string()).collect();
        let deps = classify_deps_rpm(&exec, &added_ids, &[]);
        assert!(deps.is_none());
    }

    #[test]
    fn classify_deps_rpm_empty_set_returns_empty_graph() {
        let exec = MockExecutor::new();
        let added_ids: HashSet<String> = HashSet::new();
        let deps = classify_deps_rpm(&exec, &added_ids, &[]).expect("empty set should return Some");
        assert!(deps.is_empty());
    }

    #[test]
    fn filter_capabilities_skips_rpmlib_and_paths() {
        let stdout = "rpmlib(CompressedFileNames) <= 3.0.4-1\n\
                      /usr/bin/sh\n\
                      libc.so.6()(64bit)\n\
                      /sbin/ldconfig\n\
                      libm.so.6()(64bit)\n\
                      rpmlib(PayloadIsZstd) <= 5.4.18-1\n";
        let caps = filter_capabilities(stdout);
        assert_eq!(caps, vec!["libc.so.6()(64bit)", "libm.so.6()(64bit)"]);
    }

    #[test]
    fn filter_capabilities_deduplicates() {
        let stdout = "libc.so.6()(64bit)\n\
                      libc.so.6()(64bit)\n\
                      libm.so.6()(64bit)\n";
        let caps = filter_capabilities(stdout);
        assert_eq!(caps, vec!["libc.so.6()(64bit)", "libm.so.6()(64bit)"]);
    }

    #[test]
    fn filter_capabilities_strips_version_constraints() {
        let stdout = "libc.so.6(GLIBC_2.17)(64bit) >= 2.17\n\
                      libpthread.so.0()(64bit)\n";
        let caps = filter_capabilities(stdout);
        assert_eq!(
            caps,
            vec!["libc.so.6(GLIBC_2.17)(64bit)", "libpthread.so.0()(64bit)"]
        );
    }

    #[test]
    fn name_from_id_extracts_name() {
        assert_eq!(name_from_id("glibc.x86_64"), "glibc");
        assert_eq!(name_from_id("vim-enhanced.noarch"), "vim-enhanced");
        assert_eq!(name_from_id("noarch"), "noarch"); // no dot → return whole string
    }

    #[test]
    fn classify_leaf_auto_uses_rpm_with_bfs_transitive_closure() {
        // Three packages: httpd depends on apr, apr depends on glibc.
        // rpm -qR gives direct deps only → BFS must compute transitive closure.
        let exec = MockExecutor::new()
            .with_command(
                "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
                ExecResult {
                    exit_code: 1,
                    stdout: String::new(),
                    stderr: "dnf not found".into(),
                },
            )
            // Batched rpm query for all packages (sorted: apr, glibc, httpd)
            .with_command(
                &format!("rpm -q --queryformat {} apr glibc httpd", RPM_DEPS_FORMAT),
                ExecResult {
                    exit_code: 0,
                    stdout: "\
@@apr
R\tlibc.so.6()(64bit)
@@glibc
R\trpmlib(CompressedFileNames) <= 3.0.4-1
@@httpd
R\tlibapr-1.so.0()(64bit)
R\tlibc.so.6()(64bit)
"
                    .into(),
                    stderr: String::new(),
                },
            )
            .with_command(
                &format!("rpm -qa --queryformat {}", RPM_PROVIDES_INDEX_FORMAT),
                ExecResult {
                    exit_code: 0,
                    stdout: "apr.x86_64\tlibapr-1.so.0()(64bit)\n\
                             glibc.x86_64\tlibc.so.6()(64bit)\n"
                        .into(),
                    stderr: String::new(),
                },
            );

        let packages = vec![
            PackageEntry {
                name: "httpd".into(),
                version: "2.4.57-5.el9".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                ..Default::default()
            },
            PackageEntry {
                name: "apr".into(),
                version: "1.7.0-12.el9".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                version: "2.34-60.el9".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                ..Default::default()
            },
        ];

        let baseline = HashSet::new();
        let result = classify_leaf_auto(&exec, &packages, &baseline, &[]);
        // httpd is leaf (nothing depends on it).
        assert_eq!(result.leaf_packages, Some(vec!["httpd.x86_64".to_string()]));
        // apr and glibc are auto.
        let mut auto = result.auto_packages.unwrap();
        auto.sort();
        assert_eq!(auto, vec!["apr.x86_64", "glibc.x86_64"]);
        // httpd's dep tree should include BOTH apr and glibc (transitive via BFS).
        let tree = result.leaf_dep_tree.as_object().unwrap();
        let httpd_deps: Vec<String> =
            serde_json::from_value(tree.get("httpd.x86_64").unwrap().clone()).unwrap();
        assert!(httpd_deps.contains(&"apr.x86_64".to_string()));
        assert!(httpd_deps.contains(&"glibc.x86_64".to_string()));
    }

    #[test]
    fn test_parse_group_info_packages() {
        let stdout = "\
Group: Container Management
 Description: Tools for managing Linux containers
 Mandatory Packages:
   podman
   buildah
   skopeo
 Default Packages:
   containernetworking-plugins
   crun
 Optional Packages:
   toolbox
   udica
";
        let packages = parse_group_info_packages(stdout);
        assert_eq!(
            packages,
            vec![
                "buildah",
                "containernetworking-plugins",
                "crun",
                "podman",
                "skopeo",
                "toolbox",
                "udica",
            ]
        );
    }

    #[test]
    fn test_parse_group_info_empty() {
        let packages = parse_group_info_packages("");
        assert!(packages.is_empty());
    }

    #[test]
    fn test_installed_groups_filter_to_installed_only() {
        // Mock rpm -qa output with only some packages installed
        let rpm_qa_output = "\
0:bash-5.2.26-3.el9.x86_64
0:vim-enhanced-9.0.1592-1.el9.x86_64
0:httpd-2.4.57-5.el9.x86_64
";
        // Mock file ownership (required by inspector)
        let file_ownership_output = "\
@@bash
R\t/usr/bin/bash
@@vim-enhanced
R\t/usr/bin/vim
@@httpd
R\t/usr/sbin/httpd
";
        // Mock dnf group list output
        let group_list_output = "\
Installed Groups:
   Development Tools
";
        // Mock dnf group info output listing MORE packages than are installed
        let group_info_output = "\
Group: Development Tools
 Description: A basic development environment
 Mandatory Packages:
   gcc
   make
 Default Packages:
   bash
   vim-enhanced
 Optional Packages:
   httpd
   gdb
   strace
";
        let exec = MockExecutor::new()
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
            .with_command(
                "env LC_ALL=C dnf group list --installed",
                ExecResult {
                    stdout: group_list_output.into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "env LC_ALL=C dnf group info Development Tools",
                ExecResult {
                    stdout: group_info_output.into(),
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

        let output = RpmInspector::new().inspect(&ctx, &NullProgress).unwrap();
        if let SectionData::Rpm(rpm) = &output.section {
            let groups = rpm.installed_groups.as_ref().unwrap();
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0].name, "Development Tools");

            // Group metadata lists: gcc, make, bash, vim-enhanced, httpd, gdb, strace
            // But only bash, vim-enhanced, httpd are actually installed
            // Expected members after filtering: bash, httpd, vim-enhanced (sorted)
            assert_eq!(groups[0].members, vec!["bash", "httpd", "vim-enhanced"]);
        } else {
            panic!("expected SectionData::Rpm");
        }
    }
}
