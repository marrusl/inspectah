//! Integration tests for repo-less RPM detection and dnf cache scanning.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::rpm::repoless::scan_dnf_cache_for_repoless;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::types::rpm::{PackageEntry, PackageState};

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

    let mut packages = vec![pkg("custom-tool", "1.2.3", "1.el9", "x86_64", "")];
    scan_dnf_cache_for_repoless(&exec, &mut packages);

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

    let mut packages = vec![pkg("custom-tool", "1.2.3", "1.el9", "x86_64", "")];
    scan_dnf_cache_for_repoless(&exec, &mut packages);

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

    let mut packages = vec![pkg("httpd", "2.4.57", "5.el9", "x86_64", "appstream")];
    scan_dnf_cache_for_repoless(&exec, &mut packages);

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
    let mut packages = vec![pkg(
        "internal-agent",
        "2.0",
        "1.el9",
        "x86_64",
        "internal-tools",
    )];
    scan_dnf_cache_for_repoless(&exec, &mut packages);

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

    let mut packages = vec![
        pkg("httpd", "2.4.57", "5.el9", "x86_64", "appstream"),
        pkg("custom-tool", "1.0", "1.el9", "x86_64", ""),
        pkg("bash", "5.2.26", "3.el9", "x86_64", "baseos"),
    ];
    scan_dnf_cache_for_repoless(&exec, &mut packages);

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

    let mut packages = vec![
        pkg("httpd", "2.4.57", "5.el9", "x86_64", "appstream"),
        pkg("custom-tool", "1.0", "1.el9", "x86_64", ""),
    ];
    scan_dnf_cache_for_repoless(&exec, &mut packages);

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
