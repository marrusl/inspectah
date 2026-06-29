//! Source repository attribution for RPM packages.
//!
//! Attributes each installed RPM to its source repository. Primary strategy uses
//! `dnf repoquery --installed` in batches of 100. Fallback parses
//! `rpm -qi` output for `From repo` / `Repository` lines.
//!
//! `@System` and empty repo values are skipped — the downstream
//! attention model correctly handles empty `source_repo` as Tier 3
//! NeedsReview.

use inspectah_core::traits::executor::Executor;
use inspectah_core::types::rpm::{PackageEntry, RepoFile};
use std::collections::{HashMap, HashSet};

/// Maximum number of package names per dnf/rpm invocation.
const BATCH_SIZE: usize = 100;

/// Populate `source_repo` on each package entry.
///
/// 1. Collect unique package names.
/// 2. Try `dnf repoquery --installed` (primary).
/// 3. If dnf fails on the probe, fall back to `rpm -qi`.
/// 4. Set `source_repo` on matching packages.
pub fn populate_source_repos(executor: &dyn Executor, packages: &mut [PackageEntry]) {
    let name_set: HashSet<String> = packages.iter().map(|p| p.name.clone()).collect();
    let mut names: Vec<&str> = name_set.iter().map(|s| s.as_str()).collect();
    names.sort();

    if names.is_empty() {
        return;
    }

    let mut repo_map: HashMap<String, String> = HashMap::new();

    if !try_dnf_source_repo(executor, &names, &name_set, &mut repo_map) {
        try_rpm_source_repo(executor, &names, &mut repo_map);
    }

    for pkg in packages.iter_mut() {
        if let Some(repo) = repo_map.get(&pkg.name) {
            pkg.source_repo = repo.clone();
        }
    }
}

/// Returns true if the repo value should be skipped (not stored).
fn should_skip_repo(repo: &str) -> bool {
    repo.is_empty() || repo == "@System"
}

/// Primary strategy: `dnf repoquery --installed --queryformat "%{name} %{from_repo}\n"`.
///
/// Probes with the first package to detect whether dnf repoquery works.
/// If the probe fails, returns false so the caller can try the fallback.
/// Remaining packages are processed in batches of [`BATCH_SIZE`].
fn try_dnf_source_repo(
    executor: &dyn Executor,
    names: &[&str],
    name_set: &HashSet<String>,
    repo_map: &mut HashMap<String, String>,
) -> bool {
    // Probe with the first package.
    let probe = executor.run(
        "dnf",
        &[
            "repoquery",
            "--installed",
            "--queryformat",
            "%{name} %{from_repo}\n",
            names[0],
        ],
    );
    if !probe.success() {
        return false;
    }
    parse_dnf_repo_lines(&probe.stdout, name_set, repo_map);

    // Process remaining in batches.
    let mut i = 1;
    while i < names.len() {
        let end = std::cmp::min(i + BATCH_SIZE, names.len());
        let mut args: Vec<&str> = vec![
            "repoquery",
            "--installed",
            "--queryformat",
            "%{name} %{from_repo}\n",
        ];
        args.extend_from_slice(&names[i..end]);

        let result = executor.run("dnf", &args);
        if result.success() {
            parse_dnf_repo_lines(&result.stdout, name_set, repo_map);
        }

        i = end;
    }

    true
}

/// Parse `dnf repoquery` output lines of the form `<name> <repo>`.
fn parse_dnf_repo_lines(
    stdout: &str,
    name_set: &HashSet<String>,
    repo_map: &mut HashMap<String, String>,
) {
    for line in stdout.lines() {
        let line = line.trim();
        if let Some((name, repo)) = line.split_once(' ')
            && name_set.contains(name)
            && !should_skip_repo(repo)
        {
            repo_map
                .entry(name.to_string())
                .or_insert_with(|| repo.to_string());
        }
    }
}

/// Fallback strategy: `rpm -qi <pkg1> <pkg2> ...`.
///
/// Parses multiline output looking for `Name :` and `From repo :` /
/// `Repository :` lines. Correlates the repo value with the preceding
/// name line.
fn try_rpm_source_repo(
    executor: &dyn Executor,
    names: &[&str],
    repo_map: &mut HashMap<String, String>,
) {
    for chunk in names.chunks(BATCH_SIZE) {
        let mut args: Vec<&str> = vec!["-qi"];
        args.extend_from_slice(chunk);

        let result = executor.run("rpm", &args);
        if !result.success() {
            continue;
        }

        let mut cur_name = String::new();
        for line in result.stdout.lines() {
            if let Some(stripped) = line.strip_prefix("Name") {
                if let Some((_, val)) = stripped.split_once(':') {
                    cur_name = val.trim().to_string();
                }
            } else if (line.starts_with("From repo") || line.starts_with("Repository"))
                && let Some((_, val)) = line.split_once(':')
            {
                let repo = val.trim();
                if !cur_name.is_empty() && !should_skip_repo(repo) {
                    repo_map
                        .entry(cur_name.clone())
                        .or_insert_with(|| repo.to_string());
                }
            }
        }
    }
}

/// Extract INI-style section IDs (`[repo-id]`) from `.repo` file content.
fn extract_section_ids(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                Some(trimmed[1..trimmed.len() - 1].to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Normalize `source_repo` short names to full repo IDs.
///
/// `populate_source_repos` stores install-time short names like `AppStream`
/// or `baseos` (from `%{from_repo}`). The `.repo` config files use full IDs
/// like `rhel-9-for-aarch64-appstream-rpms`. This function maps short names
/// to full IDs using case-insensitive substring matching so that package
/// tables and config trees display the same identifier.
///
/// Packages with empty `source_repo` or no matching full ID are left
/// unchanged.
pub fn normalize_source_repos(packages: &mut [PackageEntry], repo_files: &[RepoFile]) {
    // Collect all section IDs from repo file content.
    let full_ids: Vec<String> = repo_files
        .iter()
        .flat_map(|rf| extract_section_ids(&rf.content))
        .collect();

    if full_ids.is_empty() {
        return;
    }

    for pkg in packages.iter_mut() {
        if pkg.source_repo.is_empty() {
            continue;
        }
        let short = pkg.source_repo.to_lowercase();
        if let Some(full_id) = full_ids
            .iter()
            .find(|id| id.to_lowercase().contains(&short))
        {
            pkg.source_repo = full_id.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::types::rpm::PackageState;

    /// Helper: build a PackageEntry with just the name set.
    fn pkg(name: &str) -> PackageEntry {
        PackageEntry {
            name: name.into(),
            state: PackageState::Added,
            ..Default::default()
        }
    }

    #[test]
    fn test_populate_via_dnf_repoquery() {
        let exec = MockExecutor::new()
            .with_command(
                "dnf repoquery --installed --queryformat %{name} %{from_repo}\n bash",
                ExecResult {
                    stdout: "bash baseos\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "dnf repoquery --installed --queryformat %{name} %{from_repo}\n glibc httpd",
                ExecResult {
                    stdout: "glibc baseos\nhttpd appstream\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let mut packages = vec![pkg("bash"), pkg("glibc"), pkg("httpd")];
        populate_source_repos(&exec, &mut packages);

        assert_eq!(
            packages
                .iter()
                .find(|p| p.name == "bash")
                .unwrap()
                .source_repo,
            "baseos"
        );
        assert_eq!(
            packages
                .iter()
                .find(|p| p.name == "glibc")
                .unwrap()
                .source_repo,
            "baseos"
        );
        assert_eq!(
            packages
                .iter()
                .find(|p| p.name == "httpd")
                .unwrap()
                .source_repo,
            "appstream"
        );
    }

    #[test]
    fn test_dnf_system_repo_skipped() {
        let exec = MockExecutor::new().with_command(
            "dnf repoquery --installed --queryformat %{name} %{from_repo}\n bash",
            ExecResult {
                stdout: "bash @System\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        );

        let mut packages = vec![pkg("bash")];
        populate_source_repos(&exec, &mut packages);

        assert_eq!(
            packages[0].source_repo, "",
            "@System should be skipped, leaving source_repo empty"
        );
    }

    #[test]
    fn test_dnf_empty_repo_skipped() {
        let exec = MockExecutor::new().with_command(
            "dnf repoquery --installed --queryformat %{name} %{from_repo}\n bash",
            ExecResult {
                stdout: "bash \n".into(),
                exit_code: 0,
                ..Default::default()
            },
        );

        let mut packages = vec![pkg("bash")];
        populate_source_repos(&exec, &mut packages);

        assert_eq!(packages[0].source_repo, "", "empty repo should be skipped");
    }

    #[test]
    fn test_fallback_to_rpm_qi() {
        // dnf repoquery fails (exit code 1)
        let exec = MockExecutor::new()
            .with_command(
                "dnf repoquery --installed --queryformat %{name} %{from_repo}\n bash",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            )
            .with_command(
                "rpm -qi bash httpd",
                ExecResult {
                    stdout: "\
Name        : bash
Version     : 5.2.26
Release     : 3.el9
From repo   : baseos
Name        : httpd
Version     : 2.4.57
Release     : 5.el9
From repo   : appstream
"
                    .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let mut packages = vec![pkg("bash"), pkg("httpd")];
        populate_source_repos(&exec, &mut packages);

        assert_eq!(
            packages
                .iter()
                .find(|p| p.name == "bash")
                .unwrap()
                .source_repo,
            "baseos"
        );
        assert_eq!(
            packages
                .iter()
                .find(|p| p.name == "httpd")
                .unwrap()
                .source_repo,
            "appstream"
        );
    }

    #[test]
    fn test_fallback_rpm_qi_repository_field() {
        // dnf fails, rpm -qi uses "Repository" instead of "From repo"
        let exec = MockExecutor::new()
            .with_command(
                "dnf repoquery --installed --queryformat %{name} %{from_repo}\n bash",
                ExecResult {
                    exit_code: 127,
                    ..Default::default()
                },
            )
            .with_command(
                "rpm -qi bash",
                ExecResult {
                    stdout: "\
Name        : bash
Version     : 5.2.26
Repository  : baseos
"
                    .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let mut packages = vec![pkg("bash")];
        populate_source_repos(&exec, &mut packages);

        assert_eq!(packages[0].source_repo, "baseos");
    }

    #[test]
    fn test_empty_when_both_fail() {
        let exec = MockExecutor::new()
            .with_command(
                "dnf repoquery --installed --queryformat %{name} %{from_repo}\n bash",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            )
            .with_command(
                "rpm -qi bash",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let mut packages = vec![pkg("bash")];
        populate_source_repos(&exec, &mut packages);

        assert_eq!(
            packages[0].source_repo, "",
            "source_repo should remain empty when both strategies fail"
        );
    }

    #[test]
    fn test_empty_packages_noop() {
        let exec = MockExecutor::new();
        let mut packages: Vec<PackageEntry> = vec![];
        populate_source_repos(&exec, &mut packages);
        // No panic, no commands run — just a no-op.
    }

    #[test]
    fn test_dnf_unknown_package_in_output_ignored() {
        // dnf output includes a package name not in our set — should be ignored
        let exec = MockExecutor::new().with_command(
            "dnf repoquery --installed --queryformat %{name} %{from_repo}\n bash",
            ExecResult {
                stdout: "bash baseos\nunknown-pkg epel\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        );

        let mut packages = vec![pkg("bash")];
        populate_source_repos(&exec, &mut packages);

        assert_eq!(packages[0].source_repo, "baseos");
        // "unknown-pkg" is silently dropped because it's not in name_set
    }

    #[test]
    fn test_duplicate_packages_first_repo_wins() {
        // Two packages with the same name — first repo found wins
        let exec = MockExecutor::new().with_command(
            "dnf repoquery --installed --queryformat %{name} %{from_repo}\n bash",
            ExecResult {
                stdout: "bash baseos\nbash appstream\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        );

        let mut packages = vec![pkg("bash"), pkg("bash")];
        populate_source_repos(&exec, &mut packages);

        // Both should get "baseos" (first seen wins in repo_map)
        assert!(packages.iter().all(|p| p.source_repo == "baseos"));
    }

    // --- normalize_source_repos tests ---

    fn pkg_with_repo(name: &str, source_repo: &str) -> PackageEntry {
        PackageEntry {
            name: name.into(),
            source_repo: source_repo.into(),
            state: PackageState::Added,
            ..Default::default()
        }
    }

    fn repo_file(content: &str) -> RepoFile {
        RepoFile {
            path: "/etc/yum.repos.d/test.repo".into(),
            content: content.into(),
            is_default_repo: false,
            include: true,
            locked: false,
            aggregate: None,
        }
    }

    #[test]
    fn test_normalize_appstream_to_full_id() {
        let repo_files = vec![repo_file(
            "[rhel-9-for-aarch64-appstream-rpms]\nname=RHEL 9 AppStream\n",
        )];
        let mut packages = vec![pkg_with_repo("httpd", "AppStream")];

        normalize_source_repos(&mut packages, &repo_files);

        assert_eq!(
            packages[0].source_repo, "rhel-9-for-aarch64-appstream-rpms",
            "AppStream should normalize to full repo ID"
        );
    }

    #[test]
    fn test_normalize_baseos_to_full_id() {
        let repo_files = vec![repo_file(
            "[rhel-9-for-x86_64-baseos-rpms]\nname=RHEL 9 BaseOS\n[rhel-9-for-x86_64-appstream-rpms]\nname=RHEL 9 AppStream\n",
        )];
        let mut packages = vec![
            pkg_with_repo("bash", "baseos"),
            pkg_with_repo("glibc", "BaseOS"),
        ];

        normalize_source_repos(&mut packages, &repo_files);

        assert_eq!(packages[0].source_repo, "rhel-9-for-x86_64-baseos-rpms");
        assert_eq!(packages[1].source_repo, "rhel-9-for-x86_64-baseos-rpms");
    }

    #[test]
    fn test_normalize_anaconda_stays_unchanged() {
        let repo_files = vec![repo_file(
            "[rhel-9-for-x86_64-baseos-rpms]\nname=RHEL 9 BaseOS\n",
        )];
        let mut packages = vec![pkg_with_repo("kernel", "anaconda")];

        normalize_source_repos(&mut packages, &repo_files);

        assert_eq!(
            packages[0].source_repo, "anaconda",
            "anaconda has no matching repo ID, should stay as-is"
        );
    }

    #[test]
    fn test_normalize_empty_source_repo_unchanged() {
        let repo_files = vec![repo_file(
            "[rhel-9-for-x86_64-baseos-rpms]\nname=RHEL 9 BaseOS\n",
        )];
        let mut packages = vec![pkg_with_repo("custom-tool", "")];

        normalize_source_repos(&mut packages, &repo_files);

        assert_eq!(
            packages[0].source_repo, "",
            "empty source_repo should stay empty"
        );
    }

    #[test]
    fn test_normalize_no_repo_files_is_noop() {
        let repo_files: Vec<RepoFile> = Vec::new();
        let mut packages = vec![pkg_with_repo("httpd", "AppStream")];

        normalize_source_repos(&mut packages, &repo_files);

        assert_eq!(
            packages[0].source_repo, "AppStream",
            "no repo files means no normalization"
        );
    }

    #[test]
    fn test_extract_section_ids() {
        let content =
            "[rhel-9-baseos]\nname=BaseOS\nenabled=1\n\n[rhel-9-appstream]\nname=AppStream\n";
        let ids = extract_section_ids(content);
        assert_eq!(ids, vec!["rhel-9-baseos", "rhel-9-appstream"]);
    }
}
