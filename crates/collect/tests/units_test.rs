//! Integration tests for the Services inspector.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::units::UnitsInspector;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError, RpmState};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::completeness::{SectionData, SourceSystemKind};
use inspectah_core::types::config::ConfigFileEntry;
use inspectah_core::types::finding::{AdvisoryType, FindingKind, ShadowType};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_core::types::units::{
    AdminStorageFragment, CarriedUnit, OrdinaryKind, PresetDefault, UnitAction, UnitFragment,
    UnitKind, UnitName, UnitSection, UnitState,
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
            "systemctl list-unit-files --no-pager",
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
    let inspector = UnitsInspector::new();
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

    let output = UnitsInspector::new().inspect(&ctx, &NullProgress).unwrap();
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
            "systemctl list-unit-files --no-pager",
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

    let output = UnitsInspector::new().inspect(&ctx, &NullProgress).unwrap();
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
            "systemctl list-unit-files --no-pager",
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

    let output = UnitsInspector::new().inspect(&ctx, &NullProgress).unwrap();
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
            "systemctl list-unit-files --no-pager",
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

    let output = UnitsInspector::new().inspect(&ctx, &NullProgress).unwrap();
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
            "systemctl list-unit-files --no-pager",
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

    let output = UnitsInspector::new().inspect(&ctx, &NullProgress).unwrap();
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

    let output = UnitsInspector::new().inspect(&ctx, &NullProgress).unwrap();
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

    let result = UnitsInspector::new().inspect(&ctx, &NullProgress);
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
        "systemctl list-unit-files --no-pager",
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

    let result = UnitsInspector::new().inspect(&ctx, &NullProgress);
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
            "systemctl list-unit-files --no-pager",
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

    let output = UnitsInspector::new().inspect(&ctx, &NullProgress).unwrap();
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
            "systemctl list-unit-files --no-pager",
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

    let output = UnitsInspector::new().inspect(&ctx, &NullProgress).unwrap();

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

    let output = UnitsInspector::new().inspect(&ctx, &NullProgress).unwrap();
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

    let output = UnitsInspector::new().inspect(&ctx, &NullProgress).unwrap();
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
            "systemctl list-unit-files --no-pager",
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

    let output = UnitsInspector::new().inspect(&ctx, &NullProgress).unwrap();
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
            "systemctl list-unit-files --no-pager",
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

    let output = UnitsInspector::new().inspect(&ctx, &NullProgress).unwrap();
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
            "systemctl list-unit-files --no-pager",
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
    // An enabled unit's file is resolved, since a local one is a state
    // change whatever its preset; on this host both are the package's.
    let exec = with_vendor_units(exec, &["chronyd.service", "sshd.service"]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = UnitsInspector::new().inspect(&ctx, &NullProgress).unwrap();
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
            "systemctl list-unit-files --no-pager",
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

    let output = UnitsInspector::new().inspect(&ctx, &NullProgress).unwrap();
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
            "systemctl list-unit-files --no-pager",
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

    let output = UnitsInspector::new().inspect(&ctx, &NullProgress).unwrap();
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
    let output = UnitsInspector::new()
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
            "systemctl list-unit-files --no-pager",
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
    match UnitsInspector::new().inspect(&ctx, &NullProgress) {
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

// ── Every unit kind (§ 5, § 7.4 of the units spec) ─────────────────

/// The listing command once every kind is listed: no `--type`.
const LIST_EVERY_KIND: &str = "systemctl list-unit-files --no-pager";

/// A mock listing `units` (name, state) under `preset` as the host's one
/// preset file, with an empty `/etc/systemd/system` listing.
fn kinds_mock(units: &[(&str, &str)], preset: &str) -> MockExecutor {
    let mut listing =
        String::from("UNIT FILE                                  STATE           PRESET\n");
    for (unit, state) in units {
        listing.push_str(&format!("{unit} {state} disabled\n"));
    }
    listing.push_str(&format!("\n{} unit files listed.\n", units.len()));
    let answer = ExecResult {
        stdout: listing,
        exit_code: 0,
        ..Default::default()
    };
    MockExecutor::new()
        .with_command(LIST_EVERY_KIND, answer.clone())
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file("/usr/lib/systemd/system-preset/90-default.preset", preset)
        .with_dir("/etc/systemd/system", vec![])
}

/// The rpm answer for one vendor unit file's owner.
fn owner_answer(exec: MockExecutor, unit: &str, owner: Option<&str>) -> MockExecutor {
    let path = format!("/usr/lib/systemd/system/{unit}");
    let answer = match owner {
        Some(owner) => ExecResult {
            stdout: format!("{owner}\n"),
            exit_code: 0,
            ..Default::default()
        },
        None => ExecResult {
            stdout: format!("file {path} is not owned by any package\n"),
            exit_code: 1,
            ..Default::default()
        },
    };
    exec.with_command(&format!("rpm -qf --queryformat %{{NAME}}\n {path}"), answer)
}

fn state_change_names(section: &UnitSection) -> Vec<&str> {
    section
        .state_changes
        .iter()
        .map(|s| s.unit().name().as_str())
        .collect()
}

fn inventory_names(section: &UnitSection) -> Vec<&str> {
    section.inventory.iter().map(|e| e.unit.as_str()).collect()
}

/// T5, collector: `cockpit.socket` enabled against a `disable` preset is one
/// state change of kind `Socket`, with a `Vendor` fragment and the owner the
/// rpm answer gives.
#[test]
fn enabled_socket_against_disable_preset_is_one_vendor_state_change() {
    let exec = kinds_mock(&[("cockpit.socket", "enabled")], "disable cockpit.socket\n")
        .with_file("/usr/lib/systemd/system/cockpit.socket", UNIT_FILE);
    let exec = owner_answer(exec, "cockpit.socket", Some("cockpit-ws"));
    let (section, warnings) = scan(&exec);

    assert_eq!(
        section.state_changes.len(),
        1,
        "{:?}",
        section.state_changes
    );
    let change = &section.state_changes[0];
    assert_eq!(change.unit().name().as_str(), "cockpit.socket");
    assert_eq!(
        change.unit().name().kind(),
        UnitKind::Ordinary(OrdinaryKind::Socket)
    );
    assert_eq!(change.current_state(), UnitState::Enabled);
    assert_eq!(change.default_state, Some(PresetDefault::Disable));
    assert_eq!(
        fragment_of(&section, "cockpit.socket"),
        &UnitFragment::Vendor {
            file: file("cockpit.socket"),
            owning_package: Some("cockpit-ws".into()),
        }
    );
    assert_eq!(inventory_names(&section), vec!["cockpit.socket"]);
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// T7: `dbus-broker.service` is kept and a `dbus-org.` activation alias is
/// dropped, from the state changes and from the inventory.
#[test]
fn dbus_broker_is_kept_and_a_dbus_org_alias_is_dropped() {
    let alias = "dbus-org.freedesktop.network1.service";
    let exec = kinds_mock(
        &[("dbus-broker.service", "enabled"), (alias, "enabled")],
        "disable *\n",
    );
    let exec = with_vendor_units(exec, &["dbus-broker.service", alias]);
    let (section, _) = scan(&exec);

    // Presence first: the broker is a state change and an inventory entry.
    assert_eq!(state_change_names(&section), vec!["dbus-broker.service"]);
    assert_eq!(inventory_names(&section), vec!["dbus-broker.service"]);
    // The alias, in the same state with its own file, is in neither.
    assert!(!state_change_names(&section).contains(&alias));
    assert!(!inventory_names(&section).contains(&alias));
}

/// T7: a local unit enabled with no matching preset rule is a state change;
/// the same local unit disabled is not, since a copied unit file is not
/// enabled until something enables it.
#[test]
fn local_enabled_unit_without_preset_is_a_state_change_and_disabled_is_not() {
    for kind in ["service", "socket", "path", "target"] {
        let unit = format!("site-app.{kind}");
        let unit_path = format!("/etc/systemd/system/{unit}");

        let enabled = kinds_mock(&[(&unit, "enabled")], "disable other.service\n")
            .with_file(&unit_path, UNIT_FILE);
        let (section, _) = scan(&enabled);
        assert_eq!(state_change_names(&section), vec![unit.as_str()], "{unit}");
        assert_eq!(
            fragment_of(&section, &unit),
            &UnitFragment::Local { file: file(&unit) }
        );
        assert_eq!(section.state_changes[0].default_state, None, "{unit}");

        let disabled = kinds_mock(&[(&unit, "disabled")], "disable other.service\n")
            .with_file(&unit_path, UNIT_FILE);
        let (section, _) = scan(&disabled);
        assert!(
            section.state_changes.is_empty(),
            "{unit}: {:?}",
            section.state_changes
        );
        assert_eq!(inventory_names(&section), vec![unit.as_str()], "{unit}");
    }
}

/// T7: a `generated` unit and each runtime state give no state change, with
/// today's handling (`generated` is dropped silently, a runtime state is
/// warned about). The same unit `enabled` is a state change.
#[test]
fn generated_and_runtime_states_give_no_state_change() {
    let unit = "cockpit.socket";
    let control = kinds_mock(&[(unit, "enabled")], "disable *\n");
    let (section, _) = scan(&with_vendor_units(control, &[unit]));
    assert_eq!(state_change_names(&section), vec![unit]);

    let generated = kinds_mock(&[(unit, "generated")], "disable *\n");
    let (section, warnings) = scan(&with_vendor_units(generated, &[unit]));
    assert!(
        section.state_changes.is_empty(),
        "{:?}",
        section.state_changes
    );
    assert!(section.inventory.is_empty());
    assert!(
        warnings.is_empty(),
        "generated is dropped silently: {warnings:?}"
    );

    for state in [
        "enabled-runtime",
        "masked-runtime",
        "transient",
        "linked-runtime",
    ] {
        let exec = kinds_mock(&[(unit, state)], "disable *\n");
        let (section, warnings) = scan(&with_vendor_units(exec, &[unit]));
        assert!(section.state_changes.is_empty(), "{state}");
        assert!(section.inventory.is_empty(), "{state}");
        assert_eq!(
            warnings,
            vec![format!(
                "unit {unit} has state '{state}' - transient state is not migrated"
            )],
            "{state}"
        );
    }
}

/// A masked template is a `Masked` state change on the template name; the
/// same template unmasked is skipped, from the inventory too.
#[test]
fn masked_template_is_a_state_change_and_an_unmasked_one_is_skipped() {
    let template = "worker@.service";
    let masked = kinds_mock(&[(template, "masked")], "disable *\n")
        .with_link(&format!("/etc/systemd/system/{template}"), "/dev/null");
    let (section, _) = scan(&with_vendor_units(masked, &[template]));
    assert_eq!(state_change_names(&section), vec![template]);
    assert_eq!(section.state_changes[0].current_state(), UnitState::Masked);
    assert_eq!(
        fragment_of(&section, template),
        &UnitFragment::Vendor {
            file: file(template),
            owning_package: None,
        }
    );
    assert_eq!(inventory_names(&section), vec![template]);

    for state in ["enabled", "disabled"] {
        let exec = kinds_mock(&[(template, state)], "enable *\n");
        let (section, warnings) = scan(&with_vendor_units(exec, &[template]));
        assert!(section.state_changes.is_empty(), "{state}");
        assert!(section.inventory.is_empty(), "{state}");
        assert!(warnings.is_empty(), "{state}: {warnings:?}");
    }
}

/// A mount finding's name, fragment and recorded state.
type FindingRow<'a> = (
    &'a str,
    &'a AdminStorageFragment,
    Option<(UnitState, Option<PresetDefault>)>,
);

/// A storage unit's owner is looked up before it is classified: a vendor
/// `tmp.mount` with an owner is a carried `PackagedStorage` state change; a
/// vendor mount with no owner, a local one, and a mask on a mount with no
/// file are mount findings and never state changes.
#[test]
fn storage_unit_routes_by_owner_to_state_change_or_mount_finding() {
    let exec = kinds_mock(
        &[
            ("tmp.mount", "enabled"),
            ("opt-tools.mount", "enabled"),
            ("srv-data.mount", "enabled"),
            ("var-cache.swap", "masked"),
        ],
        "disable *\n",
    )
    .with_file("/usr/lib/systemd/system/tmp.mount", UNIT_FILE)
    .with_file("/usr/lib/systemd/system/opt-tools.mount", UNIT_FILE)
    .with_file("/etc/systemd/system/srv-data.mount", UNIT_FILE)
    .with_link("/etc/systemd/system/var-cache.swap", "/dev/null");
    let exec = owner_answer(exec, "tmp.mount", Some("systemd"));
    let exec = owner_answer(exec, "opt-tools.mount", None);
    let (section, warnings) = scan(&exec);

    // Positive control: the owned unit is carried.
    assert_eq!(state_change_names(&section), vec!["tmp.mount"]);
    assert!(matches!(
        section.state_changes[0].unit(),
        CarriedUnit::PackagedStorage(_)
    ));

    let findings: Vec<FindingRow> = section
        .mount_findings
        .iter()
        .map(|f| {
            (
                f.unit.name().as_unit_name().as_str(),
                f.unit.fragment(),
                f.state_change,
            )
        })
        .collect();
    assert_eq!(
        findings,
        vec![
            (
                "opt-tools.mount",
                &AdminStorageFragment::UnownedVendor,
                Some((UnitState::Enabled, Some(PresetDefault::Disable)))
            ),
            (
                "srv-data.mount",
                &AdminStorageFragment::Local,
                Some((UnitState::Enabled, Some(PresetDefault::Disable)))
            ),
            (
                "var-cache.swap",
                &AdminStorageFragment::MaskOnly,
                Some((UnitState::Masked, Some(PresetDefault::Disable)))
            ),
        ]
    );
    for finding in &section.mount_findings {
        // Read from the unit file, which has no `[Mount]` section here.
        assert_eq!(finding.mount_point, None);
        assert!(!finding.nofail);
    }
    assert!(warnings.is_empty(), "routed, not only warned: {warnings:?}");
}

/// A vendor timer enabled against its preset is a state change like any
/// unit. A local timer stays with the scheduled section, which enables it
/// today, so the units section gives it no second control.
#[test]
fn local_timer_is_left_to_the_scheduled_section() {
    let exec = kinds_mock(
        &[
            ("dnf-automatic.timer", "enabled"),
            ("site-backup.timer", "enabled"),
        ],
        "disable *\n",
    )
    .with_file("/usr/lib/systemd/system/dnf-automatic.timer", UNIT_FILE)
    .with_file("/etc/systemd/system/site-backup.timer", UNIT_FILE);
    let (section, _) = scan(&exec);

    assert_eq!(state_change_names(&section), vec!["dnf-automatic.timer"]);
    assert_eq!(
        inventory_names(&section),
        vec!["dnf-automatic.timer", "site-backup.timer"]
    );

    // An `/etc` copy over a vendor timer is the scheduled section's too;
    // its vendor-only twin above is a state change.
    let shadowed = kinds_mock(&[("dnf-automatic.timer", "enabled")], "disable *\n")
        .with_file("/usr/lib/systemd/system/dnf-automatic.timer", UNIT_FILE)
        .with_file("/etc/systemd/system/dnf-automatic.timer", UNIT_FILE);
    let (section, _) = scan(&shadowed);
    assert!(
        section.state_changes.is_empty(),
        "{:?}",
        section.state_changes
    );
    assert_eq!(inventory_names(&section), vec!["dnf-automatic.timer"]);
}

/// § 7.4 stock state: `remote-cryptsetup.target`, `Disabled` against an
/// `enable` preset as a stock RHEL 9 install leaves it, keeps its inventory
/// entry and is no state change, so no `disable` nobody chose is carried.
/// The same unit in another state is a state change, and so is another
/// target disabled against `enable`: the list decides, not the kind.
#[test]
fn stock_state_away_from_preset_is_not_a_state_change() {
    let unit = "remote-cryptsetup.target";
    let stock = kinds_mock(&[(unit, "disabled")], "enable remote-cryptsetup.target\n");
    let (section, warnings) = scan(&with_vendor_units(stock, &[unit]));
    assert!(
        section.state_changes.is_empty(),
        "{:?}",
        section.state_changes
    );
    let entry = &section.inventory[0];
    assert_eq!(
        (entry.unit.as_str(), entry.state, entry.preset),
        (unit, UnitState::Disabled, Some(PresetDefault::Enable))
    );
    assert!(warnings.is_empty(), "{warnings:?}");

    // Reversed: the same unit, not in the listed state.
    let enabled = kinds_mock(&[(unit, "enabled")], "disable *\n");
    let (section, _) = scan(&with_vendor_units(enabled, &[unit]));
    assert_eq!(state_change_names(&section), vec![unit]);

    let masked = kinds_mock(&[(unit, "masked")], "enable remote-cryptsetup.target\n")
        .with_link(&format!("/etc/systemd/system/{unit}"), "/dev/null");
    let (section, _) = scan(&with_vendor_units(masked, &[unit]));
    assert_eq!(state_change_names(&section), vec![unit]);
    assert_eq!(section.state_changes[0].current_state(), UnitState::Masked);

    // Another target in the listed state is not on the list.
    let other = "remote-fs.target";
    let exec = kinds_mock(&[(other, "disabled")], "enable remote-fs.target\n");
    let (section, _) = scan(&with_vendor_units(exec, &[other]));
    assert_eq!(state_change_names(&section), vec![other]);
}

// ── One owner per path (§ 7.7 of the units spec) ───────────────────

/// The admin unit directory.
const ETC_UNITS: &str = "/etc/systemd/system";

/// The two forms a mask takes on the host.
#[derive(Debug, Clone, Copy)]
enum MaskForm {
    /// `systemctl mask`: a link to `/dev/null`.
    DevNullLink,
    /// A zero-length regular file, which systemd also reads as a mask.
    EmptyFile,
}

const MASK_FORMS: [MaskForm; 2] = [MaskForm::DevNullLink, MaskForm::EmptyFile];

/// Write a mask on `unit` under `/etc/systemd/system` in `form`.
fn with_mask(exec: MockExecutor, unit: &str, form: MaskForm) -> MockExecutor {
    let path = format!("{ETC_UNITS}/{unit}");
    match form {
        MaskForm::DevNullLink => exec.with_link(&path, "/dev/null"),
        MaskForm::EmptyFile => exec.with_file(&path, ""),
    }
}

/// Make `/etc` hold only `/etc/systemd/system` with `entries`, so the units
/// scan and the config walk read the same tree.
fn with_etc_units(exec: MockExecutor, entries: Vec<&str>) -> MockExecutor {
    exec.with_dir("/etc", vec!["systemd"])
        .with_dir("/etc/systemd", vec!["system"])
        .with_dir(ETC_UNITS, entries)
}

/// The config entries the real config inspector records over `exec`, on a
/// host where no package owns anything under `/etc`.
fn config_entries(exec: &MockExecutor) -> Vec<ConfigFileEntry> {
    let source = pkg_source();
    let rpm_state = RpmState::default();
    let ctx = InspectionContext {
        source_system: &source,
        executor: exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };
    let section = match ConfigInspector::new().inspect(&ctx, &NullProgress) {
        Ok(output) => output.section,
        Err(InspectorError::Degraded { partial, .. }) => partial.section,
        Err(e) => panic!("the config inspector failed: {e:?}"),
    };
    let SectionData::Config(config) = section else {
        panic!("expected the config section");
    };
    config.files
}

fn config_paths(exec: &MockExecutor) -> Vec<String> {
    config_entries(exec).into_iter().map(|f| f.path).collect()
}

fn drop_in_paths(section: &UnitSection) -> Vec<&str> {
    section.drop_ins.iter().map(|d| d.path.as_str()).collect()
}

/// A comment-only file over a vendor unit: the positive control for every
/// mask below. It is a unit file, not a mask, so its path is a full shadow
/// in `drop_ins` and a config entry, which the pipeline then marks
/// `Inventory` (shadowed by the units section).
const CONTROL: &str = "comment.service";
const CONTROL_PATH: &str = "/etc/systemd/system/comment.service";

fn with_control(exec: MockExecutor) -> MockExecutor {
    let exec = exec
        .with_file(CONTROL_PATH, "# replaced\n\n")
        .with_file(&format!("/usr/lib/systemd/system/{CONTROL}"), UNIT_FILE);
    owner_answer(exec, CONTROL, Some("comment-pkg"))
}

fn assert_control_is_a_full_shadow(section: &UnitSection, config: &[String], case: &str) {
    let row = section
        .drop_ins
        .iter()
        .find(|d| d.path == CONTROL_PATH)
        .unwrap_or_else(|| panic!("{case}: the comment-only file is a full shadow row"));
    assert_eq!(row.unit, CONTROL, "{case}");
    assert_eq!(row.shadow_type, Some(ShadowType::FullShadow), "{case}");
    assert_eq!(row.content, "# replaced\n\n", "{case}");
    assert!(row.disposition.is_included(), "{case}");
    assert_eq!(
        fragment_of(section, CONTROL),
        &UnitFragment::Shadowed {
            file: file(CONTROL),
            owning_package: Some("comment-pkg".into()),
        },
        "{case}"
    );
    assert!(
        config.iter().any(|p| p == CONTROL_PATH),
        "{case}: the comment-only file is a config entry: {config:?}"
    );
}

/// T11, collector half. Each mask form over a vendor unit, with no vendor
/// file, and on a template, gives one `Masked` state change and nothing
/// else: no drop-in row and no config entry. The comment-only file beside
/// it, the same path kind with bytes in it, is a full shadow and a config
/// entry on the same host.
#[test]
fn t11_each_mask_form_is_one_masked_state_change_and_nothing_else() {
    struct Case {
        unit: &'static str,
        vendor_owner: Option<&'static str>,
        expected: UnitFragment,
    }
    let cases = [
        Case {
            unit: "cups.service",
            vendor_owner: Some("cups"),
            expected: UnitFragment::Vendor {
                file: file("cups.service"),
                owning_package: Some("cups".into()),
            },
        },
        Case {
            unit: "gone.service",
            vendor_owner: None,
            expected: UnitFragment::Absent,
        },
        Case {
            unit: "worker@.service",
            vendor_owner: Some("worker"),
            expected: UnitFragment::Vendor {
                file: file("worker@.service"),
                owning_package: Some("worker".into()),
            },
        },
    ];
    for case in &cases {
        for form in MASK_FORMS {
            let label = format!("{} {form:?}", case.unit);
            let exec = kinds_mock(
                &[(case.unit, "masked"), (CONTROL, "enabled")],
                "disable *\n",
            );
            let mut exec = with_control(with_mask(exec, case.unit, form));
            if let Some(owner) = case.vendor_owner {
                exec = owner_answer(
                    with_vendor_units(exec, &[case.unit]),
                    case.unit,
                    Some(owner),
                );
            }
            let exec = with_etc_units(exec, vec![case.unit, CONTROL]);
            let (section, warnings) = scan(&exec);
            let config = config_paths(&exec);
            assert!(warnings.is_empty(), "{label}: {warnings:?}");
            assert_control_is_a_full_shadow(&section, &config, &label);

            let masked: Vec<_> = section
                .state_changes
                .iter()
                .filter(|s| s.unit().name().as_str() == case.unit)
                .collect();
            assert_eq!(masked.len(), 1, "{label}: {:?}", section.state_changes);
            assert_eq!(masked[0].current_state(), UnitState::Masked, "{label}");
            assert_eq!(fragment_of(&section, case.unit), &case.expected, "{label}");

            let path = format!("{ETC_UNITS}/{}", case.unit);
            assert!(
                !drop_in_paths(&section).contains(&path.as_str()),
                "{label}: a mask is no drop-in row: {:?}",
                section.drop_ins
            );
            assert!(
                section.drop_ins.iter().all(|d| d.unit != case.unit),
                "{label}: {:?}",
                section.drop_ins
            );
            assert!(
                !config.contains(&path),
                "{label}: a mask is no config entry: {config:?}"
            );
        }
    }
}

/// The storage kinds and a name for each.
const STORAGE_UNITS: [&str; 3] = ["srv-data.mount", "srv-data.automount", "dev-sdb.swap"];

/// A non-empty storage file under `/etc/systemd/system`: a config entry,
/// the positive control for a storage mask's "no config entry".
const STORAGE_CONTROL: &str = "srv-other.mount";
const STORAGE_CONTROL_PATH: &str = "/etc/systemd/system/srv-other.mount";

/// T11, storage half. For each storage kind and both mask forms: a mask on
/// a package-owned vendor file is a carried `Masked` state change; on an
/// unowned vendor file, or on no file, it is a mount finding with state
/// `Masked` and no state change. None of them is a drop-in row or a config
/// entry; the comment-only control on the same host is both.
#[test]
fn t11_storage_masks_route_by_owner() {
    #[derive(Debug, Clone, Copy)]
    enum Vendor {
        Owned,
        Unowned,
        NoFile,
    }
    for unit in STORAGE_UNITS {
        for form in MASK_FORMS {
            for vendor in [Vendor::Owned, Vendor::Unowned, Vendor::NoFile] {
                let label = format!("{unit} {form:?} {vendor:?}");
                let exec = kinds_mock(&[(unit, "masked"), (CONTROL, "enabled")], "disable *\n");
                let mut exec = with_control(with_mask(exec, unit, form));
                match vendor {
                    Vendor::Owned => {
                        exec = owner_answer(with_vendor_units(exec, &[unit]), unit, Some("systemd"))
                    }
                    Vendor::Unowned => {
                        exec = owner_answer(with_vendor_units(exec, &[unit]), unit, None)
                    }
                    Vendor::NoFile => {}
                }
                let exec = with_etc_units(exec, vec![unit, CONTROL, STORAGE_CONTROL])
                    .with_file(STORAGE_CONTROL_PATH, "[Mount]\nWhere=/srv/other\n");
                let (section, warnings) = scan(&exec);
                let config = config_paths(&exec);
                assert!(warnings.is_empty(), "{label}: {warnings:?}");
                assert_control_is_a_full_shadow(&section, &config, &label);
                // Control for "no config entry": a non-empty storage file on
                // the same host is one.
                assert!(
                    config.iter().any(|p| p == STORAGE_CONTROL_PATH),
                    "{label}: {config:?}"
                );

                let change = section
                    .state_changes
                    .iter()
                    .find(|s| s.unit().name().as_str() == unit);
                let finding = section
                    .mount_findings
                    .iter()
                    .find(|f| f.unit.name().as_unit_name().as_str() == unit);
                match vendor {
                    Vendor::Owned => {
                        let change = change.unwrap_or_else(|| panic!("{label}: carried"));
                        assert_eq!(change.current_state(), UnitState::Masked, "{label}");
                        assert!(
                            matches!(change.unit(), CarriedUnit::PackagedStorage(u)
                                if u.owning_package() == "systemd"),
                            "{label}: {:?}",
                            change.unit()
                        );
                        assert!(finding.is_none(), "{label}: {finding:?}");
                    }
                    Vendor::Unowned | Vendor::NoFile => {
                        assert!(change.is_none(), "{label}: {change:?}");
                        let finding = finding.unwrap_or_else(|| panic!("{label}: a finding"));
                        assert_eq!(
                            finding.state_change,
                            Some((UnitState::Masked, Some(PresetDefault::Disable))),
                            "{label}"
                        );
                        let expected = match vendor {
                            Vendor::Unowned => AdminStorageFragment::UnownedVendor,
                            _ => AdminStorageFragment::MaskOnly,
                        };
                        assert_eq!(finding.unit.fragment(), &expected, "{label}");
                    }
                }

                let path = format!("{ETC_UNITS}/{unit}");
                assert!(
                    section
                        .drop_ins
                        .iter()
                        .all(|d| d.path != path && d.unit != unit),
                    "{label}: {:?}",
                    section.drop_ins
                );
                assert!(!config.contains(&path), "{label}: {config:?}");
            }
        }
    }
}

/// § 7.7: a regular, non-empty file of a storage kind beside its vendor
/// file is a config entry and a mount finding, never a full shadow. A
/// socket in the same layout is a full shadow row (the positive control).
/// A timer in the same layout stays with the scheduled section, which
/// writes every timer file under `/etc`: no row here, or two sections
/// would write one file.
#[test]
fn regular_storage_file_is_a_config_entry_and_a_mount_finding_never_a_shadow() {
    for unit in STORAGE_UNITS {
        let socket = "app.socket";
        let timer = "app.timer";
        let exec = kinds_mock(
            &[(unit, "enabled"), (socket, "enabled"), (timer, "enabled")],
            "disable *\n",
        );
        let mut exec = with_etc_units(exec, vec![unit, socket, timer]);
        for name in [unit, socket, timer] {
            exec = exec.with_file(
                &format!("{ETC_UNITS}/{name}"),
                "[Unit]\nDescription=admin\n",
            );
            exec = owner_answer(with_vendor_units(exec, &[name]), name, Some("vendor-pkg"));
        }
        let (section, warnings) = scan(&exec);
        let config = config_paths(&exec);
        assert!(warnings.is_empty(), "{unit}: {warnings:?}");

        // Positive control: the socket is a full shadow.
        let socket_path = format!("{ETC_UNITS}/{socket}");
        let row = section
            .drop_ins
            .iter()
            .find(|d| d.path == socket_path)
            .unwrap_or_else(|| {
                panic!("{unit}: the socket shadow is a row: {:?}", section.drop_ins)
            });
        assert_eq!(row.shadow_type, Some(ShadowType::FullShadow));
        assert!(config.contains(&socket_path), "{config:?}");

        let path = format!("{ETC_UNITS}/{unit}");
        assert!(
            section.drop_ins.iter().all(|d| d.path != path),
            "{unit}: a storage file is no full shadow: {:?}",
            section.drop_ins
        );
        assert!(config.contains(&path), "{unit}: a config entry: {config:?}");
        let finding = section
            .mount_findings
            .iter()
            .find(|f| f.unit.name().as_unit_name().as_str() == unit)
            .unwrap_or_else(|| panic!("{unit}: a mount finding"));
        assert!(
            matches!(
                finding.unit.fragment(),
                AdminStorageFragment::Shadowed { .. }
            ),
            "{unit}: {:?}",
            finding.unit.fragment()
        );
        assert!(!state_change_names(&section).contains(&unit));

        let timer_path = format!("{ETC_UNITS}/{timer}");
        assert!(
            section.drop_ins.iter().all(|d| d.path != timer_path),
            "{unit}: {:?}",
            section.drop_ins
        );
    }
}

/// § 7.7 drop-ins and plan review focus 5: drop-ins are collected for every
/// parsed name of every kind, for `*.device.d` and `*.scope.d`, for a unit
/// systemd lists as generated, and for a `.d` directory whose name does not
/// parse. Each row keeps its `unit` string as written; the names that do
/// not parse stay text and never become a state change. A `.conf` in a
/// type-wide directory (`service.d`) is no row: it stays a config entry.
#[test]
fn drop_ins_are_collected_for_every_kind_and_for_names_that_do_not_parse() {
    let dirs = [
        ("httpd.service", "override.conf"),
        ("cockpit.socket", "listen.conf"),
        ("tmp.mount", "options.conf"),
        ("backup.timer", "schedule.conf"),
        ("dev-sda.device", "tag.conf"),
        ("session-1.scope", "limits.conf"),
        ("run-gen.mount", "gen.conf"),
        ("foo.servic", "typo.conf"),
    ];
    let exec = kinds_mock(
        &[
            ("httpd.service", "disabled"),
            ("run-gen.mount", "generated"),
        ],
        "disable *\n",
    );
    let mut entries: Vec<String> = dirs.iter().map(|(u, _)| format!("{u}.d")).collect();
    entries.push("service.d".into());
    let mut exec = with_etc_units(exec, entries.iter().map(String::as_str).collect());
    for (unit, conf) in dirs {
        let dir = format!("{ETC_UNITS}/{unit}.d");
        exec = exec
            .with_dir(&dir, vec![conf])
            .with_file(&format!("{dir}/{conf}"), "[Unit]\nDescription=x\n");
    }
    let type_wide = format!("{ETC_UNITS}/service.d/10-timeout.conf");
    exec = exec
        .with_dir(&format!("{ETC_UNITS}/service.d"), vec!["10-timeout.conf"])
        .with_file(&type_wide, "[Service]\nTimeoutStopSec=10\n");
    let exec = with_vendor_units(exec, &["httpd.service"]);
    let (section, warnings) = scan(&exec);
    assert!(warnings.is_empty(), "{warnings:?}");

    let rows: Vec<(&str, &str)> = section
        .drop_ins
        .iter()
        .map(|d| (d.unit.as_str(), d.path.as_str()))
        .collect();
    let expected: Vec<(String, String)> = dirs
        .iter()
        .map(|(u, c)| (u.to_string(), format!("{ETC_UNITS}/{u}.d/{c}")))
        .collect();
    let expected: Vec<(&str, &str)> = expected
        .iter()
        .map(|(u, p)| (u.as_str(), p.as_str()))
        .collect();
    assert_eq!(rows, expected);
    for row in &section.drop_ins {
        assert!(row.disposition.is_included(), "{row:?}");
    }

    // The rows for names that do not parse hold text, not a proved name,
    // and none of them is a state change or an inventory entry.
    for unparsed in ["dev-sda.device", "session-1.scope", "foo.servic"] {
        assert!(
            UnitName::try_from(unparsed.to_owned()).is_err(),
            "{unparsed}"
        );
    }
    assert!(
        section.state_changes.is_empty(),
        "{:?}",
        section.state_changes
    );
    assert!(
        section.mount_findings.is_empty(),
        "{:?}",
        section.mount_findings
    );
    assert_eq!(inventory_names(&section), vec!["httpd.service"]);

    // The type-wide drop-in is no row and stays a config entry.
    assert!(section.drop_ins.iter().all(|d| d.path != type_wide));
    let config = config_paths(&exec);
    assert!(config.contains(&type_wide), "{config:?}");
}

/// A full shadow over a vendor unit whose `/etc` file is not text, or is
/// over the executor's size cap (`InvalidData`), keeps its state change and
/// does not degrade the scan. Its content cannot be carried, so it is no
/// drop-in row: an empty drop-in would stage an empty file, which systemd
/// reads as a mask. The config entry the config inspector records for it,
/// a `ContentNotCaptured` advisory, is its one row, as for any file under
/// `/etc` that cannot be read as text. The readable twin is a full shadow
/// row.
#[test]
fn full_shadow_that_is_not_text_keeps_its_state_change_and_does_not_degrade() {
    let exec = kinds_mock(
        &[("latin1.service", "enabled"), ("twin.service", "enabled")],
        "disable *\n",
    )
    .with_file_error(
        "/etc/systemd/system/latin1.service",
        std::io::ErrorKind::InvalidData,
    )
    .with_file("/etc/systemd/system/twin.service", UNIT_FILE);
    let exec = with_etc_units(exec, vec!["latin1.service", "twin.service"]);
    let exec = with_vendor_units(exec, &["latin1.service", "twin.service"]);
    let exec = owner_answer(exec, "latin1.service", Some("a"));
    let exec = owner_answer(exec, "twin.service", Some("b"));
    let (section, warnings) = scan(&exec);
    assert!(warnings.is_empty(), "{warnings:?}");

    // Control: the readable twin.
    assert_eq!(
        fragment_of(&section, "twin.service"),
        &UnitFragment::Shadowed {
            file: file("twin.service"),
            owning_package: Some("b".into()),
        }
    );
    assert_eq!(
        drop_in_paths(&section),
        vec!["/etc/systemd/system/twin.service"]
    );

    assert_eq!(
        fragment_of(&section, "latin1.service"),
        &UnitFragment::Shadowed {
            file: file("latin1.service"),
            owning_package: Some("a".into()),
        }
    );
    let config = config_entries_not_degraded(&exec);
    let entry = config
        .iter()
        .find(|f| f.path == "/etc/systemd/system/latin1.service")
        .unwrap_or_else(|| panic!("a config entry: {config:?}"));
    assert!(is_content_not_captured(entry), "{:?}", entry.disposition);
}

/// The config entries the real config inspector records over `exec`, on a
/// host where no package owns anything under `/etc`. The inspector must
/// return `Ok`: a degraded config scan fails the test.
fn config_entries_not_degraded(exec: &MockExecutor) -> Vec<ConfigFileEntry> {
    let source = pkg_source();
    let rpm_state = RpmState::default();
    let ctx = InspectionContext {
        source_system: &source,
        executor: exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };
    let output = ConfigInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap_or_else(|e| panic!("the config inspector does not degrade: {e:?}"));
    let SectionData::Config(config) = output.section else {
        panic!("expected the config section");
    };
    config.files
}

/// A host listing one disabled vendor unit, as its preset says: the scan
/// runs to the drop-in collection, and nothing is a state change.
fn quiet_mock() -> MockExecutor {
    with_vendor_units(
        kinds_mock(&[("sshd.service", "disabled")], "disable *\n"),
        &["sshd.service"],
    )
}

fn is_content_not_captured(entry: &ConfigFileEntry) -> bool {
    matches!(
        entry.disposition,
        FindingKind::Advisory {
            advisory_type: AdvisoryType::ContentNotCaptured,
            ..
        }
    )
}

/// A drop-in `.conf` that is not text, or is over the executor's size cap
/// (`InvalidData`), in the drop-in directory of a service, a socket and a
/// `.device`, is no drop-in row and does not degrade either inspector: the
/// config entry, a `ContentNotCaptured` advisory, is its one row. The
/// readable twin in each directory is a drop-in row. A regular file whose
/// name ends in `.d` directly under `/etc/systemd/system` is not a drop-in
/// directory: listing it fails with `NotADirectory`, which does not degrade
/// the units scan, and the file stays a config entry.
#[test]
fn drop_in_that_is_not_text_is_its_config_entry_and_does_not_degrade() {
    let units = ["httpd.service", "cockpit.socket", "dev-sda.device"];
    let file_named_d = format!("{ETC_UNITS}/notes.d");
    let exec = quiet_mock();
    let mut entries: Vec<String> = units.iter().map(|u| format!("{u}.d")).collect();
    entries.push("notes.d".into());
    let mut exec = with_etc_units(exec, entries.iter().map(String::as_str).collect())
        .with_file(&file_named_d, "not a directory\n")
        .with_dir_error(&file_named_d, std::io::ErrorKind::NotADirectory);
    for unit in units {
        let dir = format!("{ETC_UNITS}/{unit}.d");
        exec = exec
            .with_dir(&dir, vec!["latin1.conf", "twin.conf"])
            .with_file_error(
                &format!("{dir}/latin1.conf"),
                std::io::ErrorKind::InvalidData,
            )
            .with_file(&format!("{dir}/twin.conf"), "[Unit]\nDescription=x\n");
    }
    let (section, warnings) = scan(&exec);
    assert!(warnings.is_empty(), "{warnings:?}");

    // Control: each readable twin is a drop-in row.
    let twins: Vec<String> = units
        .iter()
        .map(|u| format!("{ETC_UNITS}/{u}.d/twin.conf"))
        .collect();
    assert_eq!(drop_in_paths(&section), twins);

    let config = config_entries_not_degraded(&exec);
    for unit in units {
        let path = format!("{ETC_UNITS}/{unit}.d/latin1.conf");
        let entry = config
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path}: a config entry: {config:?}"));
        assert!(is_content_not_captured(entry), "{path}: {entry:?}");
    }
    assert!(
        config.iter().any(|f| f.path == file_named_d),
        "the file named *.d is a config entry: {config:?}"
    );
}

/// The full shadow scan reads the `/etc` entry first: an alias link
/// directly under `/etc/systemd/system` is never a shadow, so a vendor read
/// error for its name does not degrade the scan. Control: the same vendor
/// error under a regular `/etc` file, which may be a shadow, degrades it.
#[test]
fn vendor_read_error_under_an_etc_link_does_not_degrade() {
    let name = "alias.service";
    let etc = format!("{ETC_UNITS}/{name}");
    let vendor = format!("/usr/lib/systemd/system/{name}");
    let host = |exec: MockExecutor| {
        with_etc_units(exec, vec![name])
            .with_link_error(&vendor, std::io::ErrorKind::PermissionDenied)
    };

    let linked = host(
        quiet_mock()
            .with_file("/usr/lib/systemd/system/real.service", UNIT_FILE)
            .with_link(&etc, "/usr/lib/systemd/system/real.service"),
    );
    let (section, warnings) = scan(&linked);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(section.drop_ins.is_empty(), "{:?}", section.drop_ins);

    let regular = host(quiet_mock().with_file(&etc, UNIT_FILE));
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &regular,
        rpm_state: None,
        baseline_data: None,
    };
    match UnitsInspector::new().inspect(&ctx, &NullProgress) {
        Err(InspectorError::Degraded { reason, .. }) => {
            assert!(reason.contains(&vendor), "{reason}");
        }
        other => panic!("a vendor read error under a regular file degrades: {other:?}"),
    }
}

// ── Mount findings (§ 6.5 of the units spec) ───────────────────────

/// A mount finding as a comparable row: name, fragment, recorded state,
/// mount point and `nofail`.
type MountRow = (
    String,
    AdminStorageFragment,
    Option<(UnitState, Option<PresetDefault>)>,
    Option<String>,
    bool,
);

fn mount_rows(section: &UnitSection) -> Vec<MountRow> {
    let mut rows: Vec<MountRow> = section
        .mount_findings
        .iter()
        .map(|f| {
            (
                f.unit.name().as_unit_name().as_str().to_owned(),
                f.unit.fragment().clone(),
                f.state_change,
                f.mount_point.clone(),
                f.nofail,
            )
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

/// A mount source holding a credential, which no field may copy.
const MOUNT_SOURCE: &str = "What=https://files.example/volume?token=collector-source-1\n";

fn mount_unit(section: &str, lines: &str) -> String {
    format!("[Unit]\nDescription=admin mount\n\n[{section}]\n{MOUNT_SOURCE}{lines}")
}

/// § 6.5: every source of a mount finding, each with `mount_point` and
/// `nofail` read from the file systemd loads.
///
/// - Regular, non-empty storage files under `/etc/systemd/system`, whatever
///   their state: static (not listed), disabled, a full copy over a vendor
///   file, an enabled copy over a vendor file whose preset enables it, an
///   automount, a relative `Where=`, and a file that is not text.
/// - An unowned vendor unit only when enabled or masked: enabled at its
///   preset is a finding; the same unit disabled is not.
/// - A mask with no unit file: `MaskOnly`, no mount point.
///
/// A package-owned vendor mount enabled at its preset is neither a finding
/// nor a state change. `What=` is in no finding.
#[test]
fn mount_findings_come_from_every_source_section_6_5_names() {
    let preset = "enable opt-on.mount\nenable srv-shadow.mount\nenable tmp.mount\ndisable *\n";
    let exec = kinds_mock(
        &[
            ("srv-off.mount", "disabled"),
            ("data.swap", "disabled"),
            ("srv-shadow.mount", "enabled"),
            ("opt-on.mount", "enabled"),
            ("opt-off.mount", "disabled"),
            ("opt-masked.mount", "masked"),
            ("gone.mount", "masked"),
            ("tmp.mount", "enabled"),
        ],
        preset,
    );
    let etc_files = [
        (
            "srv-static.mount",
            mount_unit("Mount", "Where=/srv/static\nOptions=defaults,nofail\n"),
        ),
        ("srv-off.mount", mount_unit("Mount", "Where=/srv/off\n")),
        (
            "data.swap",
            mount_unit("Swap", "Options=defaults,nofail\r\n"),
        ),
        (
            "srv-shadow.mount",
            mount_unit("Mount", "Where=/srv/shadow \r\n"),
        ),
        (
            "mnt-auto.automount",
            mount_unit("Automount", "Where=/mnt/auto\n"),
        ),
        (
            "relative.mount",
            mount_unit("Mount", "Where=srv\nOptions=nofail\n"),
        ),
    ];
    let mut entries: Vec<&str> = etc_files.iter().map(|(n, _)| *n).collect();
    entries.extend(["latin1.mount", "opt-masked.mount", "gone.mount"]);
    let mut exec = with_etc_units(exec, entries);
    for (name, content) in &etc_files {
        exec = exec.with_file(&format!("{ETC_UNITS}/{name}"), content);
    }
    exec = exec.with_file_error(
        &format!("{ETC_UNITS}/latin1.mount"),
        std::io::ErrorKind::InvalidData,
    );
    exec = with_mask(exec, "opt-masked.mount", MaskForm::DevNullLink);
    exec = with_mask(exec, "gone.mount", MaskForm::DevNullLink);
    for (name, content) in [
        ("data.swap", mount_unit("Swap", "")),
        (
            "srv-shadow.mount",
            mount_unit("Mount", "Where=/srv/vendor\n"),
        ),
        ("opt-on.mount", mount_unit("Mount", "Where=/opt/on\n")),
        ("opt-off.mount", mount_unit("Mount", "Where=/opt/off\n")),
        (
            "opt-masked.mount",
            mount_unit("Mount", "Where=/opt/masked\nOptions=nofail\n"),
        ),
        ("tmp.mount", mount_unit("Mount", "Where=/tmp\n")),
    ] {
        exec = exec.with_file(&format!("/usr/lib/systemd/system/{name}"), &content);
    }
    for (name, owner) in [
        ("data.swap", Some("swap-pkg")),
        ("srv-shadow.mount", Some("srv-pkg")),
        ("opt-on.mount", None),
        ("opt-off.mount", None),
        ("opt-masked.mount", None),
        ("tmp.mount", Some("systemd")),
    ] {
        exec = owner_answer(exec, name, owner);
    }
    let (section, warnings) = scan(&exec);
    assert!(warnings.is_empty(), "{warnings:?}");

    let enabled_at_preset = Some((UnitState::Enabled, Some(PresetDefault::Enable)));
    let masked = Some((UnitState::Masked, Some(PresetDefault::Disable)));
    let path = |p: &str| Some(p.to_owned());
    let mut expected: Vec<MountRow> = vec![
        (
            "data.swap".into(),
            AdminStorageFragment::Shadowed {
                owning_package: Some("swap-pkg".into()),
            },
            None,
            None,
            true,
        ),
        (
            "gone.mount".into(),
            AdminStorageFragment::MaskOnly,
            masked,
            None,
            false,
        ),
        (
            "latin1.mount".into(),
            AdminStorageFragment::Local,
            None,
            None,
            false,
        ),
        (
            "mnt-auto.automount".into(),
            AdminStorageFragment::Local,
            None,
            path("/mnt/auto"),
            false,
        ),
        (
            "opt-masked.mount".into(),
            AdminStorageFragment::UnownedVendor,
            masked,
            path("/opt/masked"),
            true,
        ),
        (
            "opt-on.mount".into(),
            AdminStorageFragment::UnownedVendor,
            enabled_at_preset,
            path("/opt/on"),
            false,
        ),
        (
            "relative.mount".into(),
            AdminStorageFragment::Local,
            None,
            None,
            true,
        ),
        (
            "srv-off.mount".into(),
            AdminStorageFragment::Local,
            None,
            path("/srv/off"),
            false,
        ),
        (
            "srv-shadow.mount".into(),
            AdminStorageFragment::Shadowed {
                owning_package: Some("srv-pkg".into()),
            },
            enabled_at_preset,
            path("/srv/shadow"),
            false,
        ),
        (
            "srv-static.mount".into(),
            AdminStorageFragment::Local,
            None,
            path("/srv/static"),
            true,
        ),
    ];
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(mount_rows(&section), expected);

    // The unowned vendor unit that is disabled is no finding; its enabled
    // twin above is. The owned vendor mount at its preset is neither a
    // finding nor a state change.
    assert!(
        section.state_changes.is_empty(),
        "{:?}",
        section.state_changes
    );

    // No field copies the mount source. Presence first: the files hold it.
    assert!(etc_files.iter().all(|(_, c)| c.contains(MOUNT_SOURCE)));
    let written = serde_json::to_string(&section).expect("the section serializes");
    assert!(written.contains("srv-static.mount"));
    assert!(!written.contains("collector-source-1"), "{written}");
}

/// A local mount unit file that cannot be read for a reason other than its
/// content (here, permission) degrades the scan, naming the file: whether
/// it is a finding, and what it mounts, is unknown. A file that is not text
/// is a finding with no mount point and does not degrade (above).
#[test]
fn unreadable_mount_unit_file_degrades_naming_it() {
    let exec = with_etc_units(quiet_mock(), vec!["srv-locked.mount"]).with_file_error(
        &format!("{ETC_UNITS}/srv-locked.mount"),
        std::io::ErrorKind::PermissionDenied,
    );
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };
    match UnitsInspector::new().inspect(&ctx, &NullProgress) {
        Err(InspectorError::Degraded { reason, .. }) => {
            assert!(
                reason.contains("/etc/systemd/system/srv-locked.mount"),
                "{reason}"
            );
        }
        other => panic!("an unreadable mount unit file degrades the scan: {other:?}"),
    }
}
