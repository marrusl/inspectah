//! Integration tests for repo-less RPM detection and dnf cache scanning.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::rpm::repoless::scan_dnf_cache_for_repoless;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::types::FindingKind;
use inspectah_core::types::repo::RepoId;
use inspectah_core::types::rpm::{
    FileOwnershipEntry, PackageEntry, PackageState, RepoFile, RpmSection,
};

/// Helper: build a PackageEntry with the given fields.
fn pkg(name: &str, version: &str, release: &str, arch: &str, source_repo: &str) -> PackageEntry {
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

/// Build a MockExecutor with enabled repos and cache listing.
fn build_executor(enabled_repos_stdout: &str, cache_listing_stdout: &str) -> MockExecutor {
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
    let exec = build_executor(
        "repo id                       repo name\nappstream                     RHEL 9 AppStream\n",
        "/var/cache/dnf/custom-repo/packages/custom-tool-1.2.3-1.el9.x86_64.rpm\n",
    );

    let packages = scan_packages(
        &exec,
        vec![pkg("custom-tool", "1.2.3", "1.el9", "x86_64", "")],
    );

    assert!(packages[0].repoless_cached, "cached RPM should be detected");
    assert_eq!(
        packages[0].cache_path,
        Some("/var/cache/dnf/custom-repo/packages/custom-tool-1.2.3-1.el9.x86_64.rpm".into())
    );
    assert!(
        packages[0]
            .repoless_annotation
            .contains("cached RPM bundled"),
        "annotation should mention bundling"
    );
}

#[test]
fn repoless_rpm_not_in_cache() {
    let exec = build_executor(
        "repo id                       repo name\nappstream                     RHEL 9 AppStream\n",
        "", // empty cache
    );

    let packages = scan_packages(
        &exec,
        vec![pkg("custom-tool", "1.2.3", "1.el9", "x86_64", "")],
    );

    assert!(!packages[0].repoless_cached, "should not be cached");
    assert!(
        packages[0].cache_path.is_none(),
        "no cache_path when not found"
    );
    assert!(
        packages[0]
            .repoless_annotation
            .contains("manual resolution needed"),
        "annotation should direct user to manual resolution"
    );
}

#[test]
fn rpm_with_source_repo_not_flagged() {
    let exec = build_executor(
        "repo id                       repo name\nappstream                     RHEL 9 AppStream\n",
        "",
    );

    let packages = scan_packages(
        &exec,
        vec![pkg("httpd", "2.4.57", "5.el9", "x86_64", "appstream")],
    );

    // Package has an enabled source_repo — should not be treated as repo-less.
    assert!(!packages[0].repoless_cached);
    assert!(packages[0].cache_path.is_none());
    assert!(
        packages[0].repoless_annotation.is_empty(),
        "no annotation for packages with enabled source repo"
    );
}

#[test]
fn rpm_with_disabled_repo_detected_as_repoless() {
    let exec = build_executor(
        "repo id                       repo name\nappstream                     RHEL 9 AppStream\n",
        "/var/cache/dnf/internal-tools/packages/internal-agent-2.0-1.el9.x86_64.rpm\n",
    );

    // source_repo is "internal-tools" but that's not in enabled repos.
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

    assert!(
        packages[0].repoless_cached,
        "disabled repo should trigger repoless detection"
    );
    assert!(
        packages[0].repoless_annotation.contains("internal-tools"),
        "annotation should mention the disabled repo name"
    );
    assert!(
        packages[0]
            .repoless_annotation
            .contains("not in enabled repos"),
        "annotation should explain why it's repo-less"
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
        cache_path: Some("/var/cache/dnf/repo/packages/custom-tool-1.2.3-1.el9.x86_64.rpm".into()),
        repoless_annotation: "No repo source".into(),
        ..Default::default()
    };

    let json = serde_json::to_string(&entry).unwrap();
    let parsed: PackageEntry = serde_json::from_str(&json).unwrap();

    assert_eq!(
        parsed.cache_path, entry.cache_path,
        "cache_path must survive roundtrip"
    );
    assert_eq!(
        parsed.repoless_cached, entry.repoless_cached,
        "repoless_cached must survive roundtrip"
    );
    assert_eq!(
        parsed.repoless_annotation, entry.repoless_annotation,
        "repoless_annotation must survive roundtrip"
    );
}

#[test]
fn mixed_batch_only_repoless_annotated() {
    let exec = build_executor(
        "repo id                       repo name\nappstream                     RHEL 9 AppStream\nbaseos                        RHEL 9 BaseOS\n",
        "/var/cache/dnf/local/packages/custom-tool-1.0-1.el9.x86_64.rpm\n",
    );

    let packages = scan_packages(
        &exec,
        vec![
            pkg("httpd", "2.4.57", "5.el9", "x86_64", "appstream"),
            pkg("custom-tool", "1.0", "1.el9", "x86_64", ""),
            pkg("bash", "5.2.26", "3.el9", "x86_64", "baseos"),
        ],
    );

    // Only custom-tool should be annotated.
    assert!(
        packages[0].repoless_annotation.is_empty(),
        "httpd should not be annotated"
    );
    assert!(packages[1].repoless_cached, "custom-tool should be cached");
    assert!(
        packages[2].repoless_annotation.is_empty(),
        "bash should not be annotated"
    );
}

#[test]
fn dnf_repolist_failure_skips_named_repo_packages() {
    // When dnf repolist fails, packages with a named source_repo must NOT
    // be flagged as repo-less. Only packages with empty source_repo are
    // processed. This prevents overfiring when dnf is unavailable.
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
                stdout: "/var/cache/dnf/local/packages/custom-tool-1.0-1.el9.x86_64.rpm\n".into(),
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
        "httpd should not be flagged when dnf repolist fails"
    );

    // custom-tool has empty source_repo -- should still be detected
    assert!(
        packages[1].repoless_cached,
        "custom-tool with empty source_repo should be detected even when dnf fails"
    );
}

#[test]
fn short_name_matches_full_repo_id() {
    // Real RHEL scenario: dnf repolist returns full repo IDs,
    // source_repo has install-time short names from %{from_repo}.
    let exec = build_executor(
        "repo id                              repo name\nrhel-9-for-x86_64-appstream-rpms     RHEL 9 AppStream\nrhel-9-for-x86_64-baseos-rpms        RHEL 9 BaseOS\n",
        "",
    );

    let packages = scan_packages(
        &exec,
        vec![
            pkg("httpd", "2.4.57", "5.el9", "x86_64", "AppStream"),
            pkg("bash", "5.2.26", "3.el9", "x86_64", "baseos"),
        ],
    );

    assert!(
        packages[0].repoless_annotation.is_empty(),
        "AppStream should match rhel-9-for-x86_64-appstream-rpms via substring"
    );
    assert!(
        packages[1].repoless_annotation.is_empty(),
        "baseos should match rhel-9-for-x86_64-baseos-rpms via substring"
    );
}

#[test]
fn anaconda_source_repo_still_flagged() {
    // anaconda is the install-time source, not a real repo.
    let exec = build_executor(
        "repo id                              repo name\nrhel-9-for-x86_64-appstream-rpms     RHEL 9 AppStream\nrhel-9-for-x86_64-baseos-rpms        RHEL 9 BaseOS\n",
        "",
    );

    let packages = scan_packages(
        &exec,
        vec![pkg("kernel", "5.14.0", "362.el9", "x86_64", "anaconda")],
    );

    assert!(
        !packages[0].repoless_annotation.is_empty(),
        "anaconda source_repo should still be flagged as repo-less"
    );
}

// --- Repo-enabling classification (spec § 3.3) ---

const EPEL_REPO_PATH: &str = "/etc/yum.repos.d/epel.repo";
const EPEL_KEY_PATH: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9";
const VENDOR_REPO_PATH: &str = "/etc/yum.repos.d/vendor.repo";
const VENDOR_KEY_PATH: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-VENDOR";
const BOOTSTRAP_REPO_PATH: &str = "/etc/yum.repos.d/bootstrap.repo";

const EPEL_REPO_CONTENT: &str = "[epel]\nname=EPEL 9\nenabled=1\n";
const VENDOR_REPO_CONTENT: &str = "[vendor]\nname=Vendor workloads\nenabled=1\n";
const VENDOR_REPO_DISABLED_CONTENT: &str = "[vendor]\nname=Vendor workloads\nenabled=0\n";
const BOOTSTRAP_REPO_CONTENT: &str = "[bootstrap]\nname=Vendor bootstrap\nenabled=1\n";
const GPG_KEY_CONTENT: &str = "-----BEGIN PGP PUBLIC KEY BLOCK-----\n";

/// The annotation an `@commandline` candidate gets when classification
/// does not apply to it: the reason the scan has always produced, plus
/// the cache-scan suffix.
const PLAIN_COMMANDLINE_ANNOTATION: &str = "No repo source \u{2014} repo '@commandline' not in enabled repos \u{2014} cached RPM bundled (pre-excluded, no GPG verification)";

/// The annotation for a candidate that provides repository configuration
/// the export carries but that no enabled repo serves by name.
const OWNS_ONE_REPO_FILE_ANNOTATION: &str = "No repo source: owns 1 captured repo file(s) but is not available from them \u{2014} cached RPM bundled (pre-excluded, no GPG verification)";

fn repo_file(path: &str, content: &str) -> RepoFile {
    RepoFile {
        path: path.into(),
        content: content.into(),
        is_default_repo: false,
        disposition: FindingKind::included(),
        locked: false,
        aggregate: None,
    }
}

/// What the scan should have done to the fixture's one package. Every
/// fixture puts that package's RPM in the dnf cache, so each variant
/// pins `cache_path` as well — a package that left the repo-less set
/// before the cache scan cannot have picked it up.
enum Expect {
    /// Repo-enabling from these sections: no note, no cached RPM.
    Enabling(&'static [&'static str]),
    /// Still repo-less: this exact note, and the cached RPM is bundled.
    Repoless(&'static str),
    /// Neither. The candidate was cleared by the availability check
    /// without ever being repo-enabling, so it carries no note at all.
    Cleared,
}

struct ClassificationCase {
    label: &'static str,
    /// name, version, release, arch.
    package: (&'static str, &'static str, &'static str, &'static str),
    /// The package's recorded `source_repo`. Anything the enabled-repo
    /// list does not match makes it a candidate.
    source_repo: &'static str,
    /// `rpm.repo_files`, as (path, content).
    repo_files: &'static [(&'static str, &'static str)],
    /// `rpm.gpg_keys` paths.
    gpg_keys: &'static [&'static str],
    /// The paths the package's `file_ownership` entry lists. `None`
    /// gives the package no entry at all, as a degraded scan does.
    owned: Option<&'static [&'static str]>,
    /// `None` leaves `dnf repoquery` unregistered, which the mock answers
    /// with exit 127 — the "dnf could not tell us" path.
    repoquery_stdout: Option<&'static str>,
    expect: Expect,
}

#[test]
fn repo_enabling_classification_table() {
    const COMMANDLINE: &str = "@commandline";

    let cases = [
        ClassificationCase {
            label: "owns a captured epel.repo and key, and [epel] serves it",
            package: ("epel-release", "9", "11.el9", "noarch"),
            source_repo: COMMANDLINE,
            repo_files: &[(EPEL_REPO_PATH, EPEL_REPO_CONTENT)],
            gpg_keys: &[EPEL_KEY_PATH],
            // The doc file is not repository configuration, so it is not
            // one of the paths the subset test looks at.
            owned: Some(&[
                EPEL_REPO_PATH,
                EPEL_KEY_PATH,
                "/usr/share/doc/epel-release-9/README",
            ]),
            repoquery_stdout: Some("epel-release epel\nbash baseos\n"),
            expect: Expect::Enabling(&["epel"]),
        },
        ClassificationCase {
            label: "owns a second repo file the export is not carrying",
            package: ("epel-release", "9", "11.el9", "noarch"),
            source_repo: COMMANDLINE,
            repo_files: &[(EPEL_REPO_PATH, EPEL_REPO_CONTENT)],
            gpg_keys: &[EPEL_KEY_PATH],
            owned: Some(&[
                EPEL_REPO_PATH,
                "/etc/yum.repos.d/epel-testing.repo",
                EPEL_KEY_PATH,
            ]),
            repoquery_stdout: Some("epel-release epel\n"),
            expect: Expect::Repoless(PLAIN_COMMANDLINE_ANNOTATION),
        },
        ClassificationCase {
            label: "owns the captured key but no repo file",
            package: ("epel-release", "9", "11.el9", "noarch"),
            source_repo: COMMANDLINE,
            repo_files: &[(EPEL_REPO_PATH, EPEL_REPO_CONTENT)],
            gpg_keys: &[EPEL_KEY_PATH],
            owned: Some(&[EPEL_KEY_PATH]),
            repoquery_stdout: Some("epel-release epel\n"),
            expect: Expect::Repoless(PLAIN_COMMANDLINE_ANNOTATION),
        },
        ClassificationCase {
            // Only the `@commandline` test refuses this one. Without it
            // the package is recorded as repo-enabling from `vendor`, and
            // the export renders an active install naming a repo the
            // operator had switched off.
            label: "a -release package from a disabled repo that an enabled repo serves",
            package: ("vendor-release", "1.0", "1.el9", "noarch"),
            source_repo: "vendor-disabled",
            repo_files: &[(VENDOR_REPO_PATH, VENDOR_REPO_CONTENT)],
            gpg_keys: &[VENDOR_KEY_PATH],
            owned: Some(&[VENDOR_REPO_PATH, VENDOR_KEY_PATH]),
            repoquery_stdout: Some("vendor-release vendor\n"),
            expect: Expect::Cleared,
        },
        ClassificationCase {
            // A degraded scan collects no ownership data. The early
            // return for it is redundant with the repo-file count below,
            // which refuses the same input; this row pins the behaviour,
            // not that guard.
            label: "the file_ownership entry is missing, as on a degraded scan",
            package: ("epel-release", "9", "11.el9", "noarch"),
            source_repo: COMMANDLINE,
            repo_files: &[(EPEL_REPO_PATH, EPEL_REPO_CONTENT)],
            gpg_keys: &[EPEL_KEY_PATH],
            owned: None,
            repoquery_stdout: Some("epel-release epel\n"),
            expect: Expect::Repoless(PLAIN_COMMANDLINE_ANNOTATION),
        },
        ClassificationCase {
            label: "a captured, served repo owner that is not named -release",
            package: ("vendor-config", "1.0", "1.el9", "noarch"),
            source_repo: COMMANDLINE,
            repo_files: &[(VENDOR_REPO_PATH, VENDOR_REPO_CONTENT)],
            gpg_keys: &[VENDOR_KEY_PATH],
            owned: Some(&[VENDOR_REPO_PATH, VENDOR_KEY_PATH]),
            repoquery_stdout: Some("vendor-config vendor\n"),
            expect: Expect::Repoless(PLAIN_COMMANDLINE_ANNOTATION),
        },
        ClassificationCase {
            // The brief's own row. It is refused by the repo-file count
            // before the subset check runs, so it does not test the
            // subset check -- "owns a second repo file" above does.
            label: "epel.repo is not among the captured repo files",
            package: ("epel-release", "9", "11.el9", "noarch"),
            source_repo: COMMANDLINE,
            repo_files: &[],
            gpg_keys: &[EPEL_KEY_PATH],
            owned: Some(&[EPEL_REPO_PATH, EPEL_KEY_PATH]),
            repoquery_stdout: Some("epel-release epel\n"),
            expect: Expect::Repoless(PLAIN_COMMANDLINE_ANNOTATION),
        },
        ClassificationCase {
            label: "owns a fully captured vendor.repo but nothing serves it",
            package: ("vendor-release", "1.0", "1.el9", "noarch"),
            source_repo: COMMANDLINE,
            repo_files: &[(VENDOR_REPO_PATH, VENDOR_REPO_CONTENT)],
            gpg_keys: &[VENDOR_KEY_PATH],
            owned: Some(&[VENDOR_REPO_PATH, VENDOR_KEY_PATH]),
            repoquery_stdout: Some("vendor-agent vendor\n"),
            expect: Expect::Repoless(OWNS_ONE_REPO_FILE_ANNOTATION),
        },
        ClassificationCase {
            // Classification never reads repo file content, so this row
            // is driven by the availability answer alone and exercises
            // the same path as the one above.
            label: "the same with vendor.repo's only section disabled",
            package: ("vendor-release", "1.0", "1.el9", "noarch"),
            source_repo: COMMANDLINE,
            repo_files: &[(VENDOR_REPO_PATH, VENDOR_REPO_DISABLED_CONTENT)],
            gpg_keys: &[VENDOR_KEY_PATH],
            owned: Some(&[VENDOR_REPO_PATH, VENDOR_KEY_PATH]),
            repoquery_stdout: Some("vendor-agent vendor\n"),
            expect: Expect::Repoless(OWNS_ONE_REPO_FILE_ANNOTATION),
        },
        ClassificationCase {
            label: "the same with the output listing vendor-release from vendor",
            package: ("vendor-release", "1.0", "1.el9", "noarch"),
            source_repo: COMMANDLINE,
            repo_files: &[(VENDOR_REPO_PATH, VENDOR_REPO_CONTENT)],
            gpg_keys: &[VENDOR_KEY_PATH],
            owned: Some(&[VENDOR_REPO_PATH, VENDOR_KEY_PATH]),
            repoquery_stdout: Some("vendor-release vendor\n"),
            expect: Expect::Enabling(&["vendor"]),
        },
        ClassificationCase {
            label: "owns vendor.repo while a captured bootstrap.repo serves it",
            package: ("vendor-release", "1.0", "1.el9", "noarch"),
            source_repo: COMMANDLINE,
            repo_files: &[
                (VENDOR_REPO_PATH, VENDOR_REPO_CONTENT),
                (BOOTSTRAP_REPO_PATH, BOOTSTRAP_REPO_CONTENT),
            ],
            gpg_keys: &[VENDOR_KEY_PATH],
            owned: Some(&[VENDOR_REPO_PATH, VENDOR_KEY_PATH]),
            repoquery_stdout: Some("vendor-release bootstrap\n"),
            expect: Expect::Enabling(&["bootstrap"]),
        },
        ClassificationCase {
            label: "repoquery failed, so availability was never proven",
            package: ("epel-release", "9", "11.el9", "noarch"),
            source_repo: COMMANDLINE,
            repo_files: &[(EPEL_REPO_PATH, EPEL_REPO_CONTENT)],
            gpg_keys: &[EPEL_KEY_PATH],
            owned: Some(&[EPEL_REPO_PATH, EPEL_KEY_PATH]),
            repoquery_stdout: None,
            expect: Expect::Repoless(PLAIN_COMMANDLINE_ANNOTATION),
        },
        ClassificationCase {
            label: "every output line carries one token",
            package: ("epel-release", "9", "11.el9", "noarch"),
            source_repo: COMMANDLINE,
            repo_files: &[(EPEL_REPO_PATH, EPEL_REPO_CONTENT)],
            gpg_keys: &[EPEL_KEY_PATH],
            owned: Some(&[EPEL_REPO_PATH, EPEL_KEY_PATH]),
            repoquery_stdout: Some("epel-release\nbash\n"),
            expect: Expect::Repoless(OWNS_ONE_REPO_FILE_ANNOTATION),
        },
        ClassificationCase {
            label: "two builds in one repo collapse to one serving id",
            package: ("epel-release", "9", "11.el9", "noarch"),
            source_repo: COMMANDLINE,
            repo_files: &[(EPEL_REPO_PATH, EPEL_REPO_CONTENT)],
            gpg_keys: &[EPEL_KEY_PATH],
            owned: Some(&[EPEL_REPO_PATH, EPEL_KEY_PATH]),
            repoquery_stdout: Some("epel-release epel\nepel-release epel\n"),
            expect: Expect::Enabling(&["epel"]),
        },
        ClassificationCase {
            label: "two serving repos are recorded in a stable order",
            package: ("epel-release", "9", "11.el9", "noarch"),
            source_repo: COMMANDLINE,
            repo_files: &[(EPEL_REPO_PATH, EPEL_REPO_CONTENT)],
            gpg_keys: &[EPEL_KEY_PATH],
            owned: Some(&[EPEL_REPO_PATH, EPEL_KEY_PATH]),
            // dnf printed epel-next first; the snapshot must not depend
            // on the order the host happened to answer in.
            repoquery_stdout: Some("epel-release epel-next\nepel-release epel\n"),
            expect: Expect::Enabling(&["epel", "epel-next"]),
        },
    ];

    for case in cases {
        let (name, version, release, arch) = case.package;
        let cached_rpm =
            format!("/var/cache/dnf/local/packages/{name}-{version}-{release}.{arch}.rpm");

        let mut exec = MockExecutor::new()
            .with_command(
                "dnf repolist --enabled -q",
                ExecResult {
                    stdout: "repo id    repo name\nappstream    RHEL 9 AppStream\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "find /var/cache/dnf -name *.rpm -type f",
                ExecResult {
                    stdout: format!("{cached_rpm}\n"),
                    exit_code: 0,
                    ..Default::default()
                },
            );
        if let Some(stdout) = case.repoquery_stdout {
            exec = exec.with_command(
                "dnf repoquery --available --queryformat %{name} %{reponame}\n -q",
                ExecResult {
                    stdout: stdout.into(),
                    exit_code: 0,
                    ..Default::default()
                },
            );
        }

        let mut rpm = RpmSection {
            packages_added: vec![pkg(name, version, release, arch, case.source_repo)],
            repo_files: case
                .repo_files
                .iter()
                .map(|(path, content)| repo_file(path, content))
                .collect(),
            gpg_keys: case
                .gpg_keys
                .iter()
                .map(|path| repo_file(path, GPG_KEY_CONTENT))
                .collect(),
            file_ownership: case
                .owned
                .map(|paths| {
                    vec![FileOwnershipEntry {
                        package_name: name.into(),
                        paths: paths.iter().map(|p| (*p).to_string()).collect(),
                    }]
                })
                .unwrap_or_default(),
            ..Default::default()
        };
        scan_dnf_cache_for_repoless(&exec, &mut rpm);

        let got = &rpm.packages_added[0];
        let label = case.label;
        match case.expect {
            Expect::Enabling(ids) => {
                let expected: Vec<RepoId> = ids.iter().map(|id| RepoId::new(*id)).collect();
                assert_eq!(
                    got.repo_enabling_from, expected,
                    "{label}: repo_enabling_from"
                );
                assert_eq!(got.repoless_annotation, "", "{label}: repoless_annotation");
                assert!(
                    !got.repoless_cached,
                    "{label}: a repo-enabling package leaves the repo-less set before the cache scan"
                );
                assert_eq!(got.cache_path, None, "{label}: cache_path");
            }
            Expect::Repoless(note) => {
                assert!(
                    got.repo_enabling_from.is_empty(),
                    "{label}: repo_enabling_from should be empty, got {:?}",
                    got.repo_enabling_from
                );
                assert_eq!(
                    got.repoless_annotation, note,
                    "{label}: repoless_annotation"
                );
                assert!(got.repoless_cached, "{label}: the cached RPM must be found");
                assert_eq!(
                    got.cache_path,
                    Some(cached_rpm.clone()),
                    "{label}: cache_path"
                );
            }
            Expect::Cleared => {
                assert!(
                    got.repo_enabling_from.is_empty(),
                    "{label}: repo_enabling_from should be empty, got {:?}",
                    got.repo_enabling_from
                );
                assert_eq!(got.repoless_annotation, "", "{label}: repoless_annotation");
                assert!(!got.repoless_cached, "{label}: repoless_cached");
                assert_eq!(got.cache_path, None, "{label}: cache_path");
            }
        }
    }
}
