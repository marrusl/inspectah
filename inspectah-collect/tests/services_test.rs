//! Integration tests for the Services inspector.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::services::ServicesInspector;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError};
use inspectah_core::types::completeness::{SectionData, SourceSystemKind};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;

/// Standard fixture: `systemctl list-unit-files` output.
const SYSTEMCTL_FIXTURE: &str =
    include_str!("../../testdata/fixtures/services/systemctl-list-unit-files.txt");

/// Standard fixture: preset file.
const PRESET_FIXTURE: &str =
    include_str!("../../testdata/fixtures/services/preset-90-default.preset");

/// Standard fixture: drop-in override.
const DROPIN_FIXTURE: &str =
    include_str!("../../testdata/fixtures/services/dropin-httpd-override.conf");

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

/// Build a MockExecutor with the full fixture set: systemctl output,
/// one preset directory with the default preset, and one drop-in directory.
fn full_mock() -> MockExecutor {
    MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: SYSTEMCTL_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // Preset directory: /usr/lib/systemd/system-preset with one file
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file(
            "/usr/lib/systemd/system-preset/90-default.preset",
            PRESET_FIXTURE,
        )
        // Drop-in directory
        .with_dir("/etc/systemd/system", vec!["httpd.service.d"])
        .with_dir("/etc/systemd/system/httpd.service.d", vec!["override.conf"])
        .with_file(
            "/etc/systemd/system/httpd.service.d/override.conf",
            DROPIN_FIXTURE,
        )
}

// ── Test 1: Applicability ──────────────────────────────────────────

#[test]
fn applicability_package_mode_only() {
    let inspector = ServicesInspector::new();
    assert_eq!(inspector.applicable_to(), &[SourceSystemKind::PackageBased]);
}

// ── Test 2: Happy path state changes ───────────────────────────────

#[test]
fn happy_path_state_changes() {
    let exec = full_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
    };

    let output = ServicesInspector::new().inspect(&ctx).unwrap();
    let section = match &output.section {
        SectionData::Services(s) => s,
        other => panic!("expected SectionData::Services, got {:?}", other),
    };

    // bluetooth: enabled on host, preset says "disable" → divergence
    let bluetooth = section
        .state_changes
        .iter()
        .find(|s| s.unit == "bluetooth.service")
        .expect("bluetooth should have state change");
    assert_eq!(bluetooth.current_state, "enabled");
    assert_eq!(bluetooth.default_state, "disabled");
    assert_eq!(bluetooth.action, "enable");

    // gdm: disabled on host, preset says "enable" → divergence
    let gdm = section
        .state_changes
        .iter()
        .find(|s| s.unit == "gdm.service")
        .expect("gdm should have state change");
    assert_eq!(gdm.current_state, "disabled");
    assert_eq!(gdm.default_state, "enabled");
    assert_eq!(gdm.action, "disable");

    // httpd: enabled on host, no preset entry → NO state change (cannot determine divergence)
    assert!(
        section
            .state_changes
            .iter()
            .all(|s| s.unit != "httpd.service"),
        "httpd has no preset rule, should not appear in state_changes"
    );

    // libvirtd: enabled on host, no preset entry → NO state change
    assert!(
        section
            .state_changes
            .iter()
            .all(|s| s.unit != "libvirtd.service"),
        "libvirtd has no preset rule, should not appear in state_changes"
    );

    // auditd: enabled=enabled → NO divergence → NO state change
    assert!(
        section
            .state_changes
            .iter()
            .all(|s| s.unit != "auditd.service"),
        "auditd state matches preset, should not appear in state_changes"
    );

    // cups: disabled=disabled → NO divergence → NO state change
    // cups has no preset rule either, so definitely no state change
    assert!(
        section
            .state_changes
            .iter()
            .all(|s| s.unit != "cups.service"),
        "cups should not appear in state_changes"
    );

    // Verify enabled/disabled lists are populated
    assert!(section.enabled_units.contains(&"sshd.service".to_string()));
    assert!(section
        .enabled_units
        .contains(&"auditd.service".to_string()));
    assert!(section.disabled_units.contains(&"cups.service".to_string()));
    assert!(section.disabled_units.contains(&"gdm.service".to_string()));
}

// ── Test 3: Preset first-match-wins ────────────────────────────────

#[test]
fn preset_first_match_wins() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         sshd.service                               enabled         enabled\n\
                         \n\
                         1 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // Two preset files: 85 says disable, 90 says enable
        // Sorted by filename: 85 comes first → sshd default = "disabled"
        .with_dir(
            "/usr/lib/systemd/system-preset",
            vec!["85-custom.preset", "90-default.preset"],
        )
        .with_file(
            "/usr/lib/systemd/system-preset/85-custom.preset",
            "disable sshd.service\n",
        )
        .with_file(
            "/usr/lib/systemd/system-preset/90-default.preset",
            "enable sshd.service\n",
        );

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
    };

    let output = ServicesInspector::new().inspect(&ctx).unwrap();
    let section = match &output.section {
        SectionData::Services(s) => s,
        other => panic!("expected SectionData::Services, got {:?}", other),
    };

    // sshd is enabled but first-match preset says disabled → divergence
    let sshd = section
        .state_changes
        .iter()
        .find(|s| s.unit == "sshd.service")
        .expect("sshd should diverge from first-match preset");
    assert_eq!(sshd.default_state, "disabled");
    assert_eq!(sshd.current_state, "enabled");
}

// ── Test 4: Preset glob matching ───────────────────────────────────

#[test]
fn preset_glob_matching() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         custom-app.service                         disabled        disabled\n\
                         \n\
                         1 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["99-catchall.preset"])
        .with_file(
            "/usr/lib/systemd/system-preset/99-catchall.preset",
            "enable *\n",
        );

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
    };

    let output = ServicesInspector::new().inspect(&ctx).unwrap();
    let section = match &output.section {
        SectionData::Services(s) => s,
        other => panic!("expected SectionData::Services, got {:?}", other),
    };

    // custom-app is disabled, but glob preset "enable *" says enabled → divergence
    let app = section
        .state_changes
        .iter()
        .find(|s| s.unit == "custom-app.service")
        .expect("custom-app should diverge from glob preset");
    assert_eq!(app.default_state, "enabled");
    assert_eq!(app.current_state, "disabled");
}

// ── Test 5: Drop-in files collected ────────────────────────────────

#[test]
fn dropin_files_collected() {
    let exec = full_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
    };

    let output = ServicesInspector::new().inspect(&ctx).unwrap();
    let section = match &output.section {
        SectionData::Services(s) => s,
        other => panic!("expected SectionData::Services, got {:?}", other),
    };

    assert_eq!(section.drop_ins.len(), 1);
    let dropin = &section.drop_ins[0];
    assert_eq!(dropin.unit, "httpd.service");
    assert!(dropin.path.contains("override.conf"));
    assert!(dropin.content.contains("LimitNOFILE=65535"));
    assert!(dropin.include);
}

// ── Test 6: systemctl missing → Degraded ───────────────────────────

#[test]
fn systemctl_missing_returns_degraded() {
    // MockExecutor returns exit 127 for unregistered commands
    let exec = MockExecutor::new();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
    };

    let result = ServicesInspector::new().inspect(&ctx);
    match result {
        Err(InspectorError::Degraded { reason, partial }) => {
            assert!(
                reason.contains("not found"),
                "reason should mention not found, got: {reason}"
            );
            // Partial should have empty section
            if let SectionData::Services(s) = &partial.section {
                assert!(s.state_changes.is_empty());
                assert!(s.enabled_units.is_empty());
            } else {
                panic!("partial should be Services section");
            }
        }
        other => panic!("expected Degraded, got {:?}", other),
    }
}

// ── Test 7: Unreadable preset dirs → Degraded (not Ok) ─────────────

#[test]
fn unreadable_preset_returns_degraded_not_ok() {
    // systemctl works, but no preset directories registered in mock
    let exec = MockExecutor::new().with_command(
        "systemctl list-unit-files --type=service --no-pager",
        ExecResult {
            stdout: SYSTEMCTL_FIXTURE.into(),
            exit_code: 0,
            ..Default::default()
        },
    );
    // No .with_dir for preset paths → read_dir returns Err

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
    };

    let result = ServicesInspector::new().inspect(&ctx);
    match result {
        Err(InspectorError::Degraded { reason, partial }) => {
            assert!(
                reason.contains("system-preset"),
                "reason should mention preset dirs, got: {reason}"
            );
            // Partial should contain the systemctl data (enabled/disabled lists)
            if let SectionData::Services(s) = &partial.section {
                assert!(
                    !s.enabled_units.is_empty(),
                    "partial should contain enabled_units from systemctl"
                );
            } else {
                panic!("partial should be Services section");
            }
        }
        other => panic!("expected Degraded, got {:?}", other),
    }
}

// ── Test 8: Empty system → empty section ───────────────────────────

#[test]
fn empty_system_returns_empty_section() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         \n\
                         0 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file("/usr/lib/systemd/system-preset/90-default.preset", "");

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
    };

    let output = ServicesInspector::new().inspect(&ctx).unwrap();
    let section = match &output.section {
        SectionData::Services(s) => s,
        other => panic!("expected SectionData::Services, got {:?}", other),
    };

    assert!(section.state_changes.is_empty());
    assert!(section.enabled_units.is_empty());
    assert!(section.disabled_units.is_empty());
    assert!(section.drop_ins.is_empty());
}

// ── Test 9: Drop-in with secret → RedactionHint ────────────────────

#[test]
fn dropin_with_secret_produces_redaction_hint() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         myapp.service                              enabled         enabled\n\
                         \n\
                         1 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file(
            "/usr/lib/systemd/system-preset/90-default.preset",
            "enable myapp.service\n",
        )
        .with_dir("/etc/systemd/system", vec!["myapp.service.d"])
        .with_dir("/etc/systemd/system/myapp.service.d", vec!["env.conf"])
        .with_file(
            "/etc/systemd/system/myapp.service.d/env.conf",
            "[Service]\nEnvironment=DB_PASSWORD=secret123\nEnvironment=APP_PORT=8080\n",
        );

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
    };

    let output = ServicesInspector::new().inspect(&ctx).unwrap();

    // Should produce a redaction hint for DB_PASSWORD but not APP_PORT
    assert_eq!(
        output.redaction_hints.len(),
        1,
        "exactly one secret-like env var (DB_PASSWORD)"
    );
    let hint = &output.redaction_hints[0];
    assert!(hint.path.contains("env.conf"));
    assert!(hint.reason.contains("DB_PASSWORD"));
}

// ── Test 10: Snapshot test ─────────────────────────────────────────

#[test]
fn services_snapshot() {
    let exec = full_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
    };

    let output = ServicesInspector::new().inspect(&ctx).unwrap();
    let section = match &output.section {
        SectionData::Services(s) => s,
        other => panic!("expected SectionData::Services, got {:?}", other),
    };

    insta::assert_json_snapshot!(section);
}
