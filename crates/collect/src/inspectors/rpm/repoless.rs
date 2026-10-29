//! Repo-less RPM detection and dnf cache scanning.
//!
//! Identifies packages with no source repository (empty `source_repo`) or
//! whose `source_repo` names a repo not in `dnf repolist --enabled`.
//! Candidates that provide repository configuration the export carries
//! and that an enabled repo serves by name are repo-enabling instead,
//! and leave the repo-less set before the cache scan.
//! For each remaining repo-less package, scans `/var/cache/dnf/` for
//! cached `.rpm` files matching the package NEVRA.

use inspectah_core::traits::executor::Executor;
use inspectah_core::types::repo::RepoId;
use inspectah_core::types::rpm::{FileOwnershipEntry, PackageEntry, RepoFile, RpmSection};
use std::collections::{BTreeSet, HashMap, HashSet};

/// The `dnf repoquery` format that answers both "is this name available"
/// and "from which section". `%{reponame}` prints the section id in dnf 4.
const AVAILABILITY_QUERY_FORMAT: &str = "%{name} %{reponame}\n";

/// Directories whose files make a package a provider of repository
/// configuration (the first of the two classification tests).
const REPO_CONFIG_DIRS: [&str; 2] = ["/etc/yum.repos.d/", "/etc/pki/rpm-gpg/"];

/// The recorded source repo of a package installed by local RPM path.
const COMMANDLINE_REPO: &str = "@commandline";

/// A package that carries a repository definition is named for it.
const RELEASE_NAME_SUFFIX: &str = "-release";

/// Fetch the list of enabled repo IDs from `dnf repolist --enabled`.
///
/// Returns `None` if the command fails (e.g., dnf not available).
/// Callers must distinguish "no enabled repos" (`Some(vec![])`) from
/// "unable to determine" (`None`) to avoid treating all packages as
/// repo-less when dnf itself is broken.
fn get_enabled_repos(exec: &dyn Executor) -> Option<Vec<String>> {
    let result = exec.run("dnf", &["repolist", "--enabled", "-q"]);
    if result.exit_code != 0 {
        return None;
    }
    Some(
        result
            .stdout
            .lines()
            .skip(1) // Skip header line
            .filter_map(|line| {
                let id = line.split_whitespace().next()?;
                if id.is_empty() {
                    None
                } else {
                    Some(id.to_string())
                }
            })
            .collect(),
    )
}

/// Check whether a package's `source_repo` short name matches any enabled
/// repo ID using case-insensitive substring matching.
///
/// `dnf repoquery --installed` returns install-time short names like
/// `AppStream` or `baseos`, while `dnf repolist --enabled` returns full
/// repo IDs like `rhel-9-for-aarch64-appstream-rpms`. Exact comparison
/// fails for every RHEL system. This function normalizes by checking
/// whether the lowercased short name appears anywhere within any
/// lowercased enabled repo ID.
fn repo_matches_enabled(source_repo: &str, enabled_repos: &[String]) -> bool {
    let short = source_repo.to_lowercase();
    enabled_repos
        .iter()
        .any(|full_id| full_id.to_lowercase().contains(&short))
}

/// Query `dnf repoquery --available` for every package name the host's
/// enabled repositories serve, and the sections that serve it.
///
/// Returns `None` if the command fails, so callers can tell "no enabled
/// repo serves this name" from "we could not ask". A line that does not
/// split into exactly two whitespace tokens is dropped: a host whose dnf
/// does not print `%{reponame}` answers nothing rather than answering
/// wrongly.
///
/// Every value is non-empty by construction — an entry exists only
/// because some line named a section — and the serving ids are collected
/// into a `BTreeSet` so the recorded order is a property of the container
/// rather than of the order the host happened to print.
fn get_available_packages(exec: &dyn Executor) -> Option<HashMap<String, BTreeSet<RepoId>>> {
    let result = exec.run(
        "dnf",
        &[
            "repoquery",
            "--available",
            "--queryformat",
            AVAILABILITY_QUERY_FORMAT,
            "-q",
        ],
    );
    if result.exit_code != 0 {
        return None;
    }
    let mut by_name: HashMap<String, BTreeSet<RepoId>> = HashMap::new();
    for line in result.stdout.lines() {
        let mut tokens = line.split_whitespace();
        let (Some(name), Some(repo), None) = (tokens.next(), tokens.next(), tokens.next()) else {
            continue;
        };
        by_name
            .entry(name.to_string())
            .or_default()
            .insert(RepoId::new(repo));
    }
    Some(by_name)
}

/// What the two classification tests concluded about one repo-less
/// candidate.
///
/// This stays private deliberately. The serialized contract is
/// `repo_enabling_from`: non-empty means repo-enabling, and a second
/// field carrying the same fact would be a second source of truth for
/// it. Promote this onto `PackageEntry` the moment anything needs to
/// *branch* on `ConfigOnly` rather than display it — that distinction
/// currently survives only inside a rendered sentence, and deriving
/// structure by parsing rendered text is not an option.
enum RepoEnablingVerdict {
    /// The candidate provides repository configuration the export carries
    /// and these sections serve it by name: it is repo-enabling.
    Enabling(Vec<RepoId>),
    /// The candidate provides repository configuration the export carries,
    /// but no enabled repo serves it by name. It stays repo-less, with a
    /// note that says which half it failed. Carries the number of captured
    /// repo files it owns.
    ConfigOnly { captured_repo_files: usize },
    /// Not a repo-enabling candidate, or availability could not be
    /// determined. The ordinary repo-less path applies.
    Repoless,
}

/// Whether a path is one the first classification test looks at.
fn is_repo_config_path(path: &str) -> bool {
    REPO_CONFIG_DIRS.iter().any(|dir| path.starts_with(dir))
}

/// Run the two classification tests against one repo-less candidate.
///
/// *Provides repository configuration:* installed `@commandline`, named
/// `*-release`, and every path it owns under the repo configuration
/// directories is one the snapshot captured, with at least one `.repo`
/// file among them. Owning a repo file the export is not carrying fails
/// the test: the Containerfile could not reproduce the definition.
///
/// *Installable by name from a repository the export can carry:* the
/// name is a key of the availability map. `available` being `None` means
/// the question could not be asked, and the recovery path is suppressed
/// unless availability is proven.
fn classify_repo_enabling(
    pkg: &PackageEntry,
    repo_files: &[RepoFile],
    gpg_keys: &[RepoFile],
    file_ownership: &[FileOwnershipEntry],
    available: Option<&HashMap<String, BTreeSet<RepoId>>>,
) -> RepoEnablingVerdict {
    let Some(available) = available else {
        return RepoEnablingVerdict::Repoless;
    };
    if pkg.source_repo != COMMANDLINE_REPO || !pkg.name.ends_with(RELEASE_NAME_SUFFIX) {
        return RepoEnablingVerdict::Repoless;
    }
    // A degraded scan collects no ownership data. The repo-file count
    // below refuses the same input, so this is an early return rather
    // than an independent guard.
    let Some(owned) = file_ownership
        .iter()
        .find(|entry| entry.package_name == pkg.name)
    else {
        return RepoEnablingVerdict::Repoless;
    };

    let captured_repo_files: HashSet<&str> = repo_files.iter().map(|f| f.path.as_str()).collect();
    let captured_keys: HashSet<&str> = gpg_keys.iter().map(|f| f.path.as_str()).collect();

    let mut owned_repo_files = 0usize;
    for path in owned.paths.iter().filter(|path| is_repo_config_path(path)) {
        if captured_repo_files.contains(path.as_str()) {
            owned_repo_files += 1;
        } else if !captured_keys.contains(path.as_str()) {
            return RepoEnablingVerdict::Repoless;
        }
    }
    if owned_repo_files == 0 {
        return RepoEnablingVerdict::Repoless;
    }

    match available.get(&pkg.name) {
        Some(serving) => RepoEnablingVerdict::Enabling(serving.iter().cloned().collect()),
        None => RepoEnablingVerdict::ConfigOnly {
            captured_repo_files: owned_repo_files,
        },
    }
}

/// A package the scan is about to annotate.
struct RepolessTarget {
    /// Index into `packages_added`.
    index: usize,
    /// `Some(n)` when the package owns `n` captured repo files but no
    /// enabled repo serves it by name; the note says so.
    captured_repo_files: Option<usize>,
}

/// Identify repo-less packages and scan `/var/cache/dnf/` for cached RPMs.
///
/// A package is repo-less when:
/// 1. `source_repo` is empty (no repo recorded), OR
/// 2. `source_repo` names a repo not in `dnf repolist --enabled`
///
/// Packages from the `anaconda` pseudo-repo (OS installer) get a
/// secondary availability check: if `dnf repoquery --available` confirms
/// the package exists in an enabled repo, it is NOT repo-less.
/// `@commandline` packages (installed via `rpm -i`) are repo-less unless
/// [`classify_repo_enabling`] proves they are repo-enabling: otherwise
/// the user explicitly bypassed the repo system and the installed version
/// may differ from the repo version.
///
/// For each truly repo-less package, checks whether a matching `.rpm`
/// file exists in the dnf cache. Found RPMs get `repoless_cached = true`
/// and `cache_path` set; missing RPMs get an annotation directing the
/// user toward manual resolution.
///
/// Only `packages_added` is annotated. The rest of the section is read
/// for classification: `repo_files`, `gpg_keys`, and `file_ownership`
/// say what repository configuration the export can carry and who owns
/// it.
pub fn scan_dnf_cache_for_repoless(exec: &dyn Executor, rpm: &mut RpmSection) {
    let enabled_repos = get_enabled_repos(exec);
    let RpmSection {
        packages_added,
        repo_files,
        gpg_keys,
        file_ownership,
        ..
    } = rpm;

    // Phase 1: identify candidates whose source_repo doesn't match an
    // enabled repo. These are candidates, not confirmed repo-less —
    // the checks in phase 2 may clear them.
    let candidate_indices: Vec<usize> = packages_added
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            if p.source_repo.is_empty() {
                true // Always a candidate: no repo recorded
            } else {
                match &enabled_repos {
                    Some(repos) => !repo_matches_enabled(&p.source_repo, repos),
                    None => false, // dnf failed -- don't assume disabled
                }
            }
        })
        .map(|(i, _)| i)
        .collect();

    if candidate_indices.is_empty() {
        return;
    }

    // Phase 2: repo-enabling classification, then the secondary
    // availability check for anaconda packages.
    // @commandline packages are repo-less unless they are repo-enabling.
    // Empty source_repo packages are always repo-less (no info at all).
    // Only anaconda (and similar installer pseudo-repos) get the check.
    let available = get_available_packages(exec);
    let mut repoless_targets: Vec<RepolessTarget> = Vec::new();
    for i in candidate_indices {
        let captured_repo_files = match classify_repo_enabling(
            &packages_added[i],
            repo_files,
            gpg_keys,
            file_ownership,
            available.as_ref(),
        ) {
            RepoEnablingVerdict::Enabling(serving) => {
                // Repo-enabling: leaves the repo-less set before the
                // cache scan, so it never gets a note or an upload.
                packages_added[i].repo_enabling_from = serving;
                continue;
            }
            RepoEnablingVerdict::ConfigOnly {
                captured_repo_files,
            } => Some(captured_repo_files),
            RepoEnablingVerdict::Repoless => None,
        };

        let pkg = &packages_added[i];
        // @commandline = explicitly installed outside repos, always repo-less
        let is_repoless = if pkg.source_repo.starts_with('@') || pkg.source_repo.is_empty() {
            true
        } else {
            // For other pseudo-repos (anaconda, etc.), check availability
            match &available {
                Some(avail) => !avail.contains_key(&pkg.name),
                None => true, // repoquery failed -- keep candidate as repo-less
            }
        };
        if is_repoless {
            repoless_targets.push(RepolessTarget {
                index: i,
                captured_repo_files,
            });
        }
    }

    if repoless_targets.is_empty() {
        return;
    }

    // List all .rpm files in the dnf cache.
    let cache_result = exec.run("find", &["/var/cache/dnf", "-name", "*.rpm", "-type", "f"]);
    let cache_files: Vec<String> = match cache_result {
        ref r if r.exit_code == 0 => r
            .stdout
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        _ => Vec::new(),
    };

    for target in repoless_targets {
        let pkg = &mut packages_added[target.index];
        let expected_filename = format!(
            "{}-{}-{}.{}.rpm",
            pkg.name, pkg.version, pkg.release, pkg.arch
        );

        let cache_match = cache_files.iter().find(|f| f.ends_with(&expected_filename));

        let is_disabled_repo = !pkg.source_repo.is_empty();
        let reason = match target.captured_repo_files {
            Some(n) => format!(
                "No repo source: owns {n} captured repo file(s) but is not available from them"
            ),
            None if is_disabled_repo => format!(
                "No repo source \u{2014} repo '{}' not in enabled repos",
                pkg.source_repo
            ),
            None => "No repo source".to_string(),
        };

        if let Some(path) = cache_match {
            pkg.repoless_cached = true;
            pkg.cache_path = Some(path.clone());
            pkg.repoless_annotation =
                format!("{reason} \u{2014} cached RPM bundled (pre-excluded, no GPG verification)");
        } else {
            pkg.repoless_cached = false;
            pkg.cache_path = None;
            pkg.repoless_annotation = format!("{reason} \u{2014} manual resolution needed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::types::rpm::PackageState;

    /// Helper: build a PackageEntry with name, version, release, arch, and source_repo.
    fn pkg(
        name: &str,
        version: &str,
        release: &str,
        arch: &str,
        source_repo: &str,
    ) -> PackageEntry {
        PackageEntry {
            name: name.into(),
            version: version.into(),
            release: release.into(),
            arch: arch.into(),
            source_repo: source_repo.into(),
            state: PackageState::Added,
            ..Default::default()
        }
    }

    /// Run the scan over a bare package list, with nothing captured in
    /// `repo_files`, `gpg_keys`, or `file_ownership` — which is what every
    /// fixture predating repo-enabling classification meant.
    fn scan_packages(exec: &MockExecutor, packages: Vec<PackageEntry>) -> Vec<PackageEntry> {
        let mut rpm = RpmSection {
            packages_added: packages,
            ..Default::default()
        };
        scan_dnf_cache_for_repoless(exec, &mut rpm);
        rpm.packages_added
    }

    /// Build a MockExecutor with enabled repos, available packages, and cache listing.
    fn build_repoless_executor(
        enabled_repos_stdout: &str,
        cache_listing_stdout: &str,
    ) -> MockExecutor {
        build_repoless_executor_with_available(enabled_repos_stdout, "", cache_listing_stdout)
    }

    /// Build a MockExecutor with enabled repos, `%{name} %{reponame}`
    /// availability lines, and cache listing.
    fn build_repoless_executor_with_available(
        enabled_repos_stdout: &str,
        available_stdout: &str,
        cache_listing_stdout: &str,
    ) -> MockExecutor {
        MockExecutor::new()
            .with_command(
                "dnf repolist --enabled -q",
                ExecResult {
                    stdout: enabled_repos_stdout.into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "dnf repoquery --available --queryformat %{name} %{reponame}\n -q",
                ExecResult {
                    stdout: available_stdout.into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "find /var/cache/dnf -name *.rpm -type f",
                ExecResult {
                    stdout: cache_listing_stdout.into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
    }

    #[test]
    fn repoless_rpm_found_in_cache() {
        let exec = build_repoless_executor(
            "repo id                       repo name\nappstream                     RHEL 9 AppStream\n",
            "/var/cache/dnf/custom-repo/packages/custom-tool-1.2.3-1.el9.x86_64.rpm\n",
        );

        let packages = scan_packages(
            &exec,
            vec![pkg("custom-tool", "1.2.3", "1.el9", "x86_64", "")],
        );

        assert!(packages[0].repoless_cached);
        assert_eq!(
            packages[0].cache_path,
            Some("/var/cache/dnf/custom-repo/packages/custom-tool-1.2.3-1.el9.x86_64.rpm".into())
        );
        assert!(
            packages[0]
                .repoless_annotation
                .contains("cached RPM bundled")
        );
    }

    #[test]
    fn repoless_rpm_not_in_cache() {
        let exec = build_repoless_executor(
            "repo id                       repo name\nappstream                     RHEL 9 AppStream\n",
            "", // empty cache
        );

        let packages = scan_packages(
            &exec,
            vec![pkg("custom-tool", "1.2.3", "1.el9", "x86_64", "")],
        );

        assert!(!packages[0].repoless_cached);
        assert!(packages[0].cache_path.is_none());
        assert!(
            packages[0]
                .repoless_annotation
                .contains("manual resolution needed")
        );
    }

    #[test]
    fn rpm_with_source_repo_not_flagged() {
        let exec = build_repoless_executor(
            "repo id                       repo name\nappstream                     RHEL 9 AppStream\n",
            "",
        );

        let packages = scan_packages(
            &exec,
            vec![pkg("httpd", "2.4.57", "5.el9", "x86_64", "appstream")],
        );

        // Should not be touched — source_repo matches an enabled repo.
        assert!(!packages[0].repoless_cached);
        assert!(packages[0].cache_path.is_none());
        assert!(packages[0].repoless_annotation.is_empty());
    }

    #[test]
    fn short_name_matches_full_repo_id_case_insensitive() {
        // Real RHEL scenario: dnf repolist returns full IDs, source_repo
        // has install-time short names from %{from_repo}.
        let exec = build_repoless_executor(
            "repo id                       repo name\nrhel-9-for-aarch64-appstream-rpms  RHEL 9 AppStream\nrhel-9-for-aarch64-baseos-rpms     RHEL 9 BaseOS\n",
            "",
        );

        let packages = scan_packages(
            &exec,
            vec![
                pkg("httpd", "2.4.57", "5.el9", "aarch64", "AppStream"),
                pkg("bash", "5.2.26", "3.el9", "aarch64", "baseos"),
                pkg("glibc", "2.34", "60.el9", "aarch64", "BaseOS"),
            ],
        );

        // All should match via case-insensitive substring.
        assert!(
            packages[0].repoless_annotation.is_empty(),
            "AppStream should match rhel-9-for-aarch64-appstream-rpms"
        );
        assert!(
            packages[1].repoless_annotation.is_empty(),
            "baseos should match rhel-9-for-aarch64-baseos-rpms"
        );
        assert!(
            packages[2].repoless_annotation.is_empty(),
            "BaseOS should match rhel-9-for-aarch64-baseos-rpms"
        );
    }

    #[test]
    fn anaconda_package_available_in_repo_not_flagged() {
        // anaconda is the install-time source, not a real repo. But if
        // the package IS available in an enabled repo (the common case
        // for base OS packages), it should NOT be flagged as repo-less.
        let exec = build_repoless_executor_with_available(
            "repo id                       repo name\nrhel-9-for-x86_64-appstream-rpms  RHEL 9 AppStream\nrhel-9-for-x86_64-baseos-rpms     RHEL 9 BaseOS\n",
            "kernel rhel-9-for-x86_64-baseos-rpms\nbash rhel-9-for-x86_64-baseos-rpms\nglibc rhel-9-for-x86_64-baseos-rpms\naudit rhel-9-for-x86_64-appstream-rpms\n",
            "",
        );

        let packages = scan_packages(
            &exec,
            vec![pkg("kernel", "5.14.0", "362.el9", "x86_64", "anaconda")],
        );

        assert!(
            packages[0].repoless_annotation.is_empty(),
            "anaconda package available in repos should not be flagged"
        );
    }

    #[test]
    fn anaconda_package_not_available_still_flagged() {
        // A package from anaconda that is NOT in any enabled repo
        // (e.g., a custom installer-only package) should still be flagged.
        let exec = build_repoless_executor_with_available(
            "repo id                       repo name\nrhel-9-for-x86_64-appstream-rpms  RHEL 9 AppStream\n",
            "bash rhel-9-for-x86_64-baseos-rpms\nhttpd rhel-9-for-x86_64-appstream-rpms\n",
            "",
        );

        let packages = scan_packages(
            &exec,
            vec![pkg(
                "custom-installer-pkg",
                "1.0",
                "1.el9",
                "x86_64",
                "anaconda",
            )],
        );

        assert!(
            !packages[0].repoless_annotation.is_empty(),
            "anaconda package NOT available should be flagged"
        );
    }

    #[test]
    fn commandline_package_always_flagged() {
        // @commandline packages (installed via rpm -i) are repo-less unless
        // they also provide the repository configuration they are served
        // from. This one owns no captured repo file, so availability alone
        // does not clear it.
        let exec = build_repoless_executor_with_available(
            "repo id                       repo name\nrhel-9-for-x86_64-baseos-rpms     RHEL 9 BaseOS\n",
            "epel-release epel\nbash rhel-9-for-x86_64-baseos-rpms\n",
            "",
        );

        let packages = scan_packages(
            &exec,
            vec![pkg("epel-release", "9", "7.el9", "noarch", "@commandline")],
        );

        assert!(
            !packages[0].repoless_annotation.is_empty(),
            "@commandline packages should always be flagged as repo-less"
        );
    }

    #[test]
    fn repo_matches_enabled_unit() {
        use super::repo_matches_enabled;

        let enabled = vec![
            "rhel-9-for-aarch64-appstream-rpms".to_string(),
            "rhel-9-for-aarch64-baseos-rpms".to_string(),
        ];

        assert!(repo_matches_enabled("AppStream", &enabled));
        assert!(repo_matches_enabled("appstream", &enabled));
        assert!(repo_matches_enabled("baseos", &enabled));
        assert!(repo_matches_enabled("BaseOS", &enabled));
        assert!(!repo_matches_enabled("anaconda", &enabled));
        assert!(!repo_matches_enabled("epel", &enabled));
    }

    #[test]
    fn rpm_with_disabled_repo_detected_as_repoless() {
        let exec = build_repoless_executor(
            "repo id                       repo name\nappstream                     RHEL 9 AppStream\n",
            "/var/cache/dnf/internal-tools/packages/internal-agent-2.0-1.el9.x86_64.rpm\n",
        );

        let packages = scan_packages(
            &exec,
            vec![pkg(
                "internal-agent",
                "2.0",
                "1.el9",
                "x86_64",
                "internal-tools",
            )],
        );

        // internal-tools is not in enabled repos → treated as repo-less.
        assert!(packages[0].repoless_cached);
        assert!(packages[0].repoless_annotation.contains("internal-tools"));
        assert!(
            packages[0]
                .repoless_annotation
                .contains("not in enabled repos")
        );
    }

    #[test]
    fn cache_path_survives_json_roundtrip() {
        let entry = PackageEntry {
            name: "custom-tool".into(),
            version: "1.2.3".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            repoless_cached: true,
            cache_path: Some(
                "/var/cache/dnf/repo/packages/custom-tool-1.2.3-1.el9.x86_64.rpm".into(),
            ),
            repoless_annotation: "No repo source \u{2014} cached RPM bundled".into(),
            ..Default::default()
        };

        let json = serde_json::to_string(&entry).unwrap();
        let parsed: PackageEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.cache_path, entry.cache_path);
        assert_eq!(parsed.repoless_cached, entry.repoless_cached);
        assert_eq!(parsed.repoless_annotation, entry.repoless_annotation);
    }

    #[test]
    fn get_enabled_repos_parses_dnf_output() {
        let exec = MockExecutor::new().with_command(
            "dnf repolist --enabled -q",
            ExecResult {
                stdout: "repo id                       repo name\nbaseos                        RHEL 9 BaseOS\nappstream                     RHEL 9 AppStream\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        );

        let repos = get_enabled_repos(&exec);
        assert_eq!(
            repos,
            Some(vec!["baseos".to_string(), "appstream".to_string()])
        );
    }

    #[test]
    fn get_enabled_repos_returns_none_on_failure() {
        let exec = MockExecutor::new().with_command(
            "dnf repolist --enabled -q",
            ExecResult {
                exit_code: 1,
                ..Default::default()
            },
        );

        let repos = get_enabled_repos(&exec);
        assert!(repos.is_none(), "should return None when dnf fails");
    }

    #[test]
    fn dnf_repolist_failure_only_flags_empty_source_repo() {
        // When dnf repolist fails, only packages with empty source_repo
        // should be treated as repo-less. Packages with a named source_repo
        // should NOT be flagged (we can't confirm the repo is disabled).
        let exec = MockExecutor::new()
            .with_command(
                "dnf repolist --enabled -q",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            )
            .with_command(
                "find /var/cache/dnf -name *.rpm -type f",
                ExecResult {
                    stdout: "/var/cache/dnf/local/packages/custom-tool-1.0-1.el9.x86_64.rpm\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let packages = scan_packages(
            &exec,
            vec![
                pkg("httpd", "2.4.57", "5.el9", "x86_64", "appstream"),
                pkg("custom-tool", "1.0", "1.el9", "x86_64", ""),
            ],
        );

        // httpd has a named source_repo -- must NOT be flagged when dnf fails
        assert!(
            packages[0].repoless_annotation.is_empty(),
            "httpd should not be flagged as repo-less when dnf fails"
        );

        // custom-tool has empty source_repo -- should be flagged
        assert!(
            packages[1].repoless_cached,
            "custom-tool with empty source_repo should still be detected"
        );
    }

    #[test]
    fn mixed_packages_only_repoless_annotated() {
        let exec = build_repoless_executor(
            "repo id                       repo name\nappstream                     RHEL 9 AppStream\nbaseos                        RHEL 9 BaseOS\n",
            "/var/cache/dnf/local/packages/custom-tool-1.0-1.el9.x86_64.rpm\n",
        );

        let packages = scan_packages(
            &exec,
            vec![
                pkg("httpd", "2.4.57", "5.el9", "x86_64", "appstream"), // has enabled repo
                pkg("custom-tool", "1.0", "1.el9", "x86_64", ""),       // empty source_repo
                pkg("bash", "5.2.26", "3.el9", "x86_64", "baseos"),     // has enabled repo
            ],
        );

        // httpd: not repo-less
        assert!(packages[0].repoless_annotation.is_empty());
        assert!(!packages[0].repoless_cached);

        // custom-tool: repo-less, found in cache
        assert!(packages[1].repoless_cached);
        assert!(packages[1].cache_path.is_some());

        // bash: not repo-less
        assert!(packages[2].repoless_annotation.is_empty());
        assert!(!packages[2].repoless_cached);
    }

    #[test]
    fn no_packages_is_noop() {
        let exec = MockExecutor::new();
        // Should not panic or make any executor calls.
        let packages = scan_packages(&exec, vec![]);
        assert!(packages.is_empty());
    }

    #[test]
    fn all_packages_have_enabled_repos_is_noop() {
        // When all packages have a source_repo that matches an enabled repo,
        // the find command should still be skipped (no repo-less packages).
        let exec = MockExecutor::new().with_command(
            "dnf repolist --enabled -q",
            ExecResult {
                stdout: "repo id                       repo name\nappstream                     RHEL 9 AppStream\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        );

        let packages = scan_packages(
            &exec,
            vec![pkg("httpd", "2.4.57", "5.el9", "x86_64", "appstream")],
        );

        // No annotation should be set.
        assert!(packages[0].repoless_annotation.is_empty());
    }
}
