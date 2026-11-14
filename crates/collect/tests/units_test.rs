//! Integration tests for the Services inspector.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::units::ServicesInspector;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::completeness::{SectionData, SourceSystemKind};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_core::types::units::{
    CarriedUnit, PresetDefault, UnitAction, UnitFragment, UnitSection, UnitState,
};

/// Standard fixture: `systemctl list-unit-files` output.
const SYSTEMCTL_FIXTURE: &str =
    include_str!("../../../testdata/fixtures/services/systemctl-list-unit-files.txt");

/// Standard fixture: preset file.
const PRESET_FIXTURE: &str =
    include_str!("../../../testdata/fixtures/services/preset-90-default.preset");

/// Standard fixture: drop-in override.
const DROPIN_FIXTURE: &str =
    include_str!("../../../testdata/fixtures/services/dropin-httpd-override.conf");

/// A minimal unit file body.
const UNIT_FILE: &str = "[Unit]\nDescription=test unit\n";

/// Register a unit file under the vendor directory for each name, as a
/// package installs it.
fn with_vendor_units(mut exec: MockExecutor, units: &[&str]) -> MockExecutor {
    for unit in units {
        exec = exec.with_file(&format!("/usr/lib/systemd/system/{unit}"), UNIT_FILE);
    }
    exec
}

/// The names `systemctl list-unit-files` output lists.
fn listed_units(listing: &str) -> Vec<&str> {
    listing
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| name.contains(".service"))
        .collect()
}

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

/// Build a MockExecutor with the full fixture set: systemctl output, a
/// vendor unit file for every listed unit, one preset directory with the
/// default preset, and one drop-in directory.
fn full_mock() -> MockExecutor {
    with_vendor_units(MockExecutor::new(), &listed_units(SYSTEMCTL_FIXTURE))
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
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Units(s) => s,
        other => panic!("expected SectionData::Units, got {:?}", other),
    };

    // bluetooth: enabled on host, preset says "disable" → divergence
    let bluetooth = section
        .state_changes
        .iter()
        .find(|s| s.unit().name().as_str() == "bluetooth.service")
        .expect("bluetooth should have state change");
    assert_eq!(bluetooth.current_state(), UnitState::Enabled);
    assert_eq!(bluetooth.default_state, Some(PresetDefault::Disable));
    assert_eq!(bluetooth.implied_action(), UnitAction::Enable);

    // gdm: disabled on host, preset says "enable" → divergence
    let gdm = section
        .state_changes
        .iter()
        .find(|s| s.unit().name().as_str() == "gdm.service")
        .expect("gdm should have state change");
    assert_eq!(gdm.current_state(), UnitState::Disabled);
    assert_eq!(gdm.default_state, Some(PresetDefault::Enable));
    assert_eq!(gdm.implied_action(), UnitAction::Disable);

    // httpd: enabled on host, no preset entry → NO state change (cannot determine divergence)
    assert!(
        section
            .state_changes
            .iter()
            .all(|s| s.unit().name().as_str() != "httpd.service"),
        "httpd has no preset rule, should not appear in state_changes"
    );

    // libvirtd: enabled on host, no preset entry → NO state change
    assert!(
        section
            .state_changes
            .iter()
            .all(|s| s.unit().name().as_str() != "libvirtd.service"),
        "libvirtd has no preset rule, should not appear in state_changes"
    );

    // auditd: enabled=enabled → NO divergence → NO state change
    assert!(
        section
            .state_changes
            .iter()
            .all(|s| s.unit().name().as_str() != "auditd.service"),
        "auditd state matches preset, should not appear in state_changes"
    );

    // cups: disabled=disabled → NO divergence → NO state change
    // cups has no preset rule either, so definitely no state change
    assert!(
        section
            .state_changes
            .iter()
            .all(|s| s.unit().name().as_str() != "cups.service"),
        "cups should not appear in state_changes"
    );

    // Verify the inventory holds every unit with its state
    assert_eq!(
        inventory_state(section, "sshd.service"),
        Some(UnitState::Enabled)
    );
    assert_eq!(
        inventory_state(section, "auditd.service"),
        Some(UnitState::Enabled)
    );
    assert_eq!(
        inventory_state(section, "cups.service"),
        Some(UnitState::Disabled)
    );
    assert_eq!(
        inventory_state(section, "gdm.service"),
        Some(UnitState::Disabled)
    );
}

/// The state the inventory records for `unit`, if it has an entry.
fn inventory_state(section: &UnitSection, unit: &str) -> Option<UnitState> {
    section
        .inventory
        .iter()
        .find(|e| e.unit.as_str() == unit)
        .map(|e| e.state)
}

/// Names of the inventory entries whose state is the one their preset gives.
fn preset_matched(section: &UnitSection) -> Vec<&str> {
    section
        .inventory
        .iter()
        .filter(|e| e.matches_preset())
        .map(|e| e.unit.as_str())
        .collect()
}

// ── Inventory: one entry per unit, with its state and preset ───────

#[test]
fn inventory_has_one_entry_per_unit_with_state_and_preset() {
    // sshd.service is listed twice: the inventory still holds it once.
    // bad.unit-kind has no unit suffix: it is warned about and skipped.
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         bluetooth.service                          enabled         disabled\n\
                         cups.service                               disabled        disabled\n\
                         sshd.service                               enabled         enabled\n\
                         sshd.service                               enabled         enabled\n\
                         bad.unit-kind                              enabled         enabled\n\
                         \n\
                         5 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file(
            "/usr/lib/systemd/system-preset/90-default.preset",
            "disable bluetooth.service\nenable sshd.service\n",
        )
        .with_dir("/etc/systemd/system", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Units(s) => s,
        other => panic!("expected SectionData::Units, got {:?}", other),
    };

    let entries: Vec<(&str, UnitState, Option<PresetDefault>)> = section
        .inventory
        .iter()
        .map(|e| (e.unit.as_str(), e.state, e.preset))
        .collect();
    assert_eq!(
        entries,
        vec![
            // Enabled against a disable preset: also a state change.
            (
                "bluetooth.service",
                UnitState::Enabled,
                Some(PresetDefault::Disable)
            ),
            // Disabled with no preset rule.
            ("cups.service", UnitState::Disabled, None),
            // Enabled as its preset says.
            (
                "sshd.service",
                UnitState::Enabled,
                Some(PresetDefault::Enable)
            ),
        ],
        "one entry per unit, in the order systemctl lists them"
    );
    assert_eq!(preset_matched(section), vec!["sshd.service"]);

    let mut names: Vec<&str> = section.inventory.iter().map(|e| e.unit.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(
        names.len(),
        section.inventory.len(),
        "no unit appears twice"
    );

    // The name that does not parse is skipped, and the warning names it.
    assert!(output.warnings.iter().any(|w| {
        w.inspector == "units" && w.extra.get("unit") == Some(&serde_json::json!("bad.unit-kind"))
    }));
}

#[test]
fn inventory_records_masked_units() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         cups.service                               masked          enabled\n\
                         sshd.service                               enabled         enabled\n\
                         \n\
                         2 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file(
            "/usr/lib/systemd/system-preset/90-default.preset",
            "enable cups.service\nenable sshd.service\n",
        )
        .with_dir("/etc/systemd/system", vec![])
        .with_link("/etc/systemd/system/cups.service", "/dev/null");
    let exec = with_vendor_units(exec, &["cups.service", "sshd.service"]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Units(s) => s,
        other => panic!("expected SectionData::Units, got {:?}", other),
    };

    assert_eq!(
        inventory_state(section, "sshd.service"),
        Some(UnitState::Enabled)
    );
    assert_eq!(
        inventory_state(section, "cups.service"),
        Some(UnitState::Masked)
    );
    assert!(
        section
            .state_changes
            .iter()
            .any(|s| s.unit().name().as_str() == "cups.service"),
        "a masked unit is a state change as well as an inventory entry"
    );
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
    let exec = with_vendor_units(exec, &["sshd.service"]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Units(s) => s,
        other => panic!("expected SectionData::Units, got {:?}", other),
    };

    // sshd is enabled but first-match preset says disabled → divergence
    let sshd = section
        .state_changes
        .iter()
        .find(|s| s.unit().name().as_str() == "sshd.service")
        .expect("sshd should diverge from first-match preset");
    assert_eq!(sshd.default_state, Some(PresetDefault::Disable));
    assert_eq!(sshd.current_state(), UnitState::Enabled);
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
    let exec = with_vendor_units(exec, &["custom-app.service"]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Units(s) => s,
        other => panic!("expected SectionData::Units, got {:?}", other),
    };

    // custom-app is disabled, but glob preset "enable *" says enabled → divergence
    let app = section
        .state_changes
        .iter()
        .find(|s| s.unit().name().as_str() == "custom-app.service")
        .expect("custom-app should diverge from glob preset");
    assert_eq!(app.default_state, Some(PresetDefault::Enable));
    assert_eq!(app.current_state(), UnitState::Disabled);
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
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Units(s) => s,
        other => panic!("expected SectionData::Units, got {:?}", other),
    };

    assert_eq!(section.drop_ins.len(), 1);
    let dropin = &section.drop_ins[0];
    assert_eq!(dropin.unit, "httpd.service");
    assert!(dropin.path.contains("override.conf"));
    assert!(dropin.content.contains("LimitNOFILE=65535"));
    assert!(dropin.disposition.is_included());
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
        baseline_data: None,
    };

    let result = ServicesInspector::new().inspect(&ctx, &NullProgress);
    match result {
        Err(InspectorError::Degraded { reason, partial }) => {
            assert!(
                reason.contains("not found"),
                "reason should mention not found, got: {reason}"
            );
            // Partial should have empty section
            if let SectionData::Units(s) = &partial.section {
                assert!(s.state_changes.is_empty());
                assert!(s.inventory.is_empty());
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
    // systemctl works, but no preset directories registered in mock. One
    // unit in each durable state: a missing preset does not undo any of them.
    let exec = MockExecutor::new().with_command(
        "systemctl list-unit-files --type=service --no-pager",
        ExecResult {
            stdout: "UNIT FILE                                  STATE           PRESET\n\
                     sshd.service                               enabled         enabled\n\
                     cups.service                               disabled        enabled\n\
                     debug-shell.service                        masked          disabled\n\
                     \n\
                     3 unit files listed.\n"
                .into(),
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
        baseline_data: None,
    };

    let result = ServicesInspector::new().inspect(&ctx, &NullProgress);
    match result {
        Err(InspectorError::Degraded { reason, partial }) => {
            assert!(
                reason.contains("system-preset"),
                "reason should mention preset dirs, got: {reason}"
            );
            // Partial keeps every unit with a durable state, with no preset.
            let SectionData::Units(s) = &partial.section else {
                panic!("partial should be the units section");
            };
            let inventory: Vec<(&str, UnitState, Option<PresetDefault>)> = s
                .inventory
                .iter()
                .map(|e| (e.unit.as_str(), e.state, e.preset))
                .collect();
            assert_eq!(
                inventory,
                [
                    ("sshd.service", UnitState::Enabled, None),
                    ("cups.service", UnitState::Disabled, None),
                    ("debug-shell.service", UnitState::Masked, None),
                ],
                "the degraded inventory must keep enabled, disabled and masked units"
            );
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
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Units(s) => s,
        other => panic!("expected SectionData::Units, got {:?}", other),
    };

    assert!(section.state_changes.is_empty());
    assert!(section.inventory.is_empty());
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
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();

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
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Units(s) => s,
        other => panic!("expected SectionData::Units, got {:?}", other),
    };

    insta::assert_json_snapshot!(section);
}

// ── Test 8: preset-matched inventory entries ───────────────────────

#[test]
fn preset_matched_entries_capture_matches() {
    let exec = full_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Units(s) => s,
        other => panic!("expected SectionData::Units, got {:?}", other),
    };

    // Units where state matches preset are the preset-matched entries
    let matched = preset_matched(section);

    // auditd: enabled=enabled → match
    assert!(
        matched.contains(&"auditd.service"),
        "auditd.service should be preset-matched (enabled=enabled)"
    );

    // chronyd: enabled=enabled → match
    assert!(
        matched.contains(&"chronyd.service"),
        "chronyd.service should be preset-matched (enabled=enabled)"
    );

    // firewalld: enabled=enabled → match
    assert!(
        matched.contains(&"firewalld.service"),
        "firewalld.service should be preset-matched (enabled=enabled)"
    );

    // kdump: enabled=enabled → match
    assert!(
        matched.contains(&"kdump.service"),
        "kdump.service should be preset-matched (enabled=enabled)"
    );

    // NetworkManager: enabled=enabled → match
    assert!(
        matched.contains(&"NetworkManager.service"),
        "NetworkManager.service should be preset-matched (enabled=enabled)"
    );

    // sshd: enabled=enabled → match
    assert!(
        matched.contains(&"sshd.service"),
        "sshd.service should be preset-matched (enabled=enabled)"
    );

    // tuned: enabled=enabled → match
    assert!(
        matched.contains(&"tuned.service"),
        "tuned.service should be preset-matched (enabled=enabled)"
    );

    // Units with divergence should NOT be preset-matched
    assert!(
        !matched.contains(&"bluetooth.service"),
        "bluetooth.service should NOT be preset-matched (enabled≠disabled)"
    );

    assert!(
        !matched.contains(&"gdm.service"),
        "gdm.service should NOT be preset-matched (disabled≠enabled)"
    );

    // Units with no preset should NOT be preset-matched
    assert!(
        !matched.contains(&"httpd.service"),
        "httpd.service should NOT be preset-matched (no preset rule)"
    );

    assert!(
        !matched.contains(&"libvirtd.service"),
        "libvirtd.service should NOT be preset-matched (no preset rule)"
    );

    // cups has no preset but is disabled - should NOT be preset-matched
    // (we can only determine matches when a preset rule exists)
    assert!(
        !matched.contains(&"cups.service"),
        "cups.service should NOT be preset-matched (no preset rule)"
    );
}

#[test]
fn no_preset_matched_entries_without_preset_rules() {
    // Mock with systemctl output but no preset files (empty directory)
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: SYSTEMCTL_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec![]); // Empty preset dir

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Units(s) => s,
        other => panic!("expected SectionData::Units, got {:?}", other),
    };

    // With no preset rules, we cannot determine matches
    assert!(
        !section.inventory.is_empty(),
        "the inventory holds the units systemctl listed"
    );
    assert!(
        preset_matched(section).is_empty(),
        "no entry is preset-matched when no preset rules exist"
    );

    // And we cannot determine divergences either
    assert!(
        section.state_changes.is_empty(),
        "state_changes should be empty when no preset rules exist"
    );
}

// ── Intent gate: warning states ───────────────────────────────────────

#[test]
fn test_intent_gate_warns_runtime_linked_bad_and_unknown_states() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         transient.service                          transient       disabled\n\
                         linked.service                             linked          enabled\n\
                         broken.service                             bad             disabled\n\
                         future.service                             future-state    disabled\n\
                         \n\
                         4 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file(
            "/usr/lib/systemd/system-preset/90-default.preset",
            "disable *\n",
        )
        .with_dir("/etc/systemd/system", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Units(s) => s,
        other => panic!("expected Services section, got {other:?}"),
    };

    assert!(
        section.state_changes.is_empty(),
        "warning-only states must not persist"
    );
    assert_eq!(
        output.warnings.len(),
        4,
        "each warning state should emit one warning"
    );
    assert!(output.warnings.iter().any(|w| {
        w.inspector == "units"
            && w.extra.get("unit") == Some(&serde_json::json!("linked.service"))
            && w.extra.get("raw_state") == Some(&serde_json::json!("linked"))
    }));
}

#[test]
fn test_clean_default_snapshot_produces_zero_state_changes() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         dbus.service                               alias           disabled\n\
                         sssd-kcm.service                           indirect        disabled\n\
                         systemd-sysupdate.service                  indirect        disabled\n\
                         chronyd.service                            enabled         enabled\n\
                         sshd.service                               enabled         enabled\n\
                         cups.service                               disabled        disabled\n\
                         \n\
                         6 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file(
            "/usr/lib/systemd/system-preset/90-default.preset",
            "enable chronyd.service\nenable sshd.service\ndisable *\n",
        )
        .with_dir("/etc/systemd/system", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Units(s) => s,
        other => panic!("expected Services section, got {other:?}"),
    };

    assert!(
        section.state_changes.is_empty(),
        "clean default host should be a no-op"
    );
    assert!(
        output.warnings.is_empty(),
        "known inert states should drop silently"
    );
}

// ── Owning-package resolution tests ──────────────────────────────────

#[test]
fn test_owning_package_batch_query_populates_state_changes() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         httpd.service                              enabled         disabled\n\
                         firewalld.service                          disabled        enabled\n\
                         \n\
                         2 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file(
            "/usr/lib/systemd/system-preset/90-default.preset",
            "disable httpd.service\nenable firewalld.service\n",
        )
        .with_command(
            "rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/httpd.service /usr/lib/systemd/system/firewalld.service",
            ExecResult {
                stdout: "httpd\nfirewalld\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/etc/systemd/system", vec![]);
    let exec = with_vendor_units(exec, &["httpd.service", "firewalld.service"]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Units(s) => s,
        other => panic!("expected Services section, got {other:?}"),
    };

    assert_eq!(
        section.state_changes[0].unit().owning_package(),
        Some("httpd")
    );
    assert_eq!(
        section.state_changes[1].unit().owning_package(),
        Some("firewalld")
    );
}

#[test]
fn test_owning_package_batch_failure_falls_back_to_individual_queries() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         httpd.service                              enabled         disabled\n\
                         firewalld.service                          disabled        enabled\n\
                         \n\
                         2 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file(
            "/usr/lib/systemd/system-preset/90-default.preset",
            "disable httpd.service\nenable firewalld.service\n",
        )
        .with_command(
            "rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/httpd.service /usr/lib/systemd/system/firewalld.service",
            ExecResult {
                stdout: "error: batch ownership lookup failed\n".into(),
                exit_code: 1,
                ..Default::default()
            },
        )
        .with_command(
            "rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/httpd.service",
            ExecResult {
                stdout: "httpd\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/firewalld.service",
            ExecResult {
                stdout: "firewalld\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/etc/systemd/system", vec![]);
    let exec = with_vendor_units(exec, &["httpd.service", "firewalld.service"]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::Units(s) => s,
        other => panic!("expected Services section, got {other:?}"),
    };

    assert_eq!(
        section.state_changes[0].unit().owning_package(),
        Some("httpd")
    );
    assert_eq!(
        section.state_changes[1].unit().owning_package(),
        Some("firewalld")
    );
}

/// The section a scan of `exec` produces, with its warnings.
fn scan(exec: &MockExecutor) -> (UnitSection, Vec<String>) {
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: exec,
        rpm_state: None,
        baseline_data: None,
    };
    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("the scan succeeds");
    let SectionData::Units(section) = output.section else {
        panic!("expected the units section");
    };
    let warnings = output.warnings.into_iter().map(|w| w.message).collect();
    (section, warnings)
}

/// A mock listing `units` (name, state) against a preset that disables
/// every unit, with an empty `/etc/systemd/system` listing.
fn listing_mock(units: &[(&str, &str)]) -> MockExecutor {
    let mut listing =
        String::from("UNIT FILE                                  STATE           PRESET\n");
    for (unit, state) in units {
        listing.push_str(&format!("{unit} {state} disabled\n"));
    }
    listing.push_str(&format!("\n{} unit files listed.\n", units.len()));
    MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: listing,
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file(
            "/usr/lib/systemd/system-preset/90-default.preset",
            "disable *\n",
        )
        .with_dir("/etc/systemd/system", vec![])
}

/// The fragment of the state change for `unit`.
fn fragment_of<'a>(section: &'a UnitSection, unit: &str) -> &'a UnitFragment {
    let change = section
        .state_changes
        .iter()
        .find(|s| s.unit().name().as_str() == unit)
        .unwrap_or_else(|| panic!("{unit} should be a state change"));
    match change.unit() {
        CarriedUnit::Ordinary(ordinary) => ordinary.fragment(),
        other => panic!("{unit} is not an ordinary unit: {other:?}"),
    }
}

fn file(name: &str) -> inspectah_core::types::units::UnitName {
    name.to_owned().try_into().unwrap()
}

/// T5, collector half: a unit only under `/etc` is `Local` and no owner is
/// asked for it; a vendor unit whose owner lookup fails is `Vendor` with no
/// owner. The two fragments differ, which is what render tells apart.
#[test]
fn local_unit_and_unowned_vendor_unit_get_different_fragments() {
    let exec = listing_mock(&[
        ("custom-local.service", "enabled"),
        ("unowned.service", "enabled"),
    ])
    .with_file("/etc/systemd/system/custom-local.service", UNIT_FILE)
    .with_file("/usr/lib/systemd/system/unowned.service", UNIT_FILE)
    .with_command(
        "rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/unowned.service",
        ExecResult {
            stdout: "file /usr/lib/systemd/system/unowned.service is not owned by any package\n"
                .into(),
            exit_code: 1,
            ..Default::default()
        },
    );
    let (section, _) = scan(&exec);

    assert_eq!(
        fragment_of(&section, "custom-local.service"),
        &UnitFragment::Local {
            file: file("custom-local.service")
        }
    );
    assert_eq!(
        fragment_of(&section, "unowned.service"),
        &UnitFragment::Vendor {
            file: file("unowned.service"),
            owning_package: None,
        }
    );
    // Only the vendor file is looked up; the local one has no owner to find.
    let rpm_queries: Vec<String> = exec
        .command_log()
        .into_iter()
        .filter(|c| c.starts_with("rpm -qf"))
        .collect();
    assert!(
        rpm_queries.iter().all(|c| !c.contains("custom-local")),
        "no owner query for a local unit: {rpm_queries:?}"
    );
    assert!(
        rpm_queries.iter().any(|c| c.contains("unowned.service")),
        "the vendor unit's owner is asked for: {rpm_queries:?}"
    );
}

/// A mask over a vendor unit, as a link to `/dev/null`, a zero-length file,
/// or a link to a link to `/dev/null`, is a masked state change whose
/// fragment is the vendor file under the mask. A file of comments only is
/// not a mask: it is a unit file that replaces the vendor one.
#[test]
fn masks_and_a_comment_only_file_resolve_by_form() {
    let exec = listing_mock(&[
        ("nulled.service", "masked"),
        ("empty.service", "masked"),
        ("hop.service", "masked"),
        ("comment.service", "enabled"),
    ])
    .with_link("/etc/systemd/system/nulled.service", "/dev/null")
    .with_file("/etc/systemd/system/empty.service", "")
    .with_link("/etc/systemd/system/hop.service", "/etc/alternatives/hop")
    .with_link("/etc/alternatives/hop", "/dev/null")
    .with_file("/etc/systemd/system/comment.service", "# replaced\n\n");
    let exec = with_vendor_units(
        exec,
        &[
            "nulled.service",
            "empty.service",
            "hop.service",
            "comment.service",
        ],
    )
    .with_command(
        "rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/nulled.service \
         /usr/lib/systemd/system/empty.service /usr/lib/systemd/system/hop.service \
         /usr/lib/systemd/system/comment.service",
        ExecResult {
            stdout: "a\nb\nc\nd\n".into(),
            exit_code: 0,
            ..Default::default()
        },
    );
    let (section, warnings) = scan(&exec);
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");

    for (unit, owner) in [
        ("nulled.service", "a"),
        ("empty.service", "b"),
        ("hop.service", "c"),
    ] {
        assert_eq!(
            fragment_of(&section, unit),
            &UnitFragment::Vendor {
                file: file(unit),
                owning_package: Some(owner.into()),
            },
            "{unit}"
        );
    }
    assert_eq!(
        fragment_of(&section, "comment.service"),
        &UnitFragment::Shadowed {
            file: file("comment.service"),
            owning_package: Some("d".into()),
        }
    );
}

/// A link to a file outside both unit directories is `linked`: no state
/// change, and today's warning. The same unit as a vendor file is a state
/// change.
#[test]
fn link_outside_the_unit_directories_is_excluded_with_a_warning() {
    // Positive control.
    let exec = with_vendor_units(
        listing_mock(&[("app.service", "enabled")]),
        &["app.service"],
    );
    let (section, _) = scan(&exec);
    assert_eq!(section.state_changes.len(), 1);

    let exec = listing_mock(&[("app.service", "enabled")])
        .with_file("/opt/app/app.service", UNIT_FILE)
        .with_link("/etc/systemd/system/app.service", "/opt/app/app.service");
    let (section, warnings) = scan(&exec);
    assert!(
        section.state_changes.is_empty(),
        "{:?}",
        section.state_changes
    );
    assert!(
        warnings
            .iter()
            .any(|w| w
                == "unit app.service has state 'linked' - linked unit requires manual handling"),
        "today's linked warning: {warnings:?}"
    );
}

/// A unit with no file in either directory is reported and has no state
/// change; the same unit with a vendor file is a state change.
#[test]
fn unit_with_no_file_is_reported_not_dropped() {
    let exec = with_vendor_units(
        listing_mock(&[("app.service", "enabled")]),
        &["app.service"],
    );
    assert_eq!(scan(&exec).0.state_changes.len(), 1);

    let (section, warnings) = scan(&listing_mock(&[("app.service", "enabled")]));
    assert!(section.state_changes.is_empty());
    assert!(
        warnings.iter().any(|w| w.contains("app.service")
            && w.contains("no unit file under /etc/systemd/system or /usr/lib/systemd/system")),
        "{warnings:?}"
    );
}

/// systemd loads a unit file whose bytes are not UTF-8 (a Latin-1 comment
/// or `Description=`), and the collector refuses a file over its size cap,
/// both as `InvalidData`. Neither says the file is empty, so neither is a
/// mask and neither loses the unit: an `/etc` unit is `Local`, a vendor
/// unit is `Vendor`, and a mask over such a vendor file hides that file.
/// The scan is not degraded (`scan` expects success).
#[test]
fn unit_file_that_is_not_text_keeps_its_state_change() {
    let exec = listing_mock(&[
        ("etc-latin1.service", "enabled"),
        ("vendor-latin1.service", "enabled"),
        ("masked-latin1.service", "masked"),
    ])
    .with_file_error(
        "/etc/systemd/system/etc-latin1.service",
        std::io::ErrorKind::InvalidData,
    )
    .with_file_error(
        "/usr/lib/systemd/system/vendor-latin1.service",
        std::io::ErrorKind::InvalidData,
    )
    .with_link("/etc/systemd/system/masked-latin1.service", "/dev/null")
    .with_file_error(
        "/usr/lib/systemd/system/masked-latin1.service",
        std::io::ErrorKind::InvalidData,
    )
    .with_command(
        "rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/vendor-latin1.service \
         /usr/lib/systemd/system/masked-latin1.service",
        ExecResult {
            stdout: "a\nb\n".into(),
            exit_code: 0,
            ..Default::default()
        },
    );
    let (section, warnings) = scan(&exec);

    assert_eq!(
        fragment_of(&section, "etc-latin1.service"),
        &UnitFragment::Local {
            file: file("etc-latin1.service")
        }
    );
    assert_eq!(
        fragment_of(&section, "vendor-latin1.service"),
        &UnitFragment::Vendor {
            file: file("vendor-latin1.service"),
            owning_package: Some("a".into()),
        }
    );
    assert_eq!(
        fragment_of(&section, "masked-latin1.service"),
        &UnitFragment::Vendor {
            file: file("masked-latin1.service"),
            owning_package: Some("b".into()),
        }
    );
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
}

/// A unit file that cannot be read for any other reason hides where the
/// unit came from, so the scan is degraded and names the file. The same
/// unit with a readable file is a state change and the scan succeeds.
#[test]
fn unreadable_unit_file_degrades_the_inspector() {
    // Positive control: the readable twin.
    let readable = listing_mock(&[("app.service", "enabled")])
        .with_file("/etc/systemd/system/app.service", UNIT_FILE);
    let (section, _) = scan(&readable);
    assert_eq!(
        fragment_of(&section, "app.service"),
        &UnitFragment::Local {
            file: file("app.service")
        }
    );

    let unreadable = listing_mock(&[("app.service", "enabled")]).with_file_error(
        "/etc/systemd/system/app.service",
        std::io::ErrorKind::PermissionDenied,
    );
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &unreadable,
        rpm_state: None,
        baseline_data: None,
    };
    match ServicesInspector::new().inspect(&ctx, &NullProgress) {
        Err(InspectorError::Degraded { reason, partial }) => {
            assert!(
                reason.starts_with("unit file read failures: ")
                    && reason.contains("/etc/systemd/system/app.service"),
                "the reason names the unreadable unit file: {reason}"
            );
            let SectionData::Units(section) = partial.section else {
                panic!("expected the units section");
            };
            assert!(
                section.state_changes.is_empty(),
                "{:?}",
                section.state_changes
            );
        }
        other => panic!("expected Degraded for an unreadable unit file, got {other:?}"),
    }
}
