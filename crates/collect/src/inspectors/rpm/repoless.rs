//! Repo-less RPM detection and dnf cache scanning.
//!
//! Identifies packages with no source repository (empty `source_repo`) or
//! whose `source_repo` names a repo not in `dnf repolist --enabled`.
//! For each repo-less package, scans `/var/cache/dnf/` for cached `.rpm`
//! files matching the package NEVRA.

use inspectah_core::traits::executor::Executor;
use inspectah_core::types::rpm::PackageEntry;

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

/// Identify repo-less packages and scan `/var/cache/dnf/` for cached RPMs.
///
/// A package is repo-less when:
/// 1. `source_repo` is empty (no repo recorded), OR
/// 2. `source_repo` names a repo not in `dnf repolist --enabled`
///
/// For each repo-less package, checks whether a matching `.rpm` file
/// exists in the dnf cache. Found RPMs get `repoless_cached = true`
/// and `cache_path` set; missing RPMs get an annotation directing the
/// user toward manual resolution.
pub fn scan_dnf_cache_for_repoless(exec: &dyn Executor, packages: &mut [PackageEntry]) {
    let enabled_repos = get_enabled_repos(exec);

    // Identify which packages are repo-less.
    // When dnf repolist failed (None), only process packages with empty
    // source_repo -- skip the disabled-repo branch entirely to avoid
    // false-flagging every package as repo-less.
    let repoless_indices: Vec<usize> = packages
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            if p.source_repo.is_empty() {
                true // Always repo-less: no repo recorded
            } else {
                match &enabled_repos {
                    Some(repos) => !repos.iter().any(|r| r == &p.source_repo),
                    None => false, // dnf failed -- don't assume disabled
                }
            }
        })
        .map(|(i, _)| i)
        .collect();

    if repoless_indices.is_empty() {
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

    for idx in repoless_indices {
        let pkg = &mut packages[idx];
        let expected_filename = format!(
            "{}-{}-{}.{}.rpm",
            pkg.name, pkg.version, pkg.release, pkg.arch
        );

        let cache_match = cache_files.iter().find(|f| f.ends_with(&expected_filename));

        let is_disabled_repo = !pkg.source_repo.is_empty();
        let reason = if is_disabled_repo {
            format!(
                "No repo source \u{2014} repo '{}' not in enabled repos",
                pkg.source_repo
            )
        } else {
            "No repo source".to_string()
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

    /// Build a MockExecutor with enabled repos and cache listing.
    fn build_repoless_executor(
        enabled_repos_stdout: &str,
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

        let mut packages = vec![pkg("custom-tool", "1.2.3", "1.el9", "x86_64", "")];
        scan_dnf_cache_for_repoless(&exec, &mut packages);

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

        let mut packages = vec![pkg("custom-tool", "1.2.3", "1.el9", "x86_64", "")];
        scan_dnf_cache_for_repoless(&exec, &mut packages);

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

        let mut packages = vec![pkg("httpd", "2.4.57", "5.el9", "x86_64", "appstream")];
        scan_dnf_cache_for_repoless(&exec, &mut packages);

        // Should not be touched — source_repo matches an enabled repo.
        assert!(!packages[0].repoless_cached);
        assert!(packages[0].cache_path.is_none());
        assert!(packages[0].repoless_annotation.is_empty());
    }

    #[test]
    fn rpm_with_disabled_repo_detected_as_repoless() {
        let exec = build_repoless_executor(
            "repo id                       repo name\nappstream                     RHEL 9 AppStream\n",
            "/var/cache/dnf/internal-tools/packages/internal-agent-2.0-1.el9.x86_64.rpm\n",
        );

        let mut packages = vec![pkg(
            "internal-agent",
            "2.0",
            "1.el9",
            "x86_64",
            "internal-tools",
        )];
        scan_dnf_cache_for_repoless(&exec, &mut packages);

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

        let mut packages = vec![
            pkg("httpd", "2.4.57", "5.el9", "x86_64", "appstream"),
            pkg("custom-tool", "1.0", "1.el9", "x86_64", ""),
        ];
        scan_dnf_cache_for_repoless(&exec, &mut packages);

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

        let mut packages = vec![
            pkg("httpd", "2.4.57", "5.el9", "x86_64", "appstream"), // has enabled repo
            pkg("custom-tool", "1.0", "1.el9", "x86_64", ""),       // empty source_repo
            pkg("bash", "5.2.26", "3.el9", "x86_64", "baseos"),     // has enabled repo
        ];
        scan_dnf_cache_for_repoless(&exec, &mut packages);

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
        let mut packages: Vec<PackageEntry> = vec![];
        // Should not panic or make any executor calls.
        scan_dnf_cache_for_repoless(&exec, &mut packages);
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

        let mut packages = vec![pkg("httpd", "2.4.57", "5.el9", "x86_64", "appstream")];
        scan_dnf_cache_for_repoless(&exec, &mut packages);

        // No annotation should be set.
        assert!(packages[0].repoless_annotation.is_empty());
    }
}
