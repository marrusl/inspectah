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
use inspectah_core::types::repo::{
    PseudoRepo, RepoAttribution, RepoId, RepoSectionDef, parse_repo_sections,
};
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

/// Attribute each package's recorded `source_repo` to a `.repo` section.
///
/// `populate_source_repos` stores the install-time value dnf recorded
/// (`%{from_repo}`): a full repo id, a short name like `AppStream` or
/// `baseos`, a pseudo repo (`@commandline`, `anaconda`, `@System`), or
/// empty. The `.repo` config files define the sections actually on the
/// host. This function classifies each package's recorded value against
/// those sections per the § 3.2 normalization rule and records the result
/// as [`RepoAttribution`], updating `source_repo` only where that rule
/// says to (the resolved id for `Exact`/`Resolved`; left as recorded
/// otherwise).
///
/// Resolution order, applied independently per package so the result never
/// depends on section order:
/// 1. Exact case-insensitive match against any section, enabled or not,
///    wins first (`Exact`) -- dnf recorded that id, so the package keeps
///    the attribution even if the section is now disabled.
/// 2. Otherwise, case-insensitive substring match against `enabled = true`
///    sections only: exactly one match is `Resolved`; more than one is
///    `Ambiguous` and `source_repo` is left as recorded.
/// 3. Otherwise: `@commandline`, `anaconda`, `@System` are `Pseudo`; empty
///    is `Unrecorded`; anything else is `Stale`.
pub fn normalize_source_repos(packages: &mut [PackageEntry], repo_files: &[RepoFile]) {
    let sections: Vec<RepoSectionDef> = repo_files
        .iter()
        .flat_map(|rf| parse_repo_sections(&rf.content))
        .collect();

    for pkg in packages.iter_mut() {
        pkg.repo_attribution = classify_recorded_repo(&pkg.source_repo, &sections);
        match &pkg.repo_attribution {
            RepoAttribution::Exact { id } | RepoAttribution::Resolved { id, .. } => {
                pkg.source_repo = id.as_str().to_string();
            }
            _ => {}
        }
    }
}

/// Classify one package's recorded `source_repo` value against the host's
/// parsed `.repo` sections. See [`normalize_source_repos`] for the
/// resolution order.
fn classify_recorded_repo(recorded: &str, sections: &[RepoSectionDef]) -> RepoAttribution {
    if let Some(section) = sections.iter().find(|s| s.id == RepoId::new(recorded)) {
        return RepoAttribution::Exact {
            id: section.id.clone(),
        };
    }

    // An empty recorded value has no substring to look for -- treat it as
    // "no match" rather than letting `str::contains("")` match every
    // enabled section.
    let substring_matches: Vec<&RepoId> = if recorded.is_empty() {
        Vec::new()
    } else {
        let lower = recorded.to_lowercase();
        sections
            .iter()
            .filter(|s| s.enabled && s.id.as_str().to_lowercase().contains(&lower))
            .map(|s| &s.id)
            .collect()
    };

    match substring_matches.len() {
        1 => RepoAttribution::Resolved {
            recorded: recorded.to_string(),
            id: substring_matches[0].clone(),
        },
        n if n > 1 => RepoAttribution::Ambiguous {
            recorded: recorded.to_string(),
            candidates: substring_matches.into_iter().cloned().collect(),
        },
        _ => match recorded {
            "@commandline" => RepoAttribution::Pseudo {
                which: PseudoRepo::Commandline,
                recorded: recorded.to_string(),
            },
            "anaconda" => RepoAttribution::Pseudo {
                which: PseudoRepo::Anaconda,
                recorded: recorded.to_string(),
            },
            "@System" => RepoAttribution::Pseudo {
                which: PseudoRepo::System,
                recorded: recorded.to_string(),
            },
            "" => RepoAttribution::Unrecorded,
            _ => RepoAttribution::Stale {
                recorded: recorded.to_string(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::types::FindingKind;
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
            disposition: FindingKind::included(),
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

    /// Four repo files matching the host's real layout, used to prove
    /// `normalize_source_repos` is order-independent (§ 8.4).
    fn host_repo_files() -> Vec<RepoFile> {
        vec![
            repo_file(
                "[redhat]\nname=Red Hat\nenabled=1\nbaseurl=https://example.invalid/redhat\n",
            ),
            repo_file(
                "[epel-cisco-openh264]\nname=EPEL openh264\nenabled=1\nbaseurl=https://example.invalid/openh264\n",
            ),
            repo_file(
                "[epel-testing]\nname=EPEL Testing\nenabled=0\nbaseurl=https://example.invalid/epel-testing\n",
            ),
            repo_file("[epel]\nname=EPEL\nenabled=1\nbaseurl=https://example.invalid/epel\n"),
        ]
    }

    #[test]
    fn normalization_is_order_independent_and_never_relabels() {
        // § 8.4: the host's four repo files in both orders yield the same answer.
        let files = host_repo_files(); // redhat.repo, epel-cisco-openh264.repo, epel-testing.repo, epel.repo
        let mut reversed = files.clone();
        reversed.reverse();
        for order in [files, reversed] {
            let mut pkgs = vec![pkg_with_repo("htop", "epel")];
            normalize_source_repos(&mut pkgs, &order);
            assert_eq!(pkgs[0].source_repo, "epel");
            assert!(
                matches!(&pkgs[0].repo_attribution, RepoAttribution::Exact { id } if id == &RepoId::new("epel"))
            );
        }
    }

    #[test]
    fn attribution_table() {
        // § 8.4, the remaining rows.
        // exact match on a disabled section -> Exact on that section
        let disabled_only = vec![repo_file(
            "[epel-testing]\nname=EPEL Testing\nenabled=0\nbaseurl=https://example.invalid/epel-testing\n",
        )];
        let mut pkgs = vec![pkg_with_repo("htop", "epel-testing")];
        normalize_source_repos(&mut pkgs, &disabled_only);
        assert_eq!(pkgs[0].source_repo, "epel-testing");
        assert!(
            matches!(&pkgs[0].repo_attribution, RepoAttribution::Exact { id } if id == &RepoId::new("epel-testing"))
        );

        // short name whose only substring match is disabled -> Stale
        let mut pkgs = vec![pkg_with_repo("htop", "testing")];
        normalize_source_repos(&mut pkgs, &disabled_only);
        assert_eq!(pkgs[0].source_repo, "testing");
        assert!(
            matches!(&pkgs[0].repo_attribution, RepoAttribution::Stale { recorded } if recorded == "testing")
        );

        // short name substring-matching two enabled sections -> Ambiguous, source_repo unchanged
        let two_enabled = vec![repo_file(
            "[rhel-9-for-x86_64-baseos-rpms]\nname=BaseOS\nenabled=1\nbaseurl=https://example.invalid/baseos\n[rhel-9-for-x86_64-baseos-debug-rpms]\nname=BaseOS Debug\nenabled=1\nbaseurl=https://example.invalid/baseos-debug\n",
        )];
        let mut pkgs = vec![pkg_with_repo("glibc", "baseos")];
        normalize_source_repos(&mut pkgs, &two_enabled);
        assert_eq!(pkgs[0].source_repo, "baseos");
        assert!(matches!(
            &pkgs[0].repo_attribution,
            RepoAttribution::Ambiguous { recorded, candidates }
                if recorded == "baseos" && candidates.len() == 2
        ));

        // "anaconda" -> Pseudo(Anaconda); "" -> Unrecorded
        let mut pkgs = vec![
            pkg_with_repo("kernel", "anaconda"),
            pkg_with_repo("custom-tool", ""),
        ];
        normalize_source_repos(&mut pkgs, &disabled_only);
        assert!(matches!(
            &pkgs[0].repo_attribution,
            RepoAttribution::Pseudo { which: PseudoRepo::Anaconda, recorded } if recorded == "anaconda"
        ));
        assert!(matches!(
            &pkgs[1].repo_attribution,
            RepoAttribution::Unrecorded
        ));
    }
}
