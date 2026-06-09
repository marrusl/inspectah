//! Integration tests for the Storage inspector.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::storage::StorageInspector;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::completeness::{SectionData, SourceSystemKind};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;

/// Standard fixture: /etc/fstab
const FSTAB_FIXTURE: &str = include_str!("../../../testdata/fixtures/storage/fstab");

/// Standard fixture: findmnt --json output
const FINDMNT_FIXTURE: &str = include_str!("../../../testdata/fixtures/storage/findmnt.json");

/// Standard fixture: lvs --reportformat json output
const LVS_FIXTURE: &str = include_str!("../../../testdata/fixtures/storage/lvs.json");

fn pkg_source() -> SourceSystem {
    SourceSystem::PackageBased {
        os_release: OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        },
    }
}

/// Build a MockExecutor with the full fixture set: fstab, findmnt, lvs.
fn full_mock() -> MockExecutor {
    MockExecutor::new()
        .with_file("/etc/fstab", FSTAB_FIXTURE)
        .with_command(
            "findmnt --json",
            ExecResult {
                stdout: FINDMNT_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "lvs --reportformat json",
            ExecResult {
                stdout: LVS_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
}

// ── Test 1: Happy path ───────────────────────────────────────────────

#[test]
fn happy_path() {
    let exec = full_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = StorageInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Storage(s) => s,
        other => panic!("expected SectionData::Storage, got {:?}", other),
    };

    // fstab entries parsed (7 non-comment, non-empty lines)
    assert_eq!(section.fstab_entries.len(), 7);

    // Check root entry
    let root = section
        .fstab_entries
        .iter()
        .find(|e| e.mount_point == "/")
        .expect("root mount should exist");
    assert_eq!(root.device, "UUID=abc-123");
    assert_eq!(root.fstype, "xfs");

    // NFS entry present
    let nfs = section
        .fstab_entries
        .iter()
        .find(|e| e.fstype == "nfs")
        .expect("NFS entry should exist");
    assert_eq!(nfs.mount_point, "/mnt/nfs");
    assert_eq!(nfs.device, "server:/export");

    // Mount points from findmnt
    assert_eq!(section.mount_points.len(), 6);
    assert!(section.mount_points.iter().any(|m| m.target == "/"));
    assert!(section.mount_points.iter().any(|m| m.target == "/mnt/nfs"));

    // LVM volumes
    assert_eq!(section.lvm_info.len(), 2);
    let data_lv = section
        .lvm_info
        .iter()
        .find(|v| v.lv_name == "data")
        .expect("data LV should exist");
    assert_eq!(data_lv.vg_name, "vg0");
    assert_eq!(data_lv.lv_size, "50.00g");
}

// ── Test 2: Credential detection ─────────────────────────────────────

#[test]
fn credential_detection() {
    let exec = full_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = StorageInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Storage(s) => s,
        other => panic!("expected SectionData::Storage, got {:?}", other),
    };

    // CIFS entry has credentials=/etc/cifs-creds
    assert_eq!(section.credential_refs.len(), 1);
    let cred = &section.credential_refs[0];
    assert_eq!(cred.mount_point, "/mnt/cifs");
    assert_eq!(cred.credential_path, "/etc/cifs-creds");
    assert_eq!(cred.source, "fstab");

    // Should also produce a RedactionHint
    assert!(
        !output.redaction_hints.is_empty(),
        "credential reference should produce a redaction hint"
    );
    let hint = &output.redaction_hints[0];
    assert!(hint.path.contains("/etc/fstab"));
    assert!(hint.reason.contains("credential"));
}

// ── Test 3: findmnt failure returns degraded ──────────────────────────

#[test]
fn findmnt_failure_returns_degraded() {
    let exec = MockExecutor::new()
        .with_file("/etc/fstab", FSTAB_FIXTURE)
        .with_command(
            "findmnt --json",
            ExecResult {
                stderr: "findmnt: command failed".into(),
                exit_code: 1,
                ..Default::default()
            },
        );
    // No lvs command — should proceed without

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = StorageInspector::new().inspect(&ctx, &NullProgress);
    match result {
        Err(InspectorError::Degraded { reason, partial }) => {
            assert!(
                reason.contains("findmnt"),
                "reason should mention findmnt, got: {reason}"
            );
            // Partial should have fstab data
            if let SectionData::Storage(s) = &partial.section {
                assert!(
                    !s.fstab_entries.is_empty(),
                    "partial should contain fstab entries"
                );
                assert!(s.mount_points.is_empty(), "mount_points should be empty");
            } else {
                panic!("partial should be Storage section");
            }
        }
        other => panic!("expected Degraded, got {:?}", other),
    }
}

// ── Test 4: Malformed findmnt JSON returns degraded ───────────────────

#[test]
fn malformed_findmnt_json() {
    let exec = MockExecutor::new()
        .with_file("/etc/fstab", FSTAB_FIXTURE)
        .with_command(
            "findmnt --json",
            ExecResult {
                stdout: "{ not valid json".into(),
                exit_code: 0,
                ..Default::default()
            },
        );

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = StorageInspector::new().inspect(&ctx, &NullProgress);
    match result {
        Err(InspectorError::Degraded { reason, partial }) => {
            assert!(
                reason.contains("parse") || reason.contains("JSON") || reason.contains("json"),
                "reason should mention parse error, got: {reason}"
            );
            if let SectionData::Storage(s) = &partial.section {
                assert!(
                    !s.fstab_entries.is_empty(),
                    "partial should contain fstab entries"
                );
            } else {
                panic!("partial should be Storage section");
            }
        }
        other => panic!("expected Degraded, got {:?}", other),
    }
}

// ── Test 5: fstab unreadable returns Failed ──────────────────────────

#[test]
fn fstab_unreadable_returns_failed() {
    // No file registered → read_file returns Err
    let exec = MockExecutor::new();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = StorageInspector::new().inspect(&ctx, &NullProgress);
    match result {
        Err(InspectorError::Failed { reason }) => {
            assert!(
                reason.contains("fstab"),
                "reason should mention fstab, got: {reason}"
            );
        }
        other => panic!("expected Failed, got {:?}", other),
    }
}

// ── Test 6: LVM not available proceeds without ───────────────────────

#[test]
fn lvm_not_available() {
    let exec = MockExecutor::new()
        .with_file("/etc/fstab", FSTAB_FIXTURE)
        .with_command(
            "findmnt --json",
            ExecResult {
                stdout: FINDMNT_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        );
    // No lvs command registered → MockExecutor returns exit 127

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = StorageInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Storage(s) => s,
        other => panic!("expected SectionData::Storage, got {:?}", other),
    };

    // fstab and findmnt should still work
    assert!(!section.fstab_entries.is_empty());
    assert!(!section.mount_points.is_empty());
    // LVM should be empty (not an error)
    assert!(section.lvm_info.is_empty());
}

// ── Test 7: Empty fstab ──────────────────────────────────────────────

#[test]
fn empty_fstab() {
    let exec = MockExecutor::new()
        .with_file("/etc/fstab", "")
        .with_command(
            "findmnt --json",
            ExecResult {
                stdout: r#"{"filesystems": []}"#.into(),
                exit_code: 0,
                ..Default::default()
            },
        );

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = StorageInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Storage(s) => s,
        other => panic!("expected SectionData::Storage, got {:?}", other),
    };

    assert!(section.fstab_entries.is_empty());
    assert!(section.mount_points.is_empty());
    assert!(section.lvm_info.is_empty());
    assert!(section.credential_refs.is_empty());
}

// ── Test 8: Applicability ─────────────────────────────────────────────

#[test]
fn applicability() {
    let inspector = StorageInspector::new();
    assert_eq!(inspector.applicable_to(), &[SourceSystemKind::PackageBased]);
}

// ── Test 9: Insta snapshot ────────────────────────────────────────────

#[test]
fn storage_snapshot() {
    let exec = full_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = StorageInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Storage(s) => s,
        other => panic!("expected SectionData::Storage, got {:?}", other),
    };

    insta::assert_json_snapshot!(section);
}
