//! Source repository attribution for RPM packages.
//!
//! Attributes each installed RPM to its source repository, by `name.arch`.
//! Primary strategy uses `dnf repoquery --installed` in batches of 100.
//! Fallback parses `rpm -qi` output for `Name`, `Architecture` and
//! `From repo` / `Repository` lines.
//!
//! `@System` and empty repo values are skipped — the downstream
//! attention model correctly handles empty `source_repo` as Tier 3
//! NeedsReview.
//!
//! A repo is never carried across architectures: a multilib host can
//! have `libfoo.i686` from a repo and `libfoo.x86_64` from a local RPM
//! path, and the repo of one says nothing about the other. A package whose
//! record cannot be told apart from another architecture's, or whose
//! architecture's records disagree, keeps an empty `source_repo`: its
//! provenance is unknown, and nothing installs it by name.

use inspectah_core::traits::executor::Executor;
use inspectah_core::types::repo::{
    PseudoRepo, RepoAttribution, RepoId, RepoSectionDef, parse_repo_sections, section_spelling,
};
use inspectah_core::types::rpm::{PackageEntry, RepoFile};
use std::collections::{HashMap, HashSet};

/// Maximum number of package names per dnf/rpm invocation.
const BATCH_SIZE: usize = 100;

/// The `dnf repoquery --installed` format: name, arch and the repo dnf
/// recorded at install time, one installed package per line.
const SOURCE_REPO_QUERY_FORMAT: &str = "%{name} %{arch} %{from_repo}\n";

/// What the queries recorded for one installed `name.arch`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Recorded {
    /// Every record for it named this repo.
    Repo(String),
    /// Two records for it named different repos (installed versions from
    /// different repos): which entry is which cannot be told.
    Conflicting,
}

/// The repos the queries recorded, by `(name, arch)`.
#[derive(Debug, Default)]
struct SourceRepoRecords {
    by_name_arch: HashMap<(String, String), Recorded>,
    /// Names with a record that carried no architecture: such a record
    /// cannot be told apart from another architecture's, so every entry
    /// of the name is left unknown.
    arch_unknown: HashSet<String>,
}

impl SourceRepoRecords {
    /// Record `repo` for `name.arch`, or for `name` alone when `arch` is
    /// `None`. A skipped value (`@System`, empty) records nothing.
    fn record(&mut self, name: &str, arch: Option<&str>, repo: &str) {
        if should_skip_repo(repo) {
            return;
        }
        let Some(arch) = arch.filter(|a| !a.is_empty()) else {
            self.arch_unknown.insert(name.to_string());
            return;
        };
        self.by_name_arch
            .entry((name.to_string(), arch.to_string()))
            .and_modify(|seen| {
                if *seen != Recorded::Repo(repo.to_string()) {
                    *seen = Recorded::Conflicting;
                }
            })
            .or_insert_with(|| Recorded::Repo(repo.to_string()));
    }

    /// The repo recorded for `pkg`'s own `name.arch`, or `None` when its
    /// provenance is unknown.
    fn repo_of(&self, pkg: &PackageEntry) -> Option<&str> {
        if self.arch_unknown.contains(&pkg.name) {
            return None;
        }
        match self
            .by_name_arch
            .get(&(pkg.name.clone(), pkg.arch.clone()))?
        {
            Recorded::Repo(repo) => Some(repo),
            Recorded::Conflicting => None,
        }
    }
}

/// Populate `source_repo` on each package entry.
///
/// 1. Collect unique package names.
/// 2. Try `dnf repoquery --installed` (primary).
/// 3. If dnf fails on the probe, fall back to `rpm -qi`.
/// 4. Set `source_repo` on each package from its own `name.arch` record
///    ([`SourceRepoRecords::repo_of`]); one with none is left as it is.
pub fn populate_source_repos(executor: &dyn Executor, packages: &mut [PackageEntry]) {
    let name_set: HashSet<String> = packages.iter().map(|p| p.name.clone()).collect();
    let mut names: Vec<&str> = name_set.iter().map(|s| s.as_str()).collect();
    names.sort();

    if names.is_empty() {
        return;
    }

    let mut records = SourceRepoRecords::default();

    if !try_dnf_source_repo(executor, &names, &name_set, &mut records) {
        try_rpm_source_repo(executor, &names, &name_set, &mut records);
    }

    for pkg in packages.iter_mut() {
        if let Some(repo) = records.repo_of(pkg) {
            pkg.source_repo = repo.to_string();
        }
    }
}

/// Returns true if the repo value should be skipped (not stored).
fn should_skip_repo(repo: &str) -> bool {
    repo.is_empty() || repo == "@System"
}

/// Primary strategy: `dnf repoquery --installed --queryformat
/// "%{name} %{arch} %{from_repo}\n"`.
///
/// Probes with the first package to detect whether dnf repoquery works.
/// If the probe fails, returns false so the caller can try the fallback.
/// Remaining packages are processed in batches of [`BATCH_SIZE`].
fn try_dnf_source_repo(
    executor: &dyn Executor,
    names: &[&str],
    name_set: &HashSet<String>,
    records: &mut SourceRepoRecords,
) -> bool {
    // Probe with the first package.
    let probe = executor.run(
        "dnf",
        &[
            "repoquery",
            "--installed",
            "--queryformat",
            SOURCE_REPO_QUERY_FORMAT,
            names[0],
        ],
    );
    if !probe.success() {
        return false;
    }
    parse_dnf_repo_lines(&probe.stdout, name_set, records);

    // Process remaining in batches.
    let mut i = 1;
    while i < names.len() {
        let end = std::cmp::min(i + BATCH_SIZE, names.len());
        let mut args: Vec<&str> = vec![
            "repoquery",
            "--installed",
            "--queryformat",
            SOURCE_REPO_QUERY_FORMAT,
        ];
        args.extend_from_slice(&names[i..end]);

        let result = executor.run("dnf", &args);
        if result.success() {
            parse_dnf_repo_lines(&result.stdout, name_set, records);
        }

        i = end;
    }

    true
}

/// Parse `dnf repoquery` output lines of the form `<name> <arch> <repo>`.
///
/// A line with an empty repo (`<name> <arch> `) records nothing; a line of
/// two words, a name and a repo with no architecture, is a record that
/// cannot be told apart from another architecture's.
fn parse_dnf_repo_lines(stdout: &str, name_set: &HashSet<String>, records: &mut SourceRepoRecords) {
    for line in stdout.lines() {
        let line = line.trim_start();
        let mut fields = line.splitn(3, ' ');
        let (Some(name), Some(second)) = (fields.next(), fields.next()) else {
            continue;
        };
        if !name_set.contains(name) {
            continue;
        }
        match fields.next() {
            Some(repo) => records.record(name, Some(second), repo.trim()),
            None => records.record(name, None, second.trim()),
        }
    }
}

/// Fallback strategy: `rpm -qi <pkg1> <pkg2> ...`.
///
/// Parses multiline output looking for `Name :`, `Architecture :` and
/// `From repo :` / `Repository :` lines, and correlates the repo value
/// with the name and architecture of the record it is in. A record whose
/// architecture is not printed before its repo cannot be told apart from
/// another architecture's.
fn try_rpm_source_repo(
    executor: &dyn Executor,
    names: &[&str],
    name_set: &HashSet<String>,
    records: &mut SourceRepoRecords,
) {
    for chunk in names.chunks(BATCH_SIZE) {
        let mut args: Vec<&str> = vec!["-qi"];
        args.extend_from_slice(chunk);

        let result = executor.run("rpm", &args);
        if !result.success() {
            continue;
        }

        let mut cur_name = String::new();
        let mut cur_arch: Option<String> = None;
        for line in result.stdout.lines() {
            let Some((key, val)) = line.split_once(':') else {
                continue;
            };
            let val = val.trim();
            match key.trim() {
                "Name" => {
                    cur_name = val.to_string();
                    cur_arch = None;
                }
                "Architecture" => cur_arch = Some(val.to_string()),
                "From repo" | "Repository" if name_set.contains(&cur_name) => {
                    records.record(&cur_name, cur_arch.as_deref(), val);
                }
                _ => {}
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
/// 1. `@commandline`, `anaconda`, `@System` (case-insensitive) are
///    `Pseudo`, whatever the host's sections are called: they record how a
///    package was installed, not a repo, and a `.repo` section spelled the
///    same does not make one.
/// 2. Otherwise, a section dnf recorded, enabled or not (`Exact`): the one
///    spelled exactly as recorded, or else the one case variant of it --
///    dnf recorded that id, so the package keeps the attribution even if
///    the section is now disabled. Two or more case variants and no exact
///    spelling is `Ambiguous` among them.
/// 3. Otherwise, case-insensitive substring match against `enabled = true`
///    sections only: exactly one match is `Resolved`; more than one is
///    `Ambiguous` and `source_repo` is left as recorded.
/// 4. Otherwise: empty is `Unrecorded`; anything else is `Stale`.
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
    if let Some(which) = pseudo_repo(recorded) {
        return RepoAttribution::Pseudo {
            which,
            recorded: recorded.to_string(),
        };
    }

    // The section dnf recorded: its exact spelling, or the one case
    // variant there is ([`section_spelling`]). dnf ids are case-sensitive,
    // so a recorded `r` is never rewritten to an `[R]` beside an `[r]`.
    // Two or more case variants and no exact spelling is ambiguous among
    // them, and `source_repo` is left as recorded.
    let spellings = || {
        sections
            .iter()
            .map(|s| s.id.as_str())
            .filter(|id| id.eq_ignore_ascii_case(recorded))
    };
    if let Some(spelling) = section_spelling(spellings(), recorded) {
        return RepoAttribution::Exact {
            id: RepoId::new(spelling),
        };
    }
    if spellings().next().is_some() {
        // Sorted and deduplicated by raw spelling, not `RepoId`'s folded
        // `Ord`, which calls every case variant equal: this list
        // serializes straight into a byte-pinned contract snapshot.
        let variants: std::collections::BTreeSet<&str> = spellings().collect();
        return RepoAttribution::Ambiguous {
            recorded: recorded.to_string(),
            candidates: variants.into_iter().map(RepoId::new).collect(),
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
        n if n > 1 => {
            // Sorted by raw spelling, not `RepoId`'s folded `Ord` (same
            // reasoning as the exact-match arm above): `Vec::sort` is
            // stable, so sorting by the folded order would leave
            // case-variant candidates in `read_dir` order relative to
            // each other. This list serializes straight into a
            // byte-pinned contract snapshot.
            let mut candidates: Vec<RepoId> = substring_matches.into_iter().cloned().collect();
            candidates.sort_by(|a, b| a.as_str().cmp(b.as_str()));
            RepoAttribution::Ambiguous {
                recorded: recorded.to_string(),
                candidates,
            }
        }
        _ if recorded.is_empty() => RepoAttribution::Unrecorded,
        _ => RepoAttribution::Stale {
            recorded: recorded.to_string(),
        },
    }
}

/// The pseudo repo a recorded value names, if it names one.
///
/// Matched case-insensitively, same as the exact-match rule: dnf has been
/// observed to record `@CommandLine` (see repo_tier's `@CommandLine`
/// case), not just the lowercase spelling.
fn pseudo_repo(recorded: &str) -> Option<PseudoRepo> {
    match recorded.to_ascii_lowercase().as_str() {
        "@commandline" => Some(PseudoRepo::Commandline),
        "anaconda" => Some(PseudoRepo::Anaconda),
        "@system" => Some(PseudoRepo::System),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::types::FindingKind;
    use inspectah_core::types::rpm::PackageState;

    /// The `dnf repoquery` query, before the package names.
    const QUERY: &str = "dnf repoquery --installed --queryformat %{name} %{arch} %{from_repo}\n";

    /// Helper: build an `x86_64` PackageEntry with just the name set.
    fn pkg(name: &str) -> PackageEntry {
        pkg_arch(name, "x86_64")
    }

    /// Helper: build a PackageEntry with the name and arch set.
    fn pkg_arch(name: &str, arch: &str) -> PackageEntry {
        PackageEntry {
            name: name.into(),
            arch: arch.into(),
            state: PackageState::Added,
            ..Default::default()
        }
    }

    fn ok(stdout: &str) -> ExecResult {
        ExecResult {
            stdout: stdout.into(),
            exit_code: 0,
            ..Default::default()
        }
    }

    #[test]
    fn test_populate_via_dnf_repoquery() {
        let exec = MockExecutor::new()
            .with_command(
                &format!("{QUERY} bash"),
                ExecResult {
                    stdout: "bash x86_64 baseos\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                &format!("{QUERY} glibc httpd"),
                ExecResult {
                    stdout: "glibc x86_64 baseos\nhttpd x86_64 appstream\n".into(),
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
            &format!("{QUERY} bash"),
            ExecResult {
                stdout: "bash x86_64 @System\n".into(),
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
            &format!("{QUERY} bash"),
            ExecResult {
                stdout: "bash x86_64 \n".into(),
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
                &format!("{QUERY} bash"),
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
Architecture: x86_64
From repo   : baseos
Name        : httpd
Version     : 2.4.57
Release     : 5.el9
Architecture: x86_64
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
                &format!("{QUERY} bash"),
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
Architecture: x86_64
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
                &format!("{QUERY} bash"),
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
            &format!("{QUERY} bash"),
            ExecResult {
                stdout: "bash x86_64 baseos\nunknown-pkg x86_64 epel\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        );

        let mut packages = vec![pkg("bash")];
        populate_source_repos(&exec, &mut packages);

        assert_eq!(packages[0].source_repo, "baseos");
        // "unknown-pkg" is silently dropped because it's not in name_set
    }

    /// Each architecture keeps its own repo, whichever dnf lists first:
    /// `libvendor.i686` came from `appstream` and `libvendor.x86_64` from a
    /// local RPM path.
    #[test]
    fn each_arch_keeps_its_own_repo() {
        let exec = MockExecutor::new().with_command(
            &format!("{QUERY} libvendor"),
            ok("libvendor i686 appstream\nlibvendor x86_64 @commandline\n"),
        );

        let mut packages = vec![
            pkg_arch("libvendor", "x86_64"),
            pkg_arch("libvendor", "i686"),
        ];
        populate_source_repos(&exec, &mut packages);

        assert_eq!(packages[0].source_repo, "@commandline");
        assert_eq!(packages[1].source_repo, "appstream");
    }

    /// What cannot be told apart is unknown: two records for one
    /// `name.arch` that name different repos, and a record with no
    /// architecture, which could be any entry of its name. A repeated
    /// record that agrees is kept.
    #[test]
    fn a_record_that_cannot_be_told_apart_leaves_the_repo_unknown() {
        let cases: [(&str, &str, &str); 4] = [
            (
                "conflicting",
                "bash x86_64 baseos\nbash x86_64 appstream\n",
                "",
            ),
            (
                "agreeing",
                "bash x86_64 baseos\nbash x86_64 baseos\n",
                "baseos",
            ),
            ("no arch", "bash baseos\n", ""),
            (
                "no arch beside an arch",
                "bash x86_64 baseos\nbash appstream\n",
                "",
            ),
        ];
        for (case, stdout, expected) in cases {
            let exec = MockExecutor::new().with_command(&format!("{QUERY} bash"), ok(stdout));
            let mut packages = vec![pkg("bash")];
            populate_source_repos(&exec, &mut packages);
            assert_eq!(packages[0].source_repo, expected, "{case}");
        }
    }

    /// The `rpm -qi` fallback keeps each architecture's repo, and a record
    /// whose architecture it does not print leaves its name unknown.
    #[test]
    fn the_rpm_fallback_keeps_each_arch_apart() {
        let failed = ExecResult {
            exit_code: 1,
            ..Default::default()
        };
        let exec = MockExecutor::new()
            .with_command(&format!("{QUERY} glibc"), failed)
            .with_command(
                "rpm -qi glibc libvendor",
                ok("\
Name        : glibc
Version     : 2.34
From repo   : baseos
Name        : libvendor
Architecture: i686
From repo   : appstream
Name        : libvendor
Architecture: x86_64
From repo   : @commandline
"),
            );

        let mut packages = vec![
            pkg("glibc"),
            pkg_arch("libvendor", "x86_64"),
            pkg_arch("libvendor", "i686"),
        ];
        populate_source_repos(&exec, &mut packages);

        assert_eq!(packages[0].source_repo, "", "glibc's record has no arch");
        assert_eq!(packages[1].source_repo, "@commandline");
        assert_eq!(packages[2].source_repo, "appstream");
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
            gpgkey_references: Default::default(),
            shipped_by_base: None,
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
    fn test_normalize_no_repo_files_is_stale() {
        // With zero sections, a non-empty, non-pseudo recorded value has
        // nothing to match: § 3.2 calls that Stale, not Unrecorded (the
        // host did record a value) and not a no-op (source_repo is left
        // as recorded, but the attribution is a real classification).
        let repo_files: Vec<RepoFile> = Vec::new();
        let mut packages = vec![pkg_with_repo("httpd", "AppStream")];

        normalize_source_repos(&mut packages, &repo_files);

        assert_eq!(
            packages[0].source_repo, "AppStream",
            "no repo files means source_repo is left as recorded"
        );
        assert!(matches!(
            &packages[0].repo_attribution,
            RepoAttribution::Stale { recorded } if recorded == "AppStream"
        ));
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
    fn normalization_keeps_the_recorded_spelling_of_a_case_variant_id() {
        // dnf ids are case-sensitive: `[ab]` and `[AB]` are two repos, in one
        // file or in two, and a package dnf recorded from `ab` came from
        // `[ab]`. A lone case variant stands in for the recorded spelling;
        // two with no exact spelling are ambiguous. Every case runs with
        // the files in both orders, and asserts the raw spelling, since
        // `RepoId`'s own equality folds case.
        let lower = "[ab]\nbaseurl=https://example.invalid/lower\nenabled=0\n";
        let upper = "[AB]\nbaseurl=https://example.invalid/upper\nenabled=1\n";
        let same_file = format!("{lower}{upper}");
        // `(what, file contents, recorded, source_repo after, Exact id or
        // the Ambiguous candidates)`.
        type Case<'a> = (
            &'a str,
            Vec<&'a str>,
            &'a str,
            &'a str,
            Result<&'a str, Vec<&'a str>>,
        );
        let cases: Vec<Case> = vec![
            (
                "same file, recorded ab",
                vec![&same_file],
                "ab",
                "ab",
                Ok("ab"),
            ),
            (
                "same file, recorded AB",
                vec![&same_file],
                "AB",
                "AB",
                Ok("AB"),
            ),
            (
                "cross file, recorded ab",
                vec![upper, lower],
                "ab",
                "ab",
                Ok("ab"),
            ),
            (
                "cross file, recorded AB",
                vec![upper, lower],
                "AB",
                "AB",
                Ok("AB"),
            ),
            ("one case variant", vec![upper], "ab", "AB", Ok("AB")),
            (
                "two case variants, neither recorded",
                vec![&same_file],
                "Ab",
                "Ab",
                Err(vec!["AB", "ab"]),
            ),
        ];
        for (what, contents, recorded, after, attribution) in cases {
            let files: Vec<RepoFile> = contents.iter().map(|c| repo_file(c)).collect();
            let mut reversed = files.clone();
            reversed.reverse();
            for (order, set) in [("in order", files), ("reversed", reversed)] {
                let mut pkgs = vec![pkg_with_repo("tool", recorded)];
                normalize_source_repos(&mut pkgs, &set);
                assert_eq!(pkgs[0].source_repo, after, "{what}, {order}: source_repo");
                let got = match &pkgs[0].repo_attribution {
                    RepoAttribution::Exact { id } => Ok(id.as_str()),
                    RepoAttribution::Ambiguous { candidates, .. } => {
                        Err(candidates.iter().map(RepoId::as_str).collect())
                    }
                    other => panic!("{what}, {order}: {other:?}"),
                };
                assert_eq!(got, attribution, "{what}, {order}: attribution");
            }
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
        // Sorted, not read_dir order: proves F4's fix, not just that two
        // candidates were found.
        assert!(matches!(
            &pkgs[0].repo_attribution,
            RepoAttribution::Ambiguous { recorded, candidates }
                if recorded == "baseos" && candidates == &[
                    RepoId::new("rhel-9-for-x86_64-baseos-debug-rpms"),
                    RepoId::new("rhel-9-for-x86_64-baseos-rpms"),
                ]
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

        // "@CommandLine" (observed casing, see repo_tier's own
        // "@CommandLine" case) -> Pseudo(Commandline), case-folded like the
        // lowercase spelling.
        let mut pkgs = vec![pkg_with_repo("local-tool", "@CommandLine")];
        normalize_source_repos(&mut pkgs, &disabled_only);
        assert!(matches!(
            &pkgs[0].repo_attribution,
            RepoAttribution::Pseudo { which: PseudoRepo::Commandline, recorded } if recorded == "@CommandLine"
        ));
    }

    #[test]
    fn a_pseudo_repo_stays_pseudo_whatever_a_section_is_called() {
        // A pseudo source names no repo, so a `.repo` section spelled the
        // same, enabled or not, or one containing it, does not turn it
        // into one: pseudo names are recognized before section matching.
        let sections = "[anaconda]\nenabled=0\nbaseurl=https://example.invalid/a\n\
                        [@commandline]\nbaseurl=https://example.invalid/c\n\
                        [@System]\nbaseurl=https://example.invalid/s\n\
                        [anaconda-extras]\nbaseurl=https://example.invalid/x\n";
        let files = vec![repo_file(sections)];
        let cases = [
            ("anaconda", PseudoRepo::Anaconda),
            ("Anaconda", PseudoRepo::Anaconda),
            ("@commandline", PseudoRepo::Commandline),
            ("@System", PseudoRepo::System),
        ];
        for (recorded, which) in cases {
            let mut pkgs = vec![pkg_with_repo("tool", recorded)];
            normalize_source_repos(&mut pkgs, &files);
            assert_eq!(
                pkgs[0].repo_attribution,
                RepoAttribution::Pseudo {
                    which,
                    recorded: recorded.into()
                },
                "{recorded}"
            );
            assert_eq!(
                pkgs[0].source_repo, recorded,
                "{recorded} is left as recorded"
            );
        }
    }

    #[test]
    fn exact_match_duplicate_sections_is_order_independent() {
        // F4(a) regression: two files defining the same repo id with
        // different ASCII case (`[EPEL]`, `[epel]`) are both exact matches
        // for a recorded value of "epel" -- `RepoId`'s folded `Eq` makes
        // them equal to each other, so the choice between them cannot be
        // made via `RepoId`'s own (also folded) `Ord` without degenerating
        // to "whichever `read_dir` returned first". The written spelling
        // must be the same regardless of file order.
        let upper_first = vec![
            repo_file(
                "[EPEL]\nname=EPEL upper\nenabled=1\nbaseurl=https://example.invalid/epel-upper\n",
            ),
            repo_file(
                "[epel]\nname=EPEL lower\nenabled=1\nbaseurl=https://example.invalid/epel-lower\n",
            ),
        ];
        let mut lower_first = upper_first.clone();
        lower_first.reverse();

        let mut pkgs_a = vec![pkg_with_repo("htop", "epel")];
        normalize_source_repos(&mut pkgs_a, &upper_first);
        let mut pkgs_b = vec![pkg_with_repo("htop", "epel")];
        normalize_source_repos(&mut pkgs_b, &lower_first);

        assert_eq!(pkgs_a[0].source_repo, pkgs_b[0].source_repo);
    }
}
