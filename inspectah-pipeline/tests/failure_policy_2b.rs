//! Failure policy tests for Slice 2b inspectors — Network, Containers, UsersGroups.
//!
//! Verifies degraded/failed semantics match spec:
//! - Network: PermissionDenied on NM dir → Degraded
//! - Containers: podman ps failure → Degraded
//! - UsersGroups: passwd failure → Failed, shadow PermissionDenied → Degraded

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::containers::ContainersInspector;
use inspectah_collect::inspectors::network::NetworkInspector;
use inspectah_collect::inspectors::users::UsersGroupsInspector;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::completeness::{Completeness, InspectorId};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_pipeline::collect::collect;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn test_os_release() -> OsRelease {
    OsRelease {
        name: "Red Hat Enterprise Linux".into(),
        version_id: "9.4".into(),
        id: "rhel".into(),
        ..Default::default()
    }
}

fn package_based_source() -> SourceSystem {
    SourceSystem::PackageBased {
        os_release: test_os_release(),
    }
}

fn minimal_rpm_mock(exec: MockExecutor) -> MockExecutor {
    exec.with_command(
        "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
        ExecResult {
            stdout: "0:bash-5.2.26-3.el9.x86_64\n".into(),
            exit_code: 0,
            ..Default::default()
        },
    )
}

// ---------------------------------------------------------------------------
// Network inspector failure tests
// ---------------------------------------------------------------------------

#[test]
fn network_permission_denied_degraded() {
    // NetworkManager dir is present but unreadable → Degraded.
    let exec = minimal_rpm_mock(MockExecutor::new())
        .with_dir_error(
            "/etc/NetworkManager/system-connections",
            std::io::ErrorKind::PermissionDenied,
        )
        .with_command(
            "ip route",
            ExecResult {
                stdout: "default via 192.168.1.1 dev eth0\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "ip rule",
            ExecResult {
                stdout: String::new(),
                exit_code: 0,
                ..Default::default()
            },
        );

    let source = package_based_source();
    let inspectors: Vec<Box<dyn inspectah_core::traits::inspector::Inspector>> =
        vec![Box::new(NetworkInspector::new())];

    let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress);
    let snapshot = &pipeline.state.snapshot;

    match &snapshot.completeness {
        Completeness::Partial {
            degraded_sections, ..
        } => {
            assert!(
                degraded_sections.contains(&InspectorId::Network),
                "Network inspector should be degraded when NM dir is unreadable"
            );
        }
        other => panic!("Expected Completeness::Partial, got {:?}", other),
    }

    // Verify partial data was still collected (ip route output).
    assert!(
        snapshot.network.is_some(),
        "Network section should be present even when degraded"
    );
}

#[test]
fn network_not_found_not_degraded() {
    // All network directories missing (no NM, no firewalld) → Complete.
    // This is a valid state: system with no network management.
    let exec = minimal_rpm_mock(MockExecutor::new())
        .with_command(
            "ip route",
            ExecResult {
                stdout: String::new(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "ip rule",
            ExecResult {
                stdout: String::new(),
                exit_code: 0,
                ..Default::default()
            },
        );

    let source = package_based_source();
    let inspectors: Vec<Box<dyn inspectah_core::traits::inspector::Inspector>> =
        vec![Box::new(NetworkInspector::new())];

    let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress);
    let snapshot = &pipeline.state.snapshot;

    assert!(
        matches!(snapshot.completeness, Completeness::Complete),
        "Network inspector should be Complete when dirs are simply absent"
    );

    assert!(
        snapshot.network.is_some(),
        "Network section should be present with empty data"
    );
}

// ---------------------------------------------------------------------------
// Containers inspector failure tests
// ---------------------------------------------------------------------------

#[test]
fn containers_podman_json_parse_error_degraded() {
    // podman is installed but ps returns invalid JSON → Degraded.
    let exec = minimal_rpm_mock(MockExecutor::new())
        .with_command(
            "which podman",
            ExecResult {
                stdout: "/usr/bin/podman\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "podman ps --format json",
            ExecResult {
                exit_code: 0,
                stdout: "not valid json{{{".into(),
                ..Default::default()
            },
        );

    let source = package_based_source();
    let inspectors: Vec<Box<dyn inspectah_core::traits::inspector::Inspector>> =
        vec![Box::new(ContainersInspector::new())];

    let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress);
    let snapshot = &pipeline.state.snapshot;

    match &snapshot.completeness {
        Completeness::Partial {
            degraded_sections, ..
        } => {
            assert!(
                degraded_sections.contains(&InspectorId::Containers),
                "Containers inspector should be degraded when podman JSON is invalid"
            );
        }
        other => panic!("Expected Completeness::Partial, got {:?}", other),
    }

    // Containers section should still be present (with empty containers list).
    assert!(
        snapshot.containers.is_some(),
        "Containers section should be present even when degraded"
    );
}

#[test]
fn containers_all_dirs_missing_complete() {
    // No quadlet, compose, podman, or flatpak directories → Complete.
    // This is a valid state: system with no containers configured.
    let exec = minimal_rpm_mock(MockExecutor::new())
        .with_command(
            "which podman",
            ExecResult {
                stdout: "/usr/bin/podman\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "podman ps --format json",
            ExecResult {
                stdout: "[]".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "podman ps --all --format json",
            ExecResult {
                stdout: "[]".into(),
                exit_code: 0,
                ..Default::default()
            },
        );

    let source = package_based_source();
    let inspectors: Vec<Box<dyn inspectah_core::traits::inspector::Inspector>> =
        vec![Box::new(ContainersInspector::new())];

    let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress);
    let snapshot = &pipeline.state.snapshot;

    assert!(
        matches!(snapshot.completeness, Completeness::Complete),
        "Containers inspector should be Complete when dirs are simply absent"
    );

    assert!(
        snapshot.containers.is_some(),
        "Containers section should be present with empty data"
    );
}

// ---------------------------------------------------------------------------
// UsersGroups inspector failure tests
// ---------------------------------------------------------------------------

#[test]
fn users_passwd_failure_incomplete() {
    // /etc/passwd read failure → Fatal error, Incomplete.
    let exec = minimal_rpm_mock(MockExecutor::new())
        .with_file_error("/etc/passwd", std::io::ErrorKind::PermissionDenied);

    let source = package_based_source();
    let inspectors: Vec<Box<dyn inspectah_core::traits::inspector::Inspector>> =
        vec![Box::new(UsersGroupsInspector::new())];

    let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress);
    let snapshot = &pipeline.state.snapshot;

    match &snapshot.completeness {
        Completeness::Incomplete {
            failed_sections, ..
        } => {
            assert!(
                failed_sections.contains(&InspectorId::UsersGroups),
                "UsersGroups inspector should fail when passwd is unreadable"
            );
        }
        other => panic!("Expected Completeness::Incomplete, got {:?}", other),
    }

    // No users section should be present (fatal failure).
    assert!(
        snapshot.users_groups.is_none(),
        "UsersGroups section should be absent when inspector fails"
    );
}

#[test]
fn users_shadow_permission_denied_degraded() {
    // /etc/shadow unreadable (common for non-root) → Degraded.
    let exec = minimal_rpm_mock(MockExecutor::new())
        .with_file(
            "/etc/passwd",
            "root:x:0:0:root:/root:/bin/bash\n\
             testuser:x:1001:1001:Test User:/home/testuser:/bin/bash\n",
        )
        .with_file_error("/etc/shadow", std::io::ErrorKind::PermissionDenied)
        .with_file(
            "/etc/group",
            "wheel:x:10:\n\
             testuser:x:1001:\n",
        );

    let source = package_based_source();
    let inspectors: Vec<Box<dyn inspectah_core::traits::inspector::Inspector>> =
        vec![Box::new(UsersGroupsInspector::new())];

    let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress);
    let snapshot = &pipeline.state.snapshot;

    match &snapshot.completeness {
        Completeness::Partial {
            degraded_sections, ..
        } => {
            assert!(
                degraded_sections.contains(&InspectorId::UsersGroups),
                "UsersGroups inspector should be degraded when shadow is unreadable"
            );
        }
        other => panic!("Expected Completeness::Partial, got {:?}", other),
    }

    // Users section should be present with partial data (no shadow info).
    assert!(
        snapshot.users_groups.is_some(),
        "UsersGroups section should be present even when degraded"
    );
}

// ---------------------------------------------------------------------------
// Mixed failure scenarios
// ---------------------------------------------------------------------------

#[test]
fn mixed_failures_across_inspectors() {
    // Network degraded + UsersGroups failed → Incomplete.
    // When any inspector fails, overall completeness is Incomplete.
    let exec = minimal_rpm_mock(MockExecutor::new())
        // Network: NM dir PermissionDenied → Degraded
        .with_dir_error(
            "/etc/NetworkManager/system-connections",
            std::io::ErrorKind::PermissionDenied,
        )
        .with_command(
            "ip route",
            ExecResult {
                stdout: String::new(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "ip rule",
            ExecResult {
                stdout: String::new(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // UsersGroups: passwd unreadable → Failed
        .with_file_error("/etc/passwd", std::io::ErrorKind::PermissionDenied);

    let source = package_based_source();
    let inspectors: Vec<Box<dyn inspectah_core::traits::inspector::Inspector>> = vec![
        Box::new(NetworkInspector::new()),
        Box::new(UsersGroupsInspector::new()),
    ];

    let pipeline = collect(&source, &exec, &inspectors, None, &NullProgress);
    let snapshot = &pipeline.state.snapshot;

    match &snapshot.completeness {
        Completeness::Incomplete {
            failed_sections,
            degraded_sections,
            ..
        } => {
            let failed_set: HashSet<_> = failed_sections.iter().collect();
            let degraded_set: HashSet<_> = degraded_sections.iter().collect();

            assert!(
                failed_set.contains(&InspectorId::UsersGroups),
                "UsersGroups should be in failed_sections"
            );
            assert!(
                degraded_set.contains(&InspectorId::Network),
                "Network should be in degraded_sections"
            );
        }
        other => panic!(
            "Expected Completeness::Incomplete with both failed and degraded, got {:?}",
            other
        ),
    }

    // Network section present (degraded), UsersGroups absent (failed).
    assert!(
        snapshot.network.is_some(),
        "Network section should be present (degraded)"
    );
    assert!(
        snapshot.users_groups.is_none(),
        "UsersGroups section should be absent (failed)"
    );
}
