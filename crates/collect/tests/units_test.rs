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
    AdminStorageFragment, CarriedUnit, InstallUnknown, LinkRelation, ManualLinkCause, MemberLinks,
    OrdinaryKind, PresetDefault, UnitAction, UnitFragment, UnitKind, UnitName, UnitSection,
    UnitState,
};
use inspectah_core::types::warnings::Warning;

/// Standard fixture: `systemctl list-unit-files` output.
const SYSTEMCTL_FIXTURE: &str =
    include_str!("../../../testdata/fixtures/services/systemctl-list-unit-files.txt");

/// Standard fixture: preset file.
const PRESET_FIXTURE: &str =
    include_str!("../../../testdata/fixtures/services/preset-90-default.preset");

/// Standard fixture: drop-in override.
const DROPIN_FIXTURE: &str =
    include_str!("../../../testdata/fixtures/services/dropin-httpd-override.conf");

/// A minimal unit file body, wanted by `multi-user.target`.
const UNIT_FILE: &str = "[Unit]\nDescription=test unit\n\n[Install]\nWantedBy=multi-user.target\n";

/// The directory `systemctl enable` links a unit [`UNIT_FILE`] describes
/// into.
const MULTI_USER_WANTS: &str = "/etc/systemd/system/multi-user.target.wants";

/// `exec` with the link `systemctl enable` makes for `unit`, whose
/// `[Install]` is [`UNIT_FILE`]'s, added to what `/etc/systemd/system`
/// already lists. A host on which `systemctl list-unit-files` says
/// `enabled` has the link that makes it so.
fn with_enable_link(exec: MockExecutor, unit: &str) -> MockExecutor {
    exec.with_dir_entry("/etc/systemd/system", "multi-user.target.wants")
        .with_dir_entry(MULTI_USER_WANTS, unit)
        .with_link(
            &format!("{MULTI_USER_WANTS}/{unit}"),
            &format!("/usr/lib/systemd/system/{unit}"),
        )
}

/// `exec` with the enable link of every unit `listing` lists `enabled`.
fn with_enable_links(mut exec: MockExecutor, listing: &str) -> MockExecutor {
    for line in listing.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if let [unit, "enabled", ..] = words.as_slice() {
            exec = with_enable_link(exec, unit);
        }
    }
    exec
}

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
    let exec = with_vendor_units(MockExecutor::new(), &listed_units(SYSTEMCTL_FIXTURE))
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
        .with_dir_entry("/etc/systemd/system", "httpd.service.d")
        .with_dir("/etc/systemd/system/httpd.service.d", vec!["override.conf"])
        .with_file(
            "/etc/systemd/system/httpd.service.d/override.conf",
            DROPIN_FIXTURE,
        );
    with_enable_links(exec, SYSTEMCTL_FIXTURE)
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
    let exec = with_vendor_units(exec, &["bluetooth.service", "cups.service", "sshd.service"]);
    let exec = with_enable_link(with_enable_link(exec, "bluetooth.service"), "sshd.service");

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
    let exec = with_enable_link(exec, "sshd.service");

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
    let exec = with_enable_link(with_vendor_units(exec, &["sshd.service"]), "sshd.service");

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
        .with_dir_entry("/etc/systemd/system", "myapp.service.d")
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
    let exec = with_enable_links(
        with_vendor_units(exec, &listed_units(SYSTEMCTL_FIXTURE)),
        SYSTEMCTL_FIXTURE,
    );

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
    let exec = with_enable_link(with_enable_link(exec, "chronyd.service"), "sshd.service");
    let exec = with_vendor_units(exec, &["cups.service"]);

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
    let exec = with_enable_link(exec, "httpd.service");

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
    let exec = with_enable_link(exec, "httpd.service");

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
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --no-pager",
            ExecResult {
                stdout: listing.clone(),
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
    with_enable_links(exec, &listing)
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
    .with_file("/etc/systemd/system/comment.service", COMMENT_SHADOW);
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
/// mask and neither loses the unit. Its `[Install]` cannot be read, so the
/// section is undetermined, with `Also=` taken as assigned: each enabled
/// unit, `/etc` or vendor, gets no state change, one
/// `UndeterminedInstall` record with reason `Unreadable` (the `/etc` unit
/// with its file), each of its links reported with that reason, and one
/// warning naming the file. The listing's `enabled` decides nothing. A
/// mask over such a vendor file hides that file. The scan is not degraded
/// (`scan` expects success).
#[test]
fn unit_file_that_is_not_text_is_an_undetermined_section() {
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
    );
    let exec = owner_answer(exec, "masked-latin1.service", Some("b"));
    let (section, _) = scan(&exec);
    let warnings = scan_warnings(&exec);

    assert_eq!(
        fragment_of(&section, "masked-latin1.service"),
        &UnitFragment::Vendor {
            file: file("masked-latin1.service"),
            owning_package: Some("b".into()),
        }
    );
    assert_eq!(state_change_names(&section), ["masked-latin1.service"]);
    let etc_file = "/etc/systemd/system/etc-latin1.service";
    let vendor_file = "/usr/lib/systemd/system/vendor-latin1.service";
    assert_eq!(
        undetermined(&section),
        [
            (
                "etc-latin1.service",
                InstallUnknown::Unreadable,
                vec![etc_file]
            ),
            ("vendor-latin1.service", InstallUnknown::Unreadable, vec![]),
        ]
    );
    let cause = ManualLinkCause::Undetermined {
        reason: InstallUnknown::Unreadable,
    };
    assert_eq!(
        manual_links(&section),
        [
            (
                LinkRelation::Wants,
                "multi-user.target",
                "etc-latin1.service",
                cause
            ),
            (
                LinkRelation::Wants,
                "multi-user.target",
                "vendor-latin1.service",
                cause
            ),
        ]
    );
    for (unit, path) in [
        ("etc-latin1.service", etc_file),
        ("vendor-latin1.service", vendor_file),
    ] {
        assert!(
            has_unreadable_install_warning(&warnings, unit, path),
            "{unit}: {warnings:?}"
        );
    }
    assert_eq!(warnings.len(), 2, "{warnings:?}");
}

/// Whether `warnings` holds the warning for `unit` whose install file
/// `path` cannot be read as text: the message names both, and the
/// warning's fields carry both.
fn has_unreadable_install_warning(warnings: &[Warning], unit: &str, path: &str) -> bool {
    warnings.iter().any(|w| {
        w.message.contains(unit)
            && w.message.contains(path)
            && w.extra.get("unit") == Some(&serde_json::json!(unit))
            && w.extra
                .get("paths")
                .and_then(|p| p.as_array())
                .is_some_and(|paths| paths.contains(&serde_json::json!(path)))
    })
}

/// An `/etc` drop-in that cannot be read as text makes the unit's section
/// undetermined, with `Also=` taken as assigned, whatever the listing
/// says: an enabled vendor unit under a `disable` preset gets no `enable`
/// row, and a disabled one with no link under an `enable` preset gets no
/// `disable` row. Each gets one `UndeterminedInstall` record listing the
/// drop-in, and a warning naming it. Control: the same hosts with a
/// readable drop-in that changes no `[Install]` key are the two rows.
#[test]
fn an_install_drop_in_that_is_not_text_gives_the_listing_no_say() {
    let drop_in = "/etc/systemd/system/app.service.d/latin1.conf";
    let host = |state: &str, preset: &str, readable: bool| {
        let exec = with_vendor_units(
            kinds_mock(&[("app.service", state)], preset),
            &["app.service"],
        )
        .with_dir_entry("/etc/systemd/system", "app.service.d")
        .with_dir("/etc/systemd/system/app.service.d", vec!["latin1.conf"]);
        let exec = if readable {
            exec.with_file(drop_in, "[Service]\nNice=5\n")
        } else {
            exec.with_file_error(drop_in, std::io::ErrorKind::InvalidData)
        };
        owner_answer(exec, "app.service", Some("apppkg"))
    };
    for (state, preset, action) in [
        ("enabled", "disable *\n", UnitAction::Enable),
        ("disabled", "enable *\n", UnitAction::Disable),
    ] {
        let (control, _) = scan(&host(state, preset, true));
        assert_eq!(state_change_names(&control), ["app.service"], "{state}");
        assert_eq!(control.state_changes[0].implied_action(), action, "{state}");
        assert!(control.undetermined_installs.is_empty(), "{state}");

        let exec = host(state, preset, false);
        let (section, _) = scan(&exec);
        assert!(
            section.state_changes.is_empty(),
            "{state}: {:?}",
            state_change_names(&section)
        );
        assert_eq!(
            undetermined(&section),
            [("app.service", InstallUnknown::Unreadable, vec![drop_in])],
            "{state}"
        );
        let warnings = scan_warnings(&exec);
        assert!(
            has_unreadable_install_warning(&warnings, "app.service", drop_in),
            "{state}: {warnings:?}"
        );
    }
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

// ── The effective unit and the search path (§ 7.1, § 7.2, T24) ─────
//
// The scan records no instance until install state is read from links,
// so the instance cases of T24 (the concrete instance as fragment, and an
// instance file elsewhere on the search path) are tested on `resolve` in
// `crates/collect/src/inspectors/units/resolve.rs`.

/// An enabled vendor `app.service`, a state change against its preset.
fn vendor_app() -> MockExecutor {
    with_vendor_units(
        listing_mock(&[("app.service", "enabled")]),
        &["app.service"],
    )
}

/// An enabled `app.service` under `/etc` only.
fn local_app() -> MockExecutor {
    listing_mock(&[("app.service", "enabled")])
        .with_file("/etc/systemd/system/app.service", UNIT_FILE)
}

/// `vendor_app` with a vendor drop-in `10-vendor.conf` in its own `.d`
/// that holds `[Install]`.
fn vendor_app_with_vendor_drop_in() -> MockExecutor {
    vendor_app()
        .with_dir(
            "/usr/lib/systemd/system/app.service.d",
            vec!["10-vendor.conf"],
        )
        .with_file(
            "/usr/lib/systemd/system/app.service.d/10-vendor.conf",
            INSTALL_DROP_IN,
        )
}

/// A drop-in holding an `[Install]` section.
const INSTALL_DROP_IN: &str = "[Install]\nWantedBy=multi-user.target\n";

/// `exec` with the drop-in `file` in `dir`, holding `text`.
fn with_drop_in(exec: MockExecutor, dir: &str, file: &str, text: &str) -> MockExecutor {
    exec.with_dir(dir, vec![file])
        .with_file(&format!("{dir}/{file}"), text)
}

/// Whether the scan of `exec` has a state change for `unit`, and its
/// warnings.
fn row_and_warnings(exec: &MockExecutor, unit: &str) -> (bool, Vec<String>) {
    let (section, warnings) = scan(exec);
    let row = section
        .state_changes
        .iter()
        .any(|s| s.unit().name().as_str() == unit);
    (row, warnings)
}

/// The structured warnings a scan of `exec` produces.
fn scan_warnings(exec: &MockExecutor) -> Vec<Warning> {
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: exec,
        rpm_state: None,
        baseline_data: None,
    };
    UnitsInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("the scan succeeds")
        .warnings
}

/// Whether `warnings` holds the search-path warning for `unit` and
/// `path`: the message names both, and the warning's fields carry both.
fn has_search_path_warning(warnings: &[Warning], unit: &str, path: &str) -> bool {
    warnings.iter().any(|w| {
        w.message.contains(unit)
            && w.message.contains(path)
            && w.extra.get("unit") == Some(&serde_json::json!(unit))
            && w.extra.get("path") == Some(&serde_json::json!(path))
    })
}

/// T24, the search path: a file on systemd's search path that this
/// collector does not read, and that changes what loads or what `enable`
/// does, makes the unit unsupported: a warning naming the unit and the
/// path, and no state change. Each case's control is the same host
/// without the extra file, which has the row and no such warning.
#[test]
fn a_file_the_search_path_loads_in_place_refuses_the_unit() {
    /// A label, the control host, the path the case adds, and the host
    /// with it.
    type Case = (
        &'static str,
        fn() -> MockExecutor,
        &'static str,
        MockExecutor,
    );
    let cases: Vec<Case> = vec![
        (
            "a copy under /usr/local/lib",
            vendor_app,
            "/usr/local/lib/systemd/system/app.service",
            vendor_app().with_file("/usr/local/lib/systemd/system/app.service", UNIT_FILE),
        ),
        (
            "a copy over an /etc unit from a directory above /etc",
            local_app,
            "/etc/systemd/system.control/app.service",
            local_app().with_file("/etc/systemd/system.control/app.service", UNIT_FILE),
        ),
        (
            "a drop-in with [Install] in the unit's own .d under /run",
            vendor_app,
            "/run/systemd/system/app.service.d/50-install.conf",
            with_drop_in(
                vendor_app(),
                "/run/systemd/system/app.service.d",
                "50-install.conf",
                INSTALL_DROP_IN,
            ),
        ),
        (
            "a drop-in with [Install] reaches a local unit too",
            local_app,
            "/run/systemd/system/app.service.d/50-install.conf",
            with_drop_in(
                local_app(),
                "/run/systemd/system/app.service.d",
                "50-install.conf",
                INSTALL_DROP_IN,
            ),
        ),
        (
            "a drop-in with no [Install] under a vendor drop-in's file name",
            vendor_app_with_vendor_drop_in,
            "/run/systemd/system/app.service.d/10-vendor.conf",
            with_drop_in(
                vendor_app_with_vendor_drop_in(),
                "/run/systemd/system/app.service.d",
                "10-vendor.conf",
                "[Service]\nNice=5\n",
            ),
        ),
    ];
    for (label, control, path, refused) in cases {
        let (row, warnings) = row_and_warnings(&control(), "app.service");
        assert!(row, "{label}: the control has its row: {warnings:?}");
        assert!(
            !warnings.iter().any(|w| w.contains(path)),
            "{label}: the control does not warn: {warnings:?}"
        );

        let (row, warnings) = row_and_warnings(&refused, "app.service");
        assert!(!row, "{label}: no state change");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("unit app.service ") && w.contains(path)),
            "{label}: a warning naming the unit and {path}: {warnings:?}"
        );
        let warnings = scan_warnings(&refused);
        assert!(
            has_search_path_warning(&warnings, "app.service", path),
            "{label}: the warning's fields carry the unit and {path}: {warnings:?}"
        );
    }
}

/// With no file in the two directories read, a file for the name anywhere
/// on the search path is what systemd loads: the warning names that path
/// in place of the no-file warning, and there is no state change. Control:
/// the same listing with no file anywhere gives the no-file warning.
#[test]
fn a_unit_found_only_elsewhere_on_the_search_path_names_that_path() {
    let path = "/usr/local/lib/systemd/system/app.service";
    let control = listing_mock(&[("app.service", "enabled")]);
    let warnings = scan_warnings(&control);
    assert!(
        warnings
            .iter()
            .any(|w| w.message.contains("app.service") && w.message.contains("no unit file")),
        "{warnings:?}"
    );
    assert!(!has_search_path_warning(&warnings, "app.service", path));

    let exec = listing_mock(&[("app.service", "enabled")]).with_file(path, UNIT_FILE);
    let (row, _) = row_and_warnings(&exec, "app.service");
    assert!(!row, "no state change");
    let warnings = scan_warnings(&exec);
    assert!(
        has_search_path_warning(&warnings, "app.service", path),
        "{warnings:?}"
    );
}

/// An `/etc` storage unit the listing does not name is a mount finding of
/// its own (§ 6.5), so the search path refuses it there too: a copy under
/// `/etc/systemd/system.control` outranks it, and the scan warns naming
/// the unit and the path and records no finding. Control: the same file
/// without the copy is a finding. The listing names another unit, since
/// an empty listing returns before the storage pass; a listed unit is
/// resolved, and refused, in the first pass.
#[test]
fn the_search_path_refuses_an_etc_storage_unit_with_a_warning() {
    let path = "/etc/systemd/system.control/data.mount";
    let host = || {
        with_vendor_units(
            listing_mock(&[("other.service", "disabled")]),
            &["other.service"],
        )
        .with_dir_entry("/etc/systemd/system", "data.mount")
        .with_file(
            "/etc/systemd/system/data.mount",
            "[Mount]\nWhat=/dev/sdb1\nWhere=/data\n",
        )
    };
    let has_finding = |exec: &MockExecutor| {
        scan(exec)
            .0
            .mount_findings
            .iter()
            .any(|f| f.unit.name().as_unit_name().as_str() == "data.mount")
    };

    let control = host();
    assert!(has_finding(&control), "the control is a finding");
    assert!(!has_search_path_warning(
        &scan_warnings(&control),
        "data.mount",
        path
    ));

    let refused = host().with_file(path, "[Mount]\nWhat=/dev/sdc1\nWhere=/data\n");
    assert!(!has_finding(&refused), "no finding");
    let warnings = scan_warnings(&refused);
    assert!(
        has_search_path_warning(&warnings, "data.mount", path),
        "{warnings:?}"
    );
    assert!(
        warnings.iter().any(|w| w.message.contains(path)
            && w.message.ends_with("its mount finding is not recorded")),
        "the storage pass says what it does not record: {warnings:?}"
    );
}

/// An enabled `/etc` storage unit is a candidate as well as a file the
/// storage pass lists, so the search path refuses it in the first pass;
/// the storage pass does not warn again. Exactly one warning names the
/// path. Control: without the copy, the unit is a finding and no warning
/// names the path.
#[test]
fn an_enabled_storage_unit_refused_by_the_search_path_is_warned_once() {
    let path = "/etc/systemd/system.control/data.mount";
    let host = || {
        listing_mock(&[("data.mount", "enabled")])
            .with_dir_entry("/etc/systemd/system", "data.mount")
            .with_file(
                "/etc/systemd/system/data.mount",
                "[Mount]\nWhat=/dev/sdb1\nWhere=/data\n",
            )
    };
    let naming_path = |exec: &MockExecutor| {
        scan_warnings(exec)
            .into_iter()
            .filter(|w| w.message.contains(path))
            .count()
    };

    let control = host();
    assert!(
        scan(&control)
            .0
            .mount_findings
            .iter()
            .any(|f| f.unit.name().as_unit_name().as_str() == "data.mount"),
        "the control is a finding"
    );
    assert_eq!(naming_path(&control), 0);

    let refused = host().with_file(path, "[Mount]\nWhat=/dev/sdc1\nWhere=/data\n");
    assert!(
        scan(&refused).0.mount_findings.is_empty(),
        "no finding for the refused unit"
    );
    assert_eq!(naming_path(&refused), 1, "{:?}", scan_warnings(&refused));
}

/// T24, what the search path does not change: a drop-in with `[Install]`
/// in the type-wide directory or a dash-prefix directory, a drop-in in
/// the unit's own `.d` with neither `[Install]` nor a vendor drop-in's
/// name, a copy of an `/etc` unit in a directory that ranks below `/etc`,
/// and a file under `generator.late`, which ranks below the vendor
/// directory. Each keeps the row and its fragment, with no warning.
#[test]
fn a_file_the_search_path_does_not_load_in_place_changes_nothing() {
    let vendor = UnitFragment::Vendor {
        file: file("app.service"),
        owning_package: None,
    };
    let cases: Vec<(&str, &str, MockExecutor, UnitFragment)> = vec![
        (
            "the type-wide directory",
            "app.service",
            with_drop_in(
                vendor_app(),
                "/run/systemd/system/service.d",
                "50-install.conf",
                INSTALL_DROP_IN,
            ),
            vendor.clone(),
        ),
        (
            "a dash-prefix directory",
            "app-x.service",
            with_drop_in(
                with_vendor_units(
                    listing_mock(&[("app-x.service", "enabled")]),
                    &["app-x.service"],
                ),
                "/run/systemd/system/app-.service.d",
                "50-install.conf",
                INSTALL_DROP_IN,
            ),
            UnitFragment::Vendor {
                file: file("app-x.service"),
                owning_package: None,
            },
        ),
        (
            "a drop-in of the unit's own with no [Install]",
            "app.service",
            with_drop_in(
                vendor_app_with_vendor_drop_in(),
                "/run/systemd/system/app.service.d",
                "50-limits.conf",
                "[Service]\nLimitNOFILE=4096\n",
            ),
            vendor.clone(),
        ),
        (
            "a copy of an /etc unit under /run/systemd/system",
            "app.service",
            local_app().with_file("/run/systemd/system/app.service", UNIT_FILE),
            UnitFragment::Local {
                file: file("app.service"),
            },
        ),
        (
            "generator.late",
            "app.service",
            vendor_app().with_file("/run/systemd/generator.late/app.service", UNIT_FILE),
            vendor,
        ),
    ];
    for (label, unit, exec, fragment) in cases {
        let (section, warnings) = scan(&exec);
        assert!(warnings.is_empty(), "{label}: {warnings:?}");
        assert_eq!(fragment_of(&section, unit), &fragment, "{label}");
    }
}

/// systemd skips hidden files when it lists drop-ins, so a vendor
/// `.hidden.conf` is not a vendor drop-in name and an `/etc`
/// `.#override.conf` holding `[Install]` (an editor's lock file) is not an
/// install file. Control: the same files without the leading `.` are
/// recorded.
#[test]
fn hidden_drop_in_names_are_not_recorded() {
    let host = |vendor: &str, etc: &str| {
        let vendor_dir = "/usr/lib/systemd/system/app.service.d";
        let etc_dir = "/etc/systemd/system/app.service.d";
        vendor_app()
            .with_dir(vendor_dir, vec![vendor])
            .with_file(&format!("{vendor_dir}/{vendor}"), "[Service]\nNice=5\n")
            .with_dir(etc_dir, vec![etc])
            .with_file(&format!("{etc_dir}/{etc}"), INSTALL_DROP_IN)
    };
    let recorded = |exec: &MockExecutor| {
        let (section, _) = scan(exec);
        let change = section
            .state_changes
            .iter()
            .find(|c| c.unit().name().as_str() == "app.service")
            .expect("app.service is a state change")
            .clone();
        (change.vendor_drop_ins, change.install_drop_ins)
    };

    let (vendor, install) = recorded(&host("vendor.conf", "override.conf"));
    assert_eq!(vendor, ["vendor.conf"]);
    assert_eq!(install, ["/etc/systemd/system/app.service.d/override.conf"]);

    let (vendor, install) = recorded(&host(".hidden.conf", ".#override.conf"));
    assert!(vendor.is_empty(), "{vendor:?}");
    assert!(install.is_empty(), "{install:?}");
}

/// The vendor `real.service` whose only `[Install]` key is
/// `Alias=public.service`.
const ALIASED_UNIT: &str = "[Service]\nExecStart=/bin/true\n\n[Install]\nAlias=public.service\n";

/// A host whose `/etc/systemd/system` holds exactly the links `links`
/// places, with the vendor [`ALIASED_UNIT`], `preset` as its one preset
/// rule, and `real.service` listed `enabled`. No enable link is made for
/// it: each case plants its own.
fn alias_host(links: &[(&str, &str)], preset: &str) -> MockExecutor {
    let listing = "UNIT FILE                                  STATE           PRESET\n\
                   real.service enabled enabled\n\n1 unit files listed.\n";
    let mut exec = MockExecutor::new()
        .with_command(
            LIST_EVERY_KIND,
            ExecResult {
                stdout: listing.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file("/usr/lib/systemd/system-preset/90-default.preset", preset)
        .with_dir("/usr/lib/systemd/system", vec!["real.service"])
        .with_file("/usr/lib/systemd/system/real.service", ALIASED_UNIT)
        .with_file(
            "/opt/app/real.service",
            "[Service]\nExecStart=/opt/app/run\n",
        )
        .with_dir(
            "/etc/systemd/system",
            links.iter().map(|(name, _)| *name).collect(),
        );
    for (name, target) in links {
        exec = exec.with_link(&format!("/etc/systemd/system/{name}"), target);
    }
    owner_answer(exec, "real.service", Some("realpkg"))
}

/// An alias is attributed to the unit at the end of its whole chain, as
/// the resolver reads it, never to its first target's name. Each case
/// plants its links by hand:
///
/// - The chain `public.service -> relay.service -> real.service`, under an
///   `enable` preset: `public.service` is the alias `[Install]` names, so
///   `real.service` is installed and at its preset, with no `disable`
///   row; `relay.service` is a second alias of it, reported as a link its
///   `[Install]` does not name.
/// - A link to `/opt/app/real.service`, outside both unit directories and
///   named like the packaged unit, under a `disable` preset: it is no
///   alias of the packaged unit, which is disabled, with no row.
/// - Control: a direct link to the vendor file, under a `disable` preset,
///   is the alias, so the unit is enabled against its preset and is a row.
#[test]
fn an_alias_is_attributed_to_the_end_of_its_chain() {
    let real = "/usr/lib/systemd/system/real.service";
    let (chain, _) = scan(&alias_host(
        &[("public.service", "relay.service"), ("relay.service", real)],
        "enable real.service\n",
    ));
    assert_eq!(
        inventory_state(&chain, "real.service"),
        Some(UnitState::Enabled),
        "the chain installs real.service"
    );
    assert!(
        chain.state_changes.is_empty(),
        "an installed unit at its enable preset has no row: {:?}",
        state_change_names(&chain)
    );
    assert_eq!(
        manual_links(&chain),
        [(
            LinkRelation::Alias,
            "relay.service",
            "real.service",
            ManualLinkCause::NotInInstall
        )]
    );

    let (outside, _) = scan(&alias_host(
        &[("public.service", "/opt/app/real.service")],
        "disable real.service\n",
    ));
    assert_eq!(
        inventory_state(&outside, "real.service"),
        Some(UnitState::Disabled),
        "a link outside the unit directories is no alias of real.service"
    );
    assert!(
        outside.state_changes.is_empty(),
        "{:?}",
        state_change_names(&outside)
    );
    assert!(
        outside.manual_links.is_empty(),
        "{:?}",
        outside.manual_links
    );

    let (direct, _) = scan(&alias_host(
        &[("public.service", real)],
        "disable real.service\n",
    ));
    assert_eq!(
        inventory_state(&direct, "real.service"),
        Some(UnitState::Enabled),
        "control: the direct alias installs real.service"
    );
    assert_eq!(state_change_names(&direct), ["real.service"]);
    assert_eq!(direct.state_changes[0].current_state(), UnitState::Enabled);
    assert!(direct.manual_links.is_empty(), "{:?}", direct.manual_links);
}

/// An enabled `/etc` storage unit whose file cannot be read is a
/// candidate and a file the storage pass lists; the read failure is
/// listed once in the degraded reason. Control: the readable file is a
/// finding and the scan is not degraded.
#[test]
fn an_unreadable_enabled_storage_unit_is_listed_once() {
    const PATH: &str = "/etc/systemd/system/data.mount";
    let host = || {
        listing_mock(&[("data.mount", "enabled")])
            .with_dir_entry("/etc/systemd/system", "data.mount")
    };
    let (section, _) = scan(&host().with_file(PATH, "[Mount]\nWhat=/dev/sdb1\nWhere=/data\n"));
    assert_eq!(section.mount_findings.len(), 1);

    let unreadable = host().with_file_error(PATH, std::io::ErrorKind::PermissionDenied);
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &unreadable,
        rpm_state: None,
        baseline_data: None,
    };
    match UnitsInspector::new().inspect(&ctx, &NullProgress) {
        Err(InspectorError::Degraded { reason, .. }) => {
            // Each failure reads `<path>: <error>`; the mock's error text
            // repeats the path, so count the `<path>: ` entries.
            assert_eq!(reason.matches(&format!("{PATH}: ")).count(), 1, "{reason}");
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
        stdout: listing.clone(),
        exit_code: 0,
        ..Default::default()
    };
    let exec = MockExecutor::new()
        .with_command(LIST_EVERY_KIND, answer.clone())
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file("/usr/lib/systemd/system-preset/90-default.preset", preset)
        .with_dir("/etc/systemd/system", vec![]);
    with_enable_links(exec, &listing)
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
        // Read from the unit file, which has no `[Mount]` section here. A
        // mask with no unit file has no options to read.
        assert_eq!(finding.mount_point, None);
        let read = *finding.unit.fragment() != AdminStorageFragment::MaskOnly;
        assert_eq!(finding.nofail, read.then_some(false));
    }
    assert!(warnings.is_empty(), "routed, not only warned: {warnings:?}");
}

/// A timer is a unit like any other (§ 5, § 8): a vendor timer enabled
/// against its preset is a state change, and so is a local one enabled,
/// with a `Local` fragment. The scheduled section gives neither a control.
#[test]
fn a_local_timer_is_a_state_change_like_any_unit() {
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

    assert_eq!(
        state_change_names(&section),
        vec!["dnf-automatic.timer", "site-backup.timer"]
    );
    let local = section
        .state_changes
        .iter()
        .find(|c| c.unit().name().as_str() == "site-backup.timer")
        .expect("the local timer's state change");
    assert!(
        matches!(
            local.unit(),
            CarriedUnit::Ordinary(o) if matches!(o.fragment(), UnitFragment::Local { .. })
        ),
        "{local:?}"
    );
    assert!(section.drop_ins.is_empty(), "{:?}", section.drop_ins);
}

/// An `/etc` copy over a vendor timer is a full shadow in `drop_ins`, as
/// for every ordinary kind (§ 7.7), and the timer a `Shadowed` state
/// change. Control: the same copy with no vendor file is no shadow row.
#[test]
fn a_timers_full_shadow_is_a_drop_in_row() {
    let shadowed = with_etc_units(
        kinds_mock(&[("dnf-automatic.timer", "enabled")], "disable *\n"),
        vec!["dnf-automatic.timer"],
    )
    .with_file("/usr/lib/systemd/system/dnf-automatic.timer", UNIT_FILE)
    .with_file("/etc/systemd/system/dnf-automatic.timer", UNIT_FILE);
    let (section, _) = scan(&shadowed);
    assert_eq!(state_change_names(&section), vec!["dnf-automatic.timer"]);
    assert!(
        matches!(
            section.state_changes[0].unit(),
            CarriedUnit::Ordinary(o) if matches!(o.fragment(), UnitFragment::Shadowed { .. })
        ),
        "{:?}",
        section.state_changes[0]
    );
    let shadows: Vec<(&str, &str, Option<ShadowType>)> = section
        .drop_ins
        .iter()
        .map(|d| (d.unit.as_str(), d.path.as_str(), d.shadow_type.clone()))
        .collect();
    assert_eq!(
        shadows,
        vec![(
            "dnf-automatic.timer",
            "/etc/systemd/system/dnf-automatic.timer",
            Some(ShadowType::FullShadow)
        )]
    );

    let local = with_etc_units(
        kinds_mock(&[("dnf-automatic.timer", "enabled")], "disable *\n"),
        vec!["dnf-automatic.timer"],
    )
    .with_file("/etc/systemd/system/dnf-automatic.timer", UNIT_FILE);
    let (section, _) = scan(&local);
    assert!(section.drop_ins.is_empty(), "{:?}", section.drop_ins);
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
    let exec = exec
        .with_dir("/etc", vec!["systemd"])
        .with_dir("/etc/systemd", vec!["system"]);
    // Added to the listing, so the enable links already there stay.
    entries
        .into_iter()
        .fold(exec, |exec, entry| exec.with_dir_entry(ETC_UNITS, entry))
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

/// A full shadow that opens with a comment: not empty, so not a mask,
/// and wanted by `multi-user.target` as [`UNIT_FILE`] is.
const COMMENT_SHADOW: &str = "# replaced\n\n[Install]\nWantedBy=multi-user.target\n";

fn with_control(exec: MockExecutor) -> MockExecutor {
    let exec = exec
        .with_file(CONTROL_PATH, COMMENT_SHADOW)
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
    assert_eq!(row.content, COMMENT_SHADOW, "{case}");
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
/// socket and a timer in the same layout are full shadow rows (the
/// positive controls): a timer is an ordinary kind (§ 8).
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
            section
                .drop_ins
                .iter()
                .any(|d| d.path == timer_path && d.shadow_type == Some(ShadowType::FullShadow)),
            "{unit}: the timer shadow is a row: {:?}",
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
/// over the executor's size cap (`InvalidData`), does not degrade the
/// scan. Its `[Install]` cannot be read, so the unit is an
/// `UndeterminedInstall` record listing the shadow, with a warning naming
/// it, and no state change. Its content cannot be carried, so it is no
/// drop-in row: an empty drop-in would stage an empty file, which systemd
/// reads as a mask. The config entry the config inspector records for it,
/// a `ContentNotCaptured` advisory, is its one row, as for any file under
/// `/etc` that cannot be read as text. The readable twin is a full shadow
/// row.
#[test]
fn full_shadow_that_is_not_text_is_undetermined_and_does_not_degrade() {
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
    let (section, _) = scan(&exec);
    let warnings = scan_warnings(&exec);
    let latin1 = "/etc/systemd/system/latin1.service";
    assert!(
        has_unreadable_install_warning(&warnings, "latin1.service", latin1),
        "{warnings:?}"
    );
    assert_eq!(warnings.len(), 1, "{warnings:?}");

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

    assert_eq!(state_change_names(&section), ["twin.service"]);
    assert_eq!(
        undetermined(&section),
        [("latin1.service", InstallUnknown::Unreadable, vec![latin1])]
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
    Option<bool>,
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

/// The `[Install]` section of an enabled unit in these tests, which
/// [`with_enable_link`] links.
const WANTED: &str = "\n[Install]\nWantedBy=multi-user.target\n";

fn mount_unit(section: &str, lines: &str) -> String {
    format!("[Unit]\nDescription=admin mount\n\n[{section}]\n{MOUNT_SOURCE}{lines}")
}

/// § 6.5: every source of a mount finding, each with `mount_point` and
/// `nofail` read from the file systemd loads. `nofail` is `None` where no
/// `Options=` was read: the file that is not text, the mask with no unit
/// file, and the automount, which has no `Options=`.
///
/// - Regular, non-empty storage files under `/etc/systemd/system`, whatever
///   their state: static (not listed), disabled, disabled against an
///   `enable` preset (skipped by the candidate pass, recorded by the
///   directory pass, no state change), a full copy over a vendor
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
    let preset = "enable opt-on.mount\nenable srv-shadow.mount\nenable tmp.mount\n\
                  enable srv-pre.mount\ndisable *\n";
    let exec = kinds_mock(
        &[
            ("srv-off.mount", "disabled"),
            ("srv-pre.mount", "disabled"),
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
        ("srv-pre.mount", mount_unit("Mount", "Where=/srv/pre\n")),
        (
            "data.swap",
            mount_unit("Swap", "Options=defaults,nofail\r\n"),
        ),
        (
            "srv-shadow.mount",
            mount_unit("Mount", &format!("Where=/srv/shadow \r\n{WANTED}")),
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
        (
            "opt-on.mount",
            mount_unit("Mount", &format!("Where=/opt/on\n{WANTED}")),
        ),
        ("opt-off.mount", mount_unit("Mount", "Where=/opt/off\n")),
        (
            "opt-masked.mount",
            mount_unit("Mount", "Where=/opt/masked\nOptions=nofail\n"),
        ),
        (
            "tmp.mount",
            mount_unit("Mount", &format!("Where=/tmp\n{WANTED}")),
        ),
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
            Some(true),
        ),
        (
            "gone.mount".into(),
            AdminStorageFragment::MaskOnly,
            masked,
            None,
            None,
        ),
        (
            "latin1.mount".into(),
            AdminStorageFragment::Local,
            None,
            None,
            None,
        ),
        (
            "mnt-auto.automount".into(),
            AdminStorageFragment::Local,
            None,
            path("/mnt/auto"),
            None,
        ),
        (
            "opt-masked.mount".into(),
            AdminStorageFragment::UnownedVendor,
            masked,
            path("/opt/masked"),
            Some(true),
        ),
        (
            "opt-on.mount".into(),
            AdminStorageFragment::UnownedVendor,
            enabled_at_preset,
            path("/opt/on"),
            Some(false),
        ),
        (
            "relative.mount".into(),
            AdminStorageFragment::Local,
            None,
            None,
            Some(true),
        ),
        (
            "srv-off.mount".into(),
            AdminStorageFragment::Local,
            None,
            path("/srv/off"),
            Some(false),
        ),
        // Disabled against an `enable` preset: the candidate pass skips an
        // advisory unit, so the directory pass records it, and a disabled
        // unit has no state change on its finding.
        (
            "srv-pre.mount".into(),
            AdminStorageFragment::Local,
            None,
            path("/srv/pre"),
            Some(false),
        ),
        (
            "srv-shadow.mount".into(),
            AdminStorageFragment::Shadowed {
                owning_package: Some("srv-pkg".into()),
            },
            enabled_at_preset,
            path("/srv/shadow"),
            Some(false),
        ),
        (
            "srv-static.mount".into(),
            AdminStorageFragment::Local,
            None,
            path("/srv/static"),
            Some(true),
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

/// A swap unit's `nofail` is recorded only when its unit file was read:
/// `Some(true)` with `nofail` in `Options=`, `Some(false)` without it (the
/// controls), and `None` for a file that is not text or is over the size
/// cap (`InvalidData`), which is still a finding and does not degrade the
/// scan.
#[test]
fn a_swap_finding_records_nofail_only_when_its_file_was_read() {
    let files = [
        ("with.swap", mount_unit("Swap", "Options=defaults,nofail\n")),
        ("without.swap", mount_unit("Swap", "Options=defaults\n")),
    ];
    let mut exec = with_etc_units(
        quiet_mock(),
        vec!["with.swap", "without.swap", "latin1.swap"],
    );
    for (name, content) in &files {
        exec = exec.with_file(&format!("{ETC_UNITS}/{name}"), content);
    }
    let exec = exec.with_file_error(
        &format!("{ETC_UNITS}/latin1.swap"),
        std::io::ErrorKind::InvalidData,
    );
    let (section, warnings) = scan(&exec);
    assert!(warnings.is_empty(), "{warnings:?}");
    let mut nofail: Vec<(&str, Option<bool>)> = section
        .mount_findings
        .iter()
        .map(|f| (f.unit.name().as_unit_name().as_str(), f.nofail))
        .collect();
    nofail.sort();
    assert_eq!(
        nofail,
        [
            ("latin1.swap", None),
            ("with.swap", Some(true)),
            ("without.swap", Some(false)),
        ]
    );
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

// ── Install state from the links (§ 7.3, T22, T24's reset states) ──

/// The administrator's unit directory, for the link tests below.
const ETC: &str = "/etc/systemd/system";
/// The vendor unit directory.
const VENDOR: &str = "/usr/lib/systemd/system";
/// A unit section wanted by `multi-user.target`.
const WANTS_MULTI_USER: &str =
    "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\n";

/// A host whose listing names `units` (name, state) under `preset`, with
/// no link anywhere until a test adds one, so systemctl's word and the
/// links can disagree as they do on a real host.
fn unlinked_host(units: &[(&str, &str)], preset: &str) -> MockExecutor {
    let mut listing =
        String::from("UNIT FILE                                  STATE           PRESET\n");
    for (unit, state) in units {
        listing.push_str(&format!("{unit} {state} disabled\n"));
    }
    listing.push_str(&format!("\n{} unit files listed.\n", units.len()));
    MockExecutor::new()
        .with_command(
            LIST_EVERY_KIND,
            ExecResult {
                stdout: listing,
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file("/usr/lib/systemd/system-preset/90-default.preset", preset)
        .with_dir(ETC, vec![])
}

/// `exec` with a link to `unit` in the `<parent><suffix>` directory under
/// `root`, added to what that directory already lists.
fn with_link_in(exec: MockExecutor, root: &str, parent_dir: &str, unit: &str) -> MockExecutor {
    let dir = format!("{root}/{parent_dir}");
    exec.with_dir_entry(root, parent_dir)
        .with_dir_entry(&dir, unit)
        .with_link(&format!("{dir}/{unit}"), &format!("{VENDOR}/{unit}"))
}

/// The inventory entry for `unit`: its state and preset.
fn entry_of(section: &UnitSection, unit: &str) -> Option<(UnitState, Option<PresetDefault>)> {
    section
        .inventory
        .iter()
        .find(|e| e.unit.as_str() == unit)
        .map(|e| (e.state, e.preset))
}

/// The state change for `unit`, as its state and preset.
fn change_of(section: &UnitSection, unit: &str) -> Option<(UnitState, Option<PresetDefault>)> {
    section
        .state_changes
        .iter()
        .find(|c| c.unit().name().as_str() == unit)
        .map(|c| (c.current_state(), c.default_state))
}

/// T22's state assertions. `backup.service` wants `multi-user.target` and
/// systemctl reports it `enabled` in every case; the links decide.
#[test]
fn t22_the_install_state_is_read_from_the_links() {
    use PresetDefault::{Disable, Enable};
    use UnitState::{Disabled, Enabled};
    let two_targets = "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target graphical.target\n";
    type Links<'a> = &'a [(&'a str, &'a str)];
    // (case, unit file, links as (root, directory), preset, inventory, state change)
    type Case<'a> = (
        &'a str,
        &'a str,
        Links<'a>,
        &'a str,
        Option<(UnitState, Option<PresetDefault>)>,
        Option<(UnitState, Option<PresetDefault>)>,
    );
    let cases: &[Case] = &[
        (
            "only a rescue.target.wants link, disable preset",
            WANTS_MULTI_USER,
            &[(ETC, "rescue.target.wants")],
            "disable *\n",
            Some((Disabled, Some(Disable))),
            None,
        ),
        (
            "only a rescue.target.wants link, enable preset",
            WANTS_MULTI_USER,
            &[(ETC, "rescue.target.wants")],
            "enable backup.service\n",
            Some((Disabled, Some(Enable))),
            Some((Disabled, Some(Enable))),
        ),
        (
            "both links",
            WANTS_MULTI_USER,
            &[
                (ETC, "rescue.target.wants"),
                (ETC, "multi-user.target.wants"),
            ],
            "disable *\n",
            Some((Enabled, Some(Disable))),
            Some((Enabled, Some(Disable))),
        ),
        (
            "two WantedBy= values and one link",
            two_targets,
            &[(ETC, "multi-user.target.wants")],
            "disable *\n",
            None,
            None,
        ),
        (
            "two WantedBy= values, the second link shipped under /usr/lib",
            two_targets,
            &[
                (ETC, "multi-user.target.wants"),
                (VENDOR, "graphical.target.wants"),
            ],
            "disable *\n",
            Some((Enabled, Some(Disable))),
            Some((Enabled, Some(Disable))),
        ),
        (
            "only vendor links",
            WANTS_MULTI_USER,
            &[(VENDOR, "multi-user.target.wants")],
            "disable *\n",
            Some((Disabled, Some(Disable))),
            None,
        ),
        (
            "only vendor links, enable preset",
            WANTS_MULTI_USER,
            &[(VENDOR, "multi-user.target.wants")],
            "enable backup.service\n",
            Some((Disabled, Some(Enable))),
            Some((Disabled, Some(Enable))),
        ),
        (
            "no [Install] and a hand-made link",
            "[Service]\nExecStart=/bin/true\n",
            &[(ETC, "multi-user.target.wants")],
            "disable *\n",
            None,
            None,
        ),
        (
            "control: the expected link alone",
            WANTS_MULTI_USER,
            &[(ETC, "multi-user.target.wants")],
            "disable *\n",
            Some((Enabled, Some(Disable))),
            Some((Enabled, Some(Disable))),
        ),
    ];
    for (case, unit_file, links, preset, inventory, change) in cases {
        let mut exec = unlinked_host(&[("backup.service", "enabled")], preset)
            .with_file(&format!("{VENDOR}/backup.service"), unit_file);
        for (root, dir) in *links {
            exec = with_link_in(exec, root, dir, "backup.service");
        }
        let (section, warnings) = scan(&exec);
        assert!(warnings.is_empty(), "{case}: {warnings:?}");
        assert_eq!(entry_of(&section, "backup.service"), *inventory, "{case}");
        assert_eq!(change_of(&section, "backup.service"), *change, "{case}");
        if inventory.is_none() {
            assert!(
                section.inventory.is_empty(),
                "{case}: {:?}",
                section.inventory
            );
        }
    }
}

/// T22: a unit whose `[Install]` holds only `Also=` has no state of its
/// own, so no state change and no inventory entry, though a link names
/// it. Control: the same `indirect` unit with a `WantedBy=` its link
/// matches is read from that link and is `Enabled`.
#[test]
fn t22_an_also_only_unit_has_no_install_state() {
    let host = |unit_file: &str| {
        with_link_in(
            unlinked_host(&[("kcm.service", "indirect")], "disable *\n")
                .with_file(&format!("{VENDOR}/kcm.service"), unit_file),
            ETC,
            "multi-user.target.wants",
            "kcm.service",
        )
    };
    let (control, _) = scan(&host(WANTS_MULTI_USER));
    assert_eq!(
        entry_of(&control, "kcm.service"),
        Some((UnitState::Enabled, Some(PresetDefault::Disable)))
    );

    let (section, warnings) = scan(&host(
        "[Service]\nExecStart=/bin/true\n\n[Install]\nAlso=kcm.socket\n",
    ));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(entry_of(&section, "kcm.service"), None);
    assert!(
        section.state_changes.is_empty(),
        "{:?}",
        section.state_changes
    );
}

/// T22: `remote-cryptsetup.target`, which a package also links under
/// `/usr/lib`, is `Disabled` against its `enable` preset on a stock host:
/// an inventory entry and no state change. Control: another unit in the
/// same state is a `Disabled` state change.
#[test]
fn t22_stock_state_read_from_the_links_is_no_state_change() {
    let target = "[Unit]\nDescription=x\n\n[Install]\nWantedBy=remote-fs.target\n";
    let host = |unit: &str| {
        with_link_in(
            unlinked_host(&[(unit, "disabled")], &format!("enable {unit}\n"))
                .with_file(&format!("{VENDOR}/{unit}"), target),
            VENDOR,
            "remote-fs.target.wants",
            unit,
        )
    };
    let (section, warnings) = scan(&host("remote-cryptsetup.target"));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        entry_of(&section, "remote-cryptsetup.target"),
        Some((UnitState::Disabled, Some(PresetDefault::Enable)))
    );
    assert!(
        section.state_changes.is_empty(),
        "{:?}",
        section.state_changes
    );

    let (control, _) = scan(&host("remote-other.target"));
    assert_eq!(
        change_of(&control, "remote-other.target"),
        Some((UnitState::Disabled, Some(PresetDefault::Enable)))
    );
}

/// § 7.3's `Undetermined` rows, for a vendor unit with `UpheldBy=` under
/// an `enable` preset: with no link it is `Disabled`, a state change;
/// with a link under `/etc` it has no install state; assigning `Also=`
/// with no link it has none either.
#[test]
fn undetermined_install_states_follow_the_links() {
    let upheld = "[Service]\nExecStart=/bin/true\n\n[Install]\nUpheldBy=multi-user.target\n";
    let upheld_also = "[Service]\nExecStart=/bin/true\n\n[Install]\nUpheldBy=multi-user.target\nAlso=helper.service\n";
    let host = |unit_file: &str| {
        unlinked_host(&[("watch.service", "enabled")], "enable watch.service\n")
            .with_file(&format!("{VENDOR}/watch.service"), unit_file)
    };

    let (no_link, warnings) = scan(&host(upheld));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        change_of(&no_link, "watch.service"),
        Some((UnitState::Disabled, Some(PresetDefault::Enable)))
    );
    assert_eq!(
        entry_of(&no_link, "watch.service"),
        Some((UnitState::Disabled, Some(PresetDefault::Enable)))
    );

    let (linked, _) = scan(&with_link_in(
        host(upheld),
        ETC,
        "multi-user.target.upholds",
        "watch.service",
    ));
    assert_eq!(entry_of(&linked, "watch.service"), None);
    assert!(
        linked.state_changes.is_empty(),
        "{:?}",
        linked.state_changes
    );

    let (also, _) = scan(&host(upheld_also));
    assert_eq!(entry_of(&also, "watch.service"), None);
    assert!(also.state_changes.is_empty(), "{:?}", also.state_changes);
}

/// T24, on the expected links: a vendor drop-in holding
/// `WantedBy=multi-user.target` in the type-wide `service.d` or the
/// dash-prefix `a-.service.d` makes no expected link, so `a-b.service`,
/// with no `[Install]` of its own and the `multi-user.target.wants` link,
/// has no install state. Control: the same drop-in in its own `.d` makes
/// that link expected, and the unit is `Enabled`.
#[test]
fn t24_a_type_wide_or_dash_prefix_drop_in_makes_no_expected_link() {
    let host = |dir: &str| {
        let exec = unlinked_host(&[("a-b.service", "enabled")], "disable *\n").with_file(
            &format!("{VENDOR}/a-b.service"),
            "[Service]\nExecStart=/bin/true\n",
        );
        let exec = with_drop_in(
            exec,
            &format!("{VENDOR}/{dir}"),
            "10-install.conf",
            INSTALL_DROP_IN,
        );
        with_link_in(exec, ETC, "multi-user.target.wants", "a-b.service")
    };
    let (control, _) = scan(&host("a-b.service.d"));
    assert_eq!(
        entry_of(&control, "a-b.service"),
        Some((UnitState::Enabled, Some(PresetDefault::Disable)))
    );
    for dir in ["service.d", "a-.service.d"] {
        let (section, warnings) = scan(&host(dir));
        assert!(warnings.is_empty(), "{dir}: {warnings:?}");
        assert_eq!(entry_of(&section, "a-b.service"), None, "{dir}");
        assert!(section.state_changes.is_empty(), "{dir}");
    }
}

/// T24's reset install states, each with `WantedBy=multi-user.target` in
/// the local unit `app.service`:
/// - an `/etc` drop-in holding only `WantedBy=` leaves nothing expected,
///   so with no link the unit has no install state and no inventory
///   entry, asserted after the drop-in is shown collected. Control:
///   without the drop-in, the link makes it `Enabled`.
/// - `WantedBy=` then `WantedBy=graphical.target` in one drop-in expects
///   `graphical.target.wants` only: a link there is `Enabled`, and the
///   `multi-user.target.wants` link alone is `Disabled`.
/// - with `Alias=x-alias.service` in the unit, an empty `Alias=` drops the
///   alias from the expected links, so the wants link alone is `Enabled`;
///   without the reset the missing alias leaves no install state.
#[test]
fn t24_reset_install_states() {
    let drop_in_dir = format!("{ETC}/app.service.d");
    let local = |unit_file: &str| {
        unlinked_host(&[("app.service", "enabled")], "disable *\n")
            .with_dir_entry(ETC, "app.service")
            .with_file(&format!("{ETC}/app.service"), unit_file)
    };
    let with_reset = |exec: MockExecutor, text: &str| {
        with_drop_in(
            exec.with_dir_entry(ETC, "app.service.d"),
            &drop_in_dir,
            "10-reset.conf",
            text,
        )
    };
    let enabled = Some((UnitState::Enabled, Some(PresetDefault::Disable)));
    let disabled = Some((UnitState::Disabled, Some(PresetDefault::Disable)));

    let (control, _) = scan(&with_link_in(
        local(WANTS_MULTI_USER),
        ETC,
        "multi-user.target.wants",
        "app.service",
    ));
    assert_eq!(entry_of(&control, "app.service"), enabled);

    let (reset, warnings) = scan(&with_reset(
        local(WANTS_MULTI_USER),
        "[Install]\nWantedBy=\n",
    ));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(
        drop_in_paths(&reset).contains(&format!("{drop_in_dir}/10-reset.conf").as_str()),
        "the reset drop-in is collected: {:?}",
        drop_in_paths(&reset)
    );
    assert_eq!(entry_of(&reset, "app.service"), None);
    assert!(reset.state_changes.is_empty(), "{:?}", reset.state_changes);

    let reset_then_add = "[Install]\nWantedBy=\nWantedBy=graphical.target\n";
    for (dir, expected) in [
        ("graphical.target.wants", enabled),
        ("multi-user.target.wants", disabled),
    ] {
        let exec = with_reset(local(WANTS_MULTI_USER), reset_then_add);
        let (section, _) = scan(&with_link_in(exec, ETC, dir, "app.service"));
        assert_eq!(entry_of(&section, "app.service"), expected, "{dir}");
    }

    let aliased = "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\nAlias=x-alias.service\n";
    let wants_link =
        |exec: MockExecutor| with_link_in(exec, ETC, "multi-user.target.wants", "app.service");
    let (no_reset, _) = scan(&wants_link(local(aliased)));
    assert_eq!(entry_of(&no_reset, "app.service"), None);
    let (alias_reset, _) = scan(&wants_link(with_reset(
        local(aliased),
        "[Install]\nAlias=\n",
    )));
    assert_eq!(entry_of(&alias_reset, "app.service"), enabled);
}

/// T39's warning half through the scan: a preset line whose verb is
/// neither `enable` nor `disable`, the first match for `app.service`,
/// leaves it in the inventory with no preset and no state change, and
/// one warning names the line, its file and the unit. Control: without
/// that line the `disable` rule after it gives an `Enabled` state change.
#[test]
fn t39_an_unknown_preset_line_stops_the_lookup_with_a_warning() {
    let host = |preset: &str| {
        with_link_in(
            unlinked_host(&[("app.service", "enabled")], preset)
                .with_file(&format!("{VENDOR}/app.service"), WANTS_MULTI_USER),
            ETC,
            "multi-user.target.wants",
            "app.service",
        )
    };
    let (control, _) = scan(&host("disable app.service\n"));
    assert_eq!(
        change_of(&control, "app.service"),
        Some((UnitState::Enabled, Some(PresetDefault::Disable)))
    );

    let exec = host("# a comment\nfrobnicate app.service\ndisable app.service\n");
    let (section, _) = scan(&exec);
    assert_eq!(
        entry_of(&section, "app.service"),
        Some((UnitState::Enabled, None))
    );
    assert!(
        section.state_changes.is_empty(),
        "{:?}",
        section.state_changes
    );
    let warnings = scan_warnings(&exec);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let warning = &warnings[0];
    let file = "/usr/lib/systemd/system-preset/90-default.preset";
    assert_eq!(warning.extra["file"], serde_json::json!(file));
    assert_eq!(warning.extra["line"], serde_json::json!(2));
    assert_eq!(warning.extra["units"], serde_json::json!(["app.service"]));
    assert!(
        warning.message.contains(file)
            && warning.message.contains("line 2")
            && warning.message.contains("app.service")
            && warning.message.contains("not compared with a preset"),
        "{}",
        warning.message
    );
}

/// § 7.3's `Undetermined` row with "some link under `/etc`": a vendor
/// unit with `WantedBy=dev-foo.device`, a value the model does not admit,
/// is `Undetermined`, and its link sits in a directory whose parent does
/// not parse as a unit name. That link still counts, so under an `enable`
/// preset the unit has no install state: no entry and no state change.
/// Control: the same unit with no link is a `Disabled` state change.
#[test]
fn a_link_under_a_parent_that_does_not_parse_is_still_a_link() {
    let host = || {
        unlinked_host(&[("guest.service", "enabled")], "enable guest.service\n").with_file(
            &format!("{VENDOR}/guest.service"),
            "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=dev-foo.device\n",
        )
    };
    let (control, _) = scan(&host());
    assert_eq!(
        change_of(&control, "guest.service"),
        Some((UnitState::Disabled, Some(PresetDefault::Enable)))
    );

    let (section, _) = scan(&with_link_in(
        host(),
        ETC,
        "dev-foo.device.wants",
        "guest.service",
    ));
    assert_eq!(entry_of(&section, "guest.service"), None);
    assert!(
        section.state_changes.is_empty(),
        "{:?}",
        section.state_changes
    );
}

/// The section and degradation reason of a scan of `exec` that degrades.
fn degraded(exec: &MockExecutor) -> (UnitSection, String) {
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: exec,
        rpm_state: None,
        baseline_data: None,
    };
    match UnitsInspector::new().inspect(&ctx, &NullProgress) {
        Err(InspectorError::Degraded { reason, partial }) => {
            let SectionData::Units(section) = partial.section else {
                panic!("expected the units section");
            };
            (section, reason)
        }
        other => panic!("the scan degrades: {other:?}"),
    }
}

/// A link directory under `/etc` that cannot be read hides links, so the
/// degraded scan derives no install state for any unit that is not
/// masked: the unit linked there has no entry and no `disable`, though
/// the links read say it has none. A mask is still recorded. Control: the
/// readable directory makes the unit `Enabled` at its preset.
#[test]
fn an_unreadable_link_directory_under_etc_withholds_install_states() {
    let host = || {
        let exec = unlinked_host(
            &[("app.service", "enabled"), ("old.service", "masked")],
            "enable app.service\ndisable *\n",
        )
        .with_file(&format!("{VENDOR}/app.service"), WANTS_MULTI_USER)
        .with_file(&format!("{VENDOR}/old.service"), WANTS_MULTI_USER);
        with_mask(exec, "old.service", MaskForm::DevNullLink).with_dir_entry(ETC, "old.service")
    };
    let (control, warnings) = scan(&with_link_in(
        host(),
        ETC,
        "multi-user.target.wants",
        "app.service",
    ));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        entry_of(&control, "app.service"),
        Some((UnitState::Enabled, Some(PresetDefault::Enable)))
    );

    for kind in [
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::Other,
    ] {
        let exec = host()
            .with_dir_entry(ETC, "multi-user.target.wants")
            .with_dir_error(&format!("{ETC}/multi-user.target.wants"), kind);
        let (section, reason) = degraded(&exec);
        assert!(
            reason.contains("/etc/systemd/system/multi-user.target.wants"),
            "{kind:?}: {reason}"
        );
        assert_eq!(entry_of(&section, "app.service"), None, "{kind:?}");
        assert_eq!(change_of(&section, "app.service"), None, "{kind:?}");
        assert_eq!(
            change_of(&section, "old.service"),
            Some((UnitState::Masked, Some(PresetDefault::Disable))),
            "{kind:?}"
        );
    }
}

/// An unknown preset line names, in its one warning, only the units whose
/// result its preset decides: here two vendor units, one line. An enabled
/// local unit and a mask the same line matches keep their state changes,
/// which do not depend on a preset, and are not named.
#[test]
fn an_unknown_preset_line_names_only_the_units_it_decides() {
    let mut exec = unlinked_host(
        &[
            ("a.service", "enabled"),
            ("b.service", "enabled"),
            ("local.service", "enabled"),
            ("gone.service", "masked"),
        ],
        "frobnicate *\ndisable *\n",
    );
    for unit in ["a.service", "b.service", "gone.service"] {
        exec = exec.with_file(&format!("{VENDOR}/{unit}"), WANTS_MULTI_USER);
    }
    exec = exec
        .with_dir_entry(ETC, "local.service")
        .with_file(&format!("{ETC}/local.service"), WANTS_MULTI_USER);
    exec =
        with_mask(exec, "gone.service", MaskForm::DevNullLink).with_dir_entry(ETC, "gone.service");
    for unit in ["a.service", "b.service", "local.service"] {
        exec = with_link_in(exec, ETC, "multi-user.target.wants", unit);
    }

    let (section, _) = scan(&exec);
    for unit in ["a.service", "b.service"] {
        assert_eq!(
            entry_of(&section, unit),
            Some((UnitState::Enabled, None)),
            "{unit}"
        );
        assert_eq!(change_of(&section, unit), None, "{unit}");
    }
    assert_eq!(
        change_of(&section, "local.service"),
        Some((UnitState::Enabled, None))
    );
    assert_eq!(
        change_of(&section, "gone.service"),
        Some((UnitState::Masked, None))
    );
    let warnings = scan_warnings(&exec);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0].extra["line"], serde_json::json!(1));
    assert_eq!(
        warnings[0].extra["units"],
        serde_json::json!(["a.service", "b.service"])
    );
}

/// A preset line starting with `;` is a comment, as one starting with `#`
/// is: `;enable app.service` before `disable app.service` changes
/// nothing, and no warning is raised. Control: the same line without the
/// `;` decides the preset.
#[test]
fn a_semicolon_preset_line_is_a_comment() {
    let host = |preset: &str| {
        with_link_in(
            unlinked_host(&[("app.service", "enabled")], preset)
                .with_file(&format!("{VENDOR}/app.service"), WANTS_MULTI_USER),
            ETC,
            "multi-user.target.wants",
            "app.service",
        )
    };
    let (control, _) = scan(&host("enable app.service\ndisable app.service\n"));
    assert_eq!(change_of(&control, "app.service"), None);

    let exec = host(";enable app.service\ndisable app.service\n");
    let (section, _) = scan(&exec);
    assert_eq!(
        change_of(&section, "app.service"),
        Some((UnitState::Enabled, Some(PresetDefault::Disable)))
    );
    assert!(scan_warnings(&exec).is_empty());
}

// ── Hand-made links and undetermined installs (§ 6.6, § 7.3, T22 to T24, T40) ──

/// Each hand-made link the scan reports: relation, parent, unit and cause.
fn manual_links(section: &UnitSection) -> Vec<(LinkRelation, &str, &str, ManualLinkCause)> {
    section
        .manual_links
        .iter()
        .map(|l| (l.relation, l.parent.as_str(), l.unit.as_str(), l.cause))
        .collect()
}

/// Each undetermined install record: unit, reason and install files.
fn undetermined(section: &UnitSection) -> Vec<(&str, InstallUnknown, Vec<&str>)> {
    section
        .undetermined_installs
        .iter()
        .map(|r| {
            (
                r.unit.as_str(),
                r.reason,
                r.install_files.iter().map(String::as_str).collect(),
            )
        })
        .collect()
}

/// T22's link half, on the T22 hosts: one `NotInInstall` link for the
/// `rescue.target.wants/` link, the extra link alone when both links
/// exist, `PartialInstall` for two `WantedBy=` values and one link, and
/// nothing for a link shipped under `/usr/lib`. Control: the expected link
/// alone reports nothing.
#[test]
fn t22_link_findings() {
    use LinkRelation::Wants;
    use ManualLinkCause::{NotInInstall, PartialInstall};
    let two_targets = "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target graphical.target\n";
    type Found<'a> = Vec<(LinkRelation, &'a str, &'a str, ManualLinkCause)>;
    // (case, unit file, links as (root, directory), links found)
    type Case<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)], Found<'a>);
    let cases: [Case; 5] = [
        (
            "only a rescue.target.wants link",
            WANTS_MULTI_USER,
            &[(ETC, "rescue.target.wants")],
            vec![(Wants, "rescue.target", "backup.service", NotInInstall)],
        ),
        (
            "both links",
            WANTS_MULTI_USER,
            &[
                (ETC, "rescue.target.wants"),
                (ETC, "multi-user.target.wants"),
            ],
            vec![(Wants, "rescue.target", "backup.service", NotInInstall)],
        ),
        (
            "two WantedBy= values and one link",
            two_targets,
            &[(ETC, "multi-user.target.wants")],
            vec![(Wants, "multi-user.target", "backup.service", PartialInstall)],
        ),
        (
            "the second link shipped under /usr/lib",
            two_targets,
            &[
                (ETC, "multi-user.target.wants"),
                (VENDOR, "graphical.target.wants"),
            ],
            vec![],
        ),
        (
            "control: the expected link alone",
            WANTS_MULTI_USER,
            &[(ETC, "multi-user.target.wants")],
            vec![],
        ),
    ];
    for (case, unit_file, links, expected) in cases {
        let mut exec = unlinked_host(&[("backup.service", "enabled")], "disable *\n")
            .with_file(&format!("{VENDOR}/backup.service"), unit_file);
        for (root, dir) in links {
            exec = with_link_in(exec, root, dir, "backup.service");
        }
        let (section, _) = scan(&exec);
        assert_eq!(manual_links(&section), expected, "{case}");
        assert!(section.undetermined_installs.is_empty(), "{case}");
    }
}

/// T23's table through the scan:
/// - a `multi-user.target.wants` link to a unit with no `[Install]` is a
///   `StaticUnit` link, and so is a `.requires` link;
/// - a link to a unit whose `WantedBy=` holds a specifier inspectah does
///   not expand is `Undetermined(Specifier)`;
/// - an alias under `/etc/systemd/system` that the unit's `Alias=` does not
///   name is a `NotInInstall` link with relation `Alias` and the alias name
///   as its parent, while the enable-made link reports nothing;
/// - `UpheldBy=multi-user.target` with its `.upholds` link is
///   `Undetermined(UpheldBy)`, with no state change and no inventory entry.
///
/// Each subject is first shown on the host (the link is in the listing),
/// and a unit at the expected link alone reports nothing (T22's control).
#[test]
fn t23_hand_made_link_causes() {
    let no_install = "[Service]\nExecStart=/bin/true\n";
    let static_host = |dir: &str| {
        with_link_in(
            unlinked_host(&[("backup.service", "static")], "disable *\n")
                .with_file(&format!("{VENDOR}/backup.service"), no_install),
            ETC,
            dir,
            "backup.service",
        )
    };
    for (dir, relation, parent) in [
        (
            "multi-user.target.wants",
            LinkRelation::Wants,
            "multi-user.target",
        ),
        ("a.target.requires", LinkRelation::Requires, "a.target"),
    ] {
        let (section, _) = scan(&static_host(dir));
        assert_eq!(
            manual_links(&section),
            [(
                relation,
                parent,
                "backup.service",
                ManualLinkCause::StaticUnit
            )],
            "{dir}"
        );
        assert!(section.state_changes.is_empty(), "{dir}");
    }

    let (specifier, _) = scan(&with_link_in(
        unlinked_host(&[("host.service", "enabled")], "disable *\n").with_file(
            &format!("{VENDOR}/host.service"),
            "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=%H.target\n",
        ),
        ETC,
        "multi-user.target.wants",
        "host.service",
    ));
    assert_eq!(
        manual_links(&specifier),
        [(
            LinkRelation::Wants,
            "multi-user.target",
            "host.service",
            ManualLinkCause::Undetermined {
                reason: InstallUnknown::Specifier
            }
        )]
    );

    let aliased = with_link_in(
        unlinked_host(&[("backup.service", "enabled")], "disable *\n")
            .with_file(&format!("{VENDOR}/backup.service"), WANTS_MULTI_USER),
        ETC,
        "multi-user.target.wants",
        "backup.service",
    )
    .with_dir_entry(ETC, "backup-alias.service")
    .with_link(
        &format!("{ETC}/backup-alias.service"),
        &format!("{VENDOR}/backup.service"),
    );
    let (alias, _) = scan(&aliased);
    assert_eq!(
        entry_of(&alias, "backup.service"),
        Some((UnitState::Enabled, Some(PresetDefault::Disable))),
        "the alias stands beside an enabled unit"
    );
    assert_eq!(
        manual_links(&alias),
        [(
            LinkRelation::Alias,
            "backup-alias.service",
            "backup.service",
            ManualLinkCause::NotInInstall
        )]
    );

    let upheld = "[Service]\nExecStart=/bin/true\n\n[Install]\nUpheldBy=multi-user.target\n";
    let (upholds, _) = scan(&with_link_in(
        unlinked_host(&[("watch.service", "enabled")], "enable watch.service\n")
            .with_file(&format!("{VENDOR}/watch.service"), upheld),
        ETC,
        "multi-user.target.upholds",
        "watch.service",
    ));
    assert_eq!(
        manual_links(&upholds),
        [(
            LinkRelation::Upholds,
            "multi-user.target",
            "watch.service",
            ManualLinkCause::Undetermined {
                reason: InstallUnknown::UpheldBy
            }
        )]
    );
    assert!(upholds.state_changes.is_empty());
    assert_eq!(entry_of(&upholds, "watch.service"), None);
    assert!(upholds.undetermined_installs.is_empty(), "no /etc file");
}

/// T24's reset links:
/// - a local unit with `WantedBy=multi-user.target`, an `/etc` drop-in
///   holding only `WantedBy=`, and its `multi-user.target.wants` link:
///   a `StaticUnit` link, the drop-in shown collected first. Control:
///   without the drop-in the link is enable-made and reports nothing.
/// - `RequiredBy=a.target` in a vendor unit, an `/etc` drop-in holding an
///   empty `RequiredBy=`, and the `a.target.requires` link: an
///   `Undetermined(RequiredByReset)` link, no state change, and one record
///   naming the drop-in.
#[test]
fn t24_reset_links() {
    let drop_in_dir = format!("{ETC}/app.service.d");
    let local = || {
        with_link_in(
            unlinked_host(&[("app.service", "enabled")], "disable *\n")
                .with_dir_entry(ETC, "app.service")
                .with_file(&format!("{ETC}/app.service"), WANTS_MULTI_USER),
            ETC,
            "multi-user.target.wants",
            "app.service",
        )
    };
    let (control, _) = scan(&local());
    assert!(
        control.manual_links.is_empty(),
        "{:?}",
        control.manual_links
    );

    let (reset, _) = scan(&with_drop_in(
        local().with_dir_entry(ETC, "app.service.d"),
        &drop_in_dir,
        "10-reset.conf",
        "[Install]\nWantedBy=\n",
    ));
    assert!(
        drop_in_paths(&reset).contains(&format!("{drop_in_dir}/10-reset.conf").as_str()),
        "{:?}",
        drop_in_paths(&reset)
    );
    assert_eq!(
        manual_links(&reset),
        [(
            LinkRelation::Wants,
            "multi-user.target",
            "app.service",
            ManualLinkCause::StaticUnit
        )]
    );

    let need_dir = format!("{ETC}/need.service.d");
    let required = with_drop_in(
        with_link_in(
            unlinked_host(&[("need.service", "enabled")], "enable need.service\n")
                .with_file(
                    &format!("{VENDOR}/need.service"),
                    "[Service]\nExecStart=/bin/true\n\n[Install]\nRequiredBy=a.target\n",
                )
                .with_dir_entry(ETC, "need.service.d"),
            ETC,
            "a.target.requires",
            "need.service",
        ),
        &need_dir,
        "10-reset.conf",
        "[Install]\nRequiredBy=\n",
    );
    let (section, _) = scan(&required);
    let reason = InstallUnknown::RequiredByReset;
    assert_eq!(
        manual_links(&section),
        [(
            LinkRelation::Requires,
            "a.target",
            "need.service",
            ManualLinkCause::Undetermined { reason }
        )]
    );
    assert!(section.state_changes.is_empty());
    assert_eq!(
        undetermined(&section),
        [(
            "need.service",
            reason,
            vec![format!("{need_dir}/10-reset.conf").as_str()]
        )]
    );
}

/// T40 through the scan, each a vendor unit under an `enable` preset:
/// - `RequiredBy=a.target` with an `/etc` drop-in holding `RequiredBy=`
///   and no link: the drop-in is collected, then no state change, no link
///   and one record naming the unit, `RequiredByReset` and the drop-in;
/// - the same with an `UpheldBy=` drop-in: the record with `UpheldBy`;
/// - control: `UpheldBy=` in the unit's own file, no `/etc` file and no
///   link: a `Disabled` state change and no record; with one link: no
///   state change, the link, and no record;
/// - with `Also=helper.service` added and no link: no state change and one
///   record with no files; with one link, the link and the same record.
#[test]
fn t40_undetermined_installs() {
    let dir = format!("{ETC}/need.service.d");
    let host = |unit_file: &str| {
        unlinked_host(&[("need.service", "disabled")], "enable need.service\n")
            .with_file(&format!("{VENDOR}/need.service"), unit_file)
    };
    let with_reset = |exec: MockExecutor, text: &str| {
        with_drop_in(
            exec.with_dir_entry(ETC, "need.service.d"),
            &dir,
            "10-reset.conf",
            text,
        )
    };
    let required = "[Service]\nExecStart=/bin/true\n\n[Install]\nRequiredBy=a.target\n";
    let drop_in = format!("{dir}/10-reset.conf");
    for (text, reason) in [
        ("[Install]\nRequiredBy=\n", InstallUnknown::RequiredByReset),
        ("[Install]\nUpheldBy=a.target\n", InstallUnknown::UpheldBy),
    ] {
        let (section, _) = scan(&with_reset(host(required), text));
        assert!(
            drop_in_paths(&section).contains(&drop_in.as_str()),
            "{reason:?}: {:?}",
            drop_in_paths(&section)
        );
        assert!(section.state_changes.is_empty(), "{reason:?}");
        assert!(section.manual_links.is_empty(), "{reason:?}");
        assert_eq!(
            undetermined(&section),
            [("need.service", reason, vec![drop_in.as_str()])],
            "{reason:?}"
        );
    }

    let upheld = "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\nUpheldBy=multi-user.target\n";
    let (no_link, _) = scan(&host(upheld));
    assert_eq!(
        change_of(&no_link, "need.service"),
        Some((UnitState::Disabled, Some(PresetDefault::Enable)))
    );
    assert!(no_link.undetermined_installs.is_empty());
    let one_link = |exec| with_link_in(exec, ETC, "multi-user.target.wants", "need.service");
    let reason = InstallUnknown::UpheldBy;
    let (linked, _) = scan(&one_link(host(upheld)));
    assert!(linked.state_changes.is_empty());
    assert_eq!(
        manual_links(&linked),
        [(
            LinkRelation::Wants,
            "multi-user.target",
            "need.service",
            ManualLinkCause::Undetermined { reason }
        )]
    );
    assert!(linked.undetermined_installs.is_empty());

    let upheld_also = format!("{upheld}Also=helper.service\n");
    let (also, _) = scan(&host(&upheld_also));
    assert!(also.state_changes.is_empty());
    assert!(also.manual_links.is_empty());
    assert_eq!(undetermined(&also), [("need.service", reason, vec![])]);
    let (also_linked, _) = scan(&one_link(host(&upheld_also)));
    assert_eq!(manual_links(&also_linked).len(), 1);
    assert_eq!(
        undetermined(&also_linked),
        [("need.service", reason, vec![])]
    );
}

/// § 7.3: a link under `/etc/systemd/system` whose parent or own name does
/// not parse as a unit name produces a structured warning naming the
/// link, and no link finding. Control: a link that parses gives no such
/// warning.
#[test]
fn a_link_that_does_not_parse_is_warned_and_reported_no_further() {
    let host = || {
        unlinked_host(&[("guest.service", "static")], "disable *\n").with_file(
            &format!("{VENDOR}/guest.service"),
            "[Service]\nExecStart=/bin/true\n",
        )
    };
    let (_, control) = scan(&with_link_in(
        host(),
        ETC,
        "multi-user.target.wants",
        "guest.service",
    ));
    assert!(control.is_empty(), "{control:?}");

    for (dir, name) in [
        ("dev-foo.device.wants", "guest.service"),
        ("multi-user.target.wants", "not a unit"),
    ] {
        let exec = with_link_in(host(), ETC, dir, name);
        let (section, _) = scan(&exec);
        assert!(section.manual_links.is_empty(), "{dir}/{name}");
        let path = format!("{ETC}/{dir}/{name}");
        let warnings = scan_warnings(&exec);
        assert!(
            warnings.iter().any(|w| w.message.contains(&path)
                && w.extra.get("path") == Some(&serde_json::json!(path))),
            "{path}: {warnings:?}"
        );
    }
}

// ── A unit at its preset whose `/etc` files change `[Install]` (§ 7.4, T30) ──

/// The unit every T30 case scans.
const APP: &str = "app.service";
/// Its full shadow's path.
const APP_SHADOW: &str = "/etc/systemd/system/app.service";
/// Its `/etc` drop-in directory.
const APP_DROP_IN_DIR: &str = "/etc/systemd/system/app.service.d";
/// Its vendor drop-in directory.
const APP_VENDOR_DROP_IN_DIR: &str = "/usr/lib/systemd/system/app.service.d";
/// A unit section with no `[Install]`.
const NO_INSTALL: &str = "[Service]\nExecStart=/bin/true\n";
/// The attention note of an additive row.
const ADDS_LINKS_NOTE: &str = "Its /etc files add links the package's unit does not make.";

/// One T30 host: `app.service` under `/usr/lib` with `vendor` as its
/// file, listed with `state`, and the `/etc` files and links given.
#[derive(Clone, Copy)]
struct T30<'a> {
    case: &'a str,
    vendor: &'a str,
    /// A vendor drop-in in its own `.d`: file name and text.
    vendor_drop_in: Option<(&'a str, &'a str)>,
    /// Its full shadow's text.
    shadow: Option<&'a str>,
    /// An `/etc` drop-in in its own `.d`: file name and text.
    drop_in: Option<(&'a str, &'a str)>,
    /// The dependency directories under `/etc` holding a link to it.
    links: &'a [&'a str],
    /// The word `systemctl list-unit-files` prints for it.
    state: &'a str,
    /// An alias under `/etc` naming it: the alias name.
    alias_link: Option<&'a str>,
}

impl<'a> T30<'a> {
    /// An enabled vendor unit with `vendor` as its file and nothing else.
    const fn new(case: &'a str, vendor: &'a str) -> Self {
        Self {
            case,
            vendor,
            vendor_drop_in: None,
            shadow: None,
            drop_in: None,
            links: &[],
            state: "enabled",
            alias_link: None,
        }
    }

    /// The host under `preset`.
    fn host(&self, preset: &str) -> MockExecutor {
        let mut exec = unlinked_host(&[(APP, self.state)], preset)
            .with_file(&format!("{VENDOR}/{APP}"), self.vendor);
        if let Some((file, text)) = self.vendor_drop_in {
            exec = with_drop_in(exec, APP_VENDOR_DROP_IN_DIR, file, text);
        }
        if let Some(text) = self.shadow {
            exec = exec.with_dir_entry(ETC, APP).with_file(APP_SHADOW, text);
        }
        if let Some((file, text)) = self.drop_in {
            exec = with_drop_in(
                exec.with_dir_entry(ETC, "app.service.d"),
                APP_DROP_IN_DIR,
                file,
                text,
            );
        }
        for dir in self.links {
            exec = with_link_in(exec, ETC, dir, APP);
        }
        if let Some(alias) = self.alias_link {
            exec = exec
                .with_dir_entry(ETC, alias)
                .with_link(&format!("{ETC}/{alias}"), APP_SHADOW);
        }
        exec
    }

    /// The path of its `/etc` file: the shadow, else the drop-in.
    fn etc_file(&self) -> String {
        match (self.shadow, self.drop_in) {
            (Some(_), _) => APP_SHADOW.to_owned(),
            (None, Some((file, _))) => format!("{APP_DROP_IN_DIR}/{file}"),
            (None, None) => panic!("{}: a T30 case has an /etc file", self.case),
        }
    }

    /// Asserts its `/etc` file is collected, the subject the absence
    /// assertions that follow are about.
    fn assert_collected(&self, section: &UnitSection) {
        let path = self.etc_file();
        assert!(
            drop_in_paths(section).contains(&path.as_str()),
            "{}: {path} collected: {:?}",
            self.case,
            drop_in_paths(section)
        );
    }
}

/// Each hand-made link or install difference the scan reports, with its
/// install files.
type Finding<'a> = (
    LinkRelation,
    &'a str,
    &'a str,
    ManualLinkCause,
    Vec<&'a str>,
);

fn findings(section: &UnitSection) -> Vec<Finding<'_>> {
    section
        .manual_links
        .iter()
        .map(|l| {
            (
                l.relation,
                l.parent.as_str(),
                l.unit.as_str(),
                l.cause,
                l.install_files.iter().map(String::as_str).collect(),
            )
        })
        .collect()
}

/// The vendor section of most T30 cases: `WantedBy=multi-user.target`.
const VENDOR_WANTS: &str = WANTS_MULTI_USER;
/// A section wanted by `multi-user.target` and `extra.target`.
const WANTS_BOTH: &str =
    "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target extra.target\n";
/// A section wanted by `graphical.target` in place of `multi-user.target`.
const WANTS_GRAPHICAL: &str =
    "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=graphical.target\n";
/// A vendor section that also names `helper.service` in `Also=`.
const WANTS_WITH_HELPER: &str = "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\nAlso=helper.service\n";
/// A drop-in that resets `WantedBy=` and wants `graphical.target`.
const RESET_THEN_GRAPHICAL: &str = "[Install]\nWantedBy=\nWantedBy=graphical.target\n";

/// T30 (d): the shadow that replaces `multi-user.target` with
/// `graphical.target`, on a host with the `graphical.target` link.
const REPLACING_SHADOW: T30 = T30 {
    shadow: Some(WANTS_GRAPHICAL),
    links: &["graphical.target.wants"],
    ..T30::new("(d) a shadow replacing the target", VENDOR_WANTS)
};

/// T30 (c), first case: a shadow adding a second `WantedBy=`.
const ADDING_SHADOW: T30 = T30 {
    shadow: Some(WANTS_BOTH),
    links: &["multi-user.target.wants", "extra.target.wants"],
    ..T30::new("(c) a shadow adding a WantedBy=", VENDOR_WANTS)
};

/// T30 (g): a shadow that drops `Also=helper.service`.
const ALSO_DROPPING_SHADOW: T30 = T30 {
    shadow: Some(VENDOR_WANTS),
    links: &["multi-user.target.wants"],
    ..T30::new("(g) a shadow dropping Also=", WANTS_WITH_HELPER)
};

/// T30 (b), (c), (e) and the first case of (l), under an `enable` preset
/// with the unit enabled: each is an `Enabled` state change at its
/// `enable` preset, with its fragment as resolved, its drop-in in
/// `install_drop_ins`, the attention note, and no link finding.
#[test]
fn t30_added_links_are_an_enabled_row() {
    let cases = [
        (
            T30 {
                drop_in: Some(("10-extra.conf", "[Install]\nWantedBy=extra.target\n")),
                links: &["multi-user.target.wants", "extra.target.wants"],
                ..T30::new("(b) a drop-in adding WantedBy=extra.target", VENDOR_WANTS)
            },
            false,
        ),
        (ADDING_SHADOW, true),
        (
            T30 {
                shadow: Some(
                    "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\nAlias=app-alias.service\n",
                ),
                links: &["multi-user.target.wants"],
                alias_link: Some("app-alias.service"),
                ..T30::new("(c) a shadow adding an Alias=", VENDOR_WANTS)
            },
            true,
        ),
        (
            T30 {
                drop_in: Some(("10-install.conf", "[Install]\nWantedBy=multi-user.target\n")),
                links: &["multi-user.target.wants"],
                ..T30::new(
                    "(e) a vendor unit made installable by a drop-in",
                    NO_INSTALL,
                )
            },
            false,
        ),
        (
            T30 {
                drop_in: Some((
                    "10-reset.conf",
                    "[Install]\nWantedBy=\nWantedBy=multi-user.target extra.target\n",
                )),
                links: &["multi-user.target.wants", "extra.target.wants"],
                ..T30::new("(l) reset, then the same target and another", VENDOR_WANTS)
            },
            false,
        ),
    ];
    for (case, shadowed) in cases {
        let (section, _) = scan(&case.host("enable app.service\n"));
        let name = case.case;
        case.assert_collected(&section);
        assert_eq!(
            change_of(&section, APP),
            Some((UnitState::Enabled, Some(PresetDefault::Enable))),
            "{name}"
        );
        let fragment = fragment_of(&section, APP);
        if shadowed {
            assert!(
                matches!(fragment, UnitFragment::Shadowed { .. }),
                "{name}: {fragment:?}"
            );
        } else {
            assert!(
                matches!(fragment, UnitFragment::Vendor { .. }),
                "{name}: {fragment:?}"
            );
        }
        let change = section
            .state_changes
            .iter()
            .find(|c| c.unit().name().as_str() == APP)
            .expect("the row asserted above");
        let expected_drop_ins: Vec<String> =
            case.drop_in.map(|_| case.etc_file()).into_iter().collect();
        assert_eq!(change.install_drop_ins, expected_drop_ins, "{name}");
        assert_eq!(
            change.attention_reason.as_deref(),
            Some(ADDS_LINKS_NOTE),
            "{name}"
        );
        assert_eq!(findings(&section), [], "{name}");
        assert!(section.undetermined_installs.is_empty(), "{name}");
    }
}

/// T30 (a) and (n): a shadow with the package's `[Install]` unchanged, one
/// with the same values reordered, and an `/etc` drop-in holding only
/// `Also=` over a package `Also=helper.service`. Each `/etc` file is first
/// shown collected and the unit `Enabled` at its `enable` preset; then
/// there is no row and no finding. (b) above is the positive control on
/// the same unit and route.
#[test]
fn t30_an_unchanged_install_section_is_no_row() {
    let cases = [
        T30 {
            shadow: Some(VENDOR_WANTS),
            links: &["multi-user.target.wants"],
            ..T30::new("(a) a shadow with the same [Install]", VENDOR_WANTS)
        },
        T30 {
            shadow: Some(
                "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=extra.target multi-user.target\n",
            ),
            links: &["multi-user.target.wants", "extra.target.wants"],
            ..T30::new("(a) a shadow with the values reordered", WANTS_BOTH)
        },
        T30 {
            drop_in: Some(("10-also.conf", "[Install]\nAlso=\n")),
            links: &["multi-user.target.wants"],
            ..T30::new("(n) an empty Also= that changes nothing", WANTS_WITH_HELPER)
        },
    ];
    for case in cases {
        let (section, _) = scan(&case.host("enable app.service\n"));
        let name = case.case;
        case.assert_collected(&section);
        assert_eq!(
            entry_of(&section, APP),
            Some((UnitState::Enabled, Some(PresetDefault::Enable))),
            "{name}"
        );
        assert_eq!(change_of(&section, APP), None, "{name}");
        assert_eq!(findings(&section), [], "{name}");
        assert!(section.undetermined_installs.is_empty(), "{name}");
    }
}

/// T30 (d), (f), (g), (i), (j), the first and third cases of (k), the
/// second case of (l), and (m): no state change, and one finding per
/// difference with its relation, cause and the `/etc` install file; (i)
/// also has its undetermined record.
#[test]
fn t30_changed_links_are_install_difference_findings() {
    use LinkRelation::{Alias, Also, Wants};
    use ManualLinkCause::{HostInstallOnly, NotInInstall, VendorInstallOnly};
    let specifier = ManualLinkCause::Undetermined {
        reason: InstallUnknown::Specifier,
    };
    // (relation, parent, unit, cause, whether it names the `/etc` file)
    type Expected<'a> = Vec<(LinkRelation, &'a str, &'a str, ManualLinkCause, bool)>;
    let cases: Vec<(T30, Expected)> = vec![
        (
            REPLACING_SHADOW,
            vec![
                (Wants, "graphical.target", APP, HostInstallOnly, true),
                (Wants, "multi-user.target", APP, VendorInstallOnly, true),
            ],
        ),
        (
            T30 {
                vendor_drop_in: Some(("10-extra.conf", "[Install]\nWantedBy=extra.target\n")),
                drop_in: Some(("10-extra.conf", "[Service]\nNice=5\n")),
                links: &["multi-user.target.wants"],
                ..T30::new(
                    "(f) a [Service] drop-in hiding a vendor [Install] drop-in",
                    VENDOR_WANTS,
                )
            },
            vec![(Wants, "extra.target", APP, VendorInstallOnly, true)],
        ),
        (
            ALSO_DROPPING_SHADOW,
            vec![(Also, APP, "helper.service", VendorInstallOnly, true)],
        ),
        (
            T30 {
                shadow: Some(VENDOR_WANTS),
                links: &["multi-user.target.wants"],
                ..T30::new(
                    "(i) a package section with a specifier, shadowed",
                    "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=%H.target\n",
                )
            },
            vec![(Wants, "multi-user.target", APP, specifier, false)],
        ),
        (
            T30 {
                links: &["graphical.target.wants", "multi-user.target.wants"],
                case: "(j) the replacing shadow with a hand-made multi-user.target link",
                ..REPLACING_SHADOW
            },
            vec![
                (Wants, "multi-user.target", APP, NotInInstall, false),
                (Wants, "graphical.target", APP, HostInstallOnly, true),
            ],
        ),
        (
            T30 {
                drop_in: Some(("10-reset.conf", "[Install]\nWantedBy=\n")),
                state: "static",
                ..T30::new("(k) a WantedBy= reset, no link", VENDOR_WANTS)
            },
            vec![(Wants, "multi-user.target", APP, VendorInstallOnly, true)],
        ),
        (
            T30 {
                shadow: Some(NO_INSTALL),
                state: "static",
                ..T30::new("(k) a shadow with no [Install], no link", VENDOR_WANTS)
            },
            vec![(Wants, "multi-user.target", APP, VendorInstallOnly, true)],
        ),
        (
            T30 {
                drop_in: Some(("10-reset.conf", RESET_THEN_GRAPHICAL)),
                links: &["graphical.target.wants"],
                ..T30::new("(l) reset, then another target", VENDOR_WANTS)
            },
            vec![
                (Wants, "graphical.target", APP, HostInstallOnly, true),
                (Wants, "multi-user.target", APP, VendorInstallOnly, true),
            ],
        ),
        (
            T30 {
                drop_in: Some(("10-alias.conf", "[Install]\nAlias=\n")),
                links: &["multi-user.target.wants"],
                ..T30::new(
                    "(m) an Alias= reset",
                    "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\nAlias=x-alias.service\n",
                )
            },
            vec![(Alias, "x-alias.service", APP, VendorInstallOnly, true)],
        ),
    ];
    for (case, expected) in cases {
        let (section, _) = scan(&case.host("enable app.service\n"));
        let name = case.case;
        case.assert_collected(&section);
        assert_eq!(change_of(&section, APP), None, "{name}");
        let file = case.etc_file();
        let expected: Vec<Finding> = expected
            .into_iter()
            .map(|(relation, parent, unit, cause, with_file)| {
                let files = if with_file {
                    vec![file.as_str()]
                } else {
                    vec![]
                };
                (relation, parent, unit, cause, files)
            })
            .collect();
        // A package section that cannot be relied on, (i), also gets the
        // unit's record naming the `/etc` file (Mark Russell, 2026-11-15).
        let record = expected.iter().find_map(|(.., cause, _)| match cause {
            ManualLinkCause::Undetermined { reason } => Some((*reason, vec![file.clone()])),
            _ => None,
        });
        assert_eq!(findings(&section), expected, "{name}");
        assert_eq!(
            section
                .undetermined_installs
                .iter()
                .map(|r| (r.reason, r.install_files.clone()))
                .collect::<Vec<_>>(),
            record.into_iter().collect::<Vec<_>>(),
            "{name}"
        );
    }
}

/// T30 (k), second case: the `WantedBy=` reset drop-in with the package's
/// `multi-user.target` link still on the host. The drop-in is collected
/// and the same unit with no link reports the `VendorInstallOnly` link
/// (the positive control); with the link kept, the host and the image both
/// have it, so it produces no finding of any cause.
#[test]
fn t30_a_package_link_kept_on_the_host_is_no_finding() {
    let unlinked = T30 {
        drop_in: Some(("10-reset.conf", "[Install]\nWantedBy=\n")),
        state: "static",
        ..T30::new("(k) a WantedBy= reset", VENDOR_WANTS)
    };
    let (control, _) = scan(&unlinked.host("enable app.service\n"));
    assert_eq!(
        findings(&control),
        [(
            LinkRelation::Wants,
            "multi-user.target",
            APP,
            ManualLinkCause::VendorInstallOnly,
            vec![unlinked.etc_file().as_str()]
        )]
    );

    let kept = T30 {
        links: &["multi-user.target.wants"],
        state: "enabled",
        ..unlinked
    };
    let exec = kept.host("enable app.service\n");
    use inspectah_core::traits::executor::Executor as _;
    assert!(
        exec.read_dir(std::path::Path::new(MULTI_USER_WANTS))
            .expect("the link's directory")
            .contains(&APP.to_owned()),
        "the package's link is on the host"
    );
    let (section, _) = scan(&exec);
    kept.assert_collected(&section);
    assert_eq!(change_of(&section, APP), None);
    assert_eq!(findings(&section), []);
    assert!(section.undetermined_installs.is_empty());
}

/// T30 (h), (o) and (p):
/// - (h) the shadows of (c), (d) and (g) under a `disable` preset: the
///   ordinary `Enabled` row, with no attention note and no finding;
/// - (o) a vendor unit with no `[Install]` and an `/etc` drop-in holding
///   only `[Service]`, no link: the drop-in collected, then no state
///   change, no finding and no record;
/// - (p) the drop-in of (l)'s second case on a host with no
///   `graphical.target.wants/` link: a `Disabled` row, whose `disable` is
///   the command, and no install-difference finding.
#[test]
fn t30_other_presets_and_controls() {
    for case in [ADDING_SHADOW, REPLACING_SHADOW, ALSO_DROPPING_SHADOW] {
        let (section, _) = scan(&case.host("disable *\n"));
        let name = case.case;
        case.assert_collected(&section);
        assert_eq!(
            change_of(&section, APP),
            Some((UnitState::Enabled, Some(PresetDefault::Disable))),
            "{name}"
        );
        let change = &section.state_changes[0];
        assert_eq!(change.attention_reason, None, "{name}");
        assert_eq!(findings(&section), [], "{name}");
    }

    let service_only = T30 {
        drop_in: Some(("10-nice.conf", "[Service]\nNice=5\n")),
        state: "static",
        ..T30::new(
            "(o) a [Service] drop-in on a unit with no [Install]",
            NO_INSTALL,
        )
    };
    let (section, _) = scan(&service_only.host("enable app.service\n"));
    service_only.assert_collected(&section);
    assert!(
        section.state_changes.is_empty(),
        "{:?}",
        section.state_changes
    );
    assert_eq!(findings(&section), []);
    assert!(section.undetermined_installs.is_empty());

    let disabled = T30 {
        drop_in: Some(("10-reset.conf", RESET_THEN_GRAPHICAL)),
        state: "disabled",
        ..T30::new("(p) reset, then another target, not linked", VENDOR_WANTS)
    };
    let (section, _) = scan(&disabled.host("enable app.service\n"));
    disabled.assert_collected(&section);
    assert_eq!(
        change_of(&section, APP),
        Some((UnitState::Disabled, Some(PresetDefault::Enable)))
    );
    assert_eq!(
        section.state_changes[0].implied_action(),
        UnitAction::Disable
    );
    assert_eq!(findings(&section), []);
}

// ---------------------------------------------------------------------
// § 7.3 "`Also=` members" and § 7.4 "An owner at its preset whose member
// is off" (T24's `Also=` reset, T25 (l), T39's owner cases).
// ---------------------------------------------------------------------

/// The attention note of an owner at its preset whose member is off.
const MEMBER_OFF_NOTE: &str = "A unit it enables through Also= is not enabled on the source host.";

/// A unit file wanted by `multi-user.target` whose `[Install]` also holds
/// `extra` (`Also=...`, `DefaultInstance=...`).
fn wants_with(extra: &str) -> String {
    format!("[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\n{extra}")
}

/// A host listing `units` under `preset`, with each `(name, text)` of
/// `files` a vendor unit file and each unit of `linked` linked from
/// `multi-user.target.wants` under `/etc`.
fn also_host(
    units: &[(&str, &str)],
    preset: &str,
    files: &[(&str, &str)],
    linked: &[&str],
) -> MockExecutor {
    let mut exec = unlinked_host(units, preset);
    for (name, text) in files {
        exec = exec.with_file(&format!("{VENDOR}/{name}"), text);
    }
    for unit in linked {
        exec = with_link_in(exec, ETC, "multi-user.target.wants", unit);
    }
    exec
}

/// The `also` list of `unit`'s state change, as names and links.
fn also_of<'a>(section: &'a UnitSection, unit: &str) -> Vec<(&'a str, MemberLinks)> {
    section
        .state_changes
        .iter()
        .find(|c| c.unit().name().as_str() == unit)
        .unwrap_or_else(|| panic!("{unit} should be a state change"))
        .also
        .iter()
        .map(|m| (m.unit.as_str(), m.links))
        .collect()
}

/// § 7.3: a chain `a`, `b`, `c` records `b` and `c` on `a`, in the order
/// the walk reaches them, and `a` itself is left out though `c` names it.
#[test]
fn an_also_chain_records_every_unit_it_reaches() {
    let exec = also_host(
        &[("a.service", "enabled"), ("b.service", "enabled")],
        "disable *\n",
        &[
            ("a.service", &wants_with("Also=b.service\n")),
            ("b.service", &wants_with("Also=c.service\n")),
            ("c.service", &wants_with("Also=a.service\n")),
        ],
        &["a.service", "b.service"],
    );
    let (section, _) = scan(&exec);
    assert_eq!(
        also_of(&section, "a.service"),
        [
            ("b.service", MemberLinks::Installed),
            ("c.service", MemberLinks::Unlinked)
        ]
    );
    assert_eq!(
        also_of(&section, "b.service"),
        [
            ("c.service", MemberLinks::Unlinked),
            ("a.service", MemberLinks::Installed)
        ]
    );
}

/// § 6.3 and § 7.3: each `MemberLinks` value from a planted link set.
/// `Unlinked` and `Inert` count every link under `/etc`, so a member with
/// a hand-made link is `Other` whatever its `[Install]`; vendor links
/// complete an install and never start one.
#[test]
fn each_member_links_value_comes_from_the_links() {
    let two_targets = "[Service]\nExecStart=/bin/true\n\n[Install]\n\
                       WantedBy=multi-user.target graphical.target\n";
    let members = [
        ("installed.service", WANTS_MULTI_USER),
        ("unlinked.service", WANTS_MULTI_USER),
        ("vendor-linked.service", WANTS_MULTI_USER),
        ("elsewhere.service", WANTS_MULTI_USER),
        ("partial.service", two_targets),
        ("inert.service", NO_INSTALL),
        ("hand-made.service", NO_INSTALL),
        ("also-only.service", "[Install]\nAlso=unlinked.service\n"),
        ("masked.service", WANTS_MULTI_USER),
        ("undetermined.service", REQUIRED_BY_RESET),
        ("unsupported.service", WANTS_MULTI_USER),
    ];
    let also: Vec<&str> = members
        .iter()
        .map(|(name, _)| *name)
        .chain(["missing.service"])
        .collect();
    let owner = wants_with(&format!("Also={}\n", also.join(" ")));
    let mut files = vec![("owner.service", owner.as_str())];
    files.extend(members);
    let mut exec = also_host(
        &[("owner.service", "enabled")],
        "disable *\n",
        &files,
        &["owner.service", "installed.service", "partial.service"],
    );
    exec = with_link_in(
        exec,
        VENDOR,
        "multi-user.target.wants",
        "vendor-linked.service",
    );
    exec = with_link_in(exec, ETC, "rescue.target.wants", "elsewhere.service");
    exec = with_link_in(exec, ETC, "multi-user.target.wants", "hand-made.service");
    exec = exec
        .with_dir_entry(ETC, "masked.service")
        .with_link(&format!("{ETC}/masked.service"), "/dev/null")
        .with_file(
            "/usr/local/lib/systemd/system/unsupported.service",
            WANTS_MULTI_USER,
        );
    let (section, _) = scan(&exec);
    assert_eq!(
        also_of(&section, "owner.service"),
        [
            ("installed.service", MemberLinks::Installed),
            ("unlinked.service", MemberLinks::Unlinked),
            ("vendor-linked.service", MemberLinks::Unlinked),
            ("elsewhere.service", MemberLinks::Other),
            ("partial.service", MemberLinks::Other),
            ("inert.service", MemberLinks::Inert),
            ("hand-made.service", MemberLinks::Other),
            ("also-only.service", MemberLinks::Inert),
            ("masked.service", MemberLinks::Other),
            ("undetermined.service", MemberLinks::Other),
            ("unsupported.service", MemberLinks::Other),
            ("missing.service", MemberLinks::Inert),
        ]
    );
}

/// An `[Install]` section whose `RequiredBy=` is reset after a value, which
/// the reader records as `Undetermined(RequiredByReset)`.
const REQUIRED_BY_RESET: &str = "[Service]\nExecStart=/bin/true\n\n[Install]\n\
                                 RequiredBy=a.target\nRequiredBy=\n";

/// § 7.3: the walk does not continue through a member whose section could
/// not be read. A member whose `RequiredBy=` is reset holds
/// `Also=deep.service`: the member is `Other` and `deep.service` is not
/// recorded. The control, the same member with a readable section, records
/// `deep.service`.
#[test]
fn the_also_walk_stops_at_a_member_whose_section_cannot_be_read() {
    let host = |member: &str| {
        also_host(
            &[("owner.service", "enabled")],
            "disable *\n",
            &[
                ("owner.service", &wants_with("Also=member.service\n")),
                ("member.service", member),
                ("deep.service", WANTS_MULTI_USER),
            ],
            &["owner.service"],
        )
    };
    let readable = format!("{WANTS_MULTI_USER}Also=deep.service\n");
    let (section, _) = scan(&host(&readable));
    assert_eq!(
        also_of(&section, "owner.service"),
        [
            ("member.service", MemberLinks::Unlinked),
            ("deep.service", MemberLinks::Unlinked)
        ]
    );

    let unreadable = format!("{REQUIRED_BY_RESET}Also=deep.service\n");
    let (section, _) = scan(&host(&unreadable));
    assert_eq!(
        also_of(&section, "owner.service"),
        [("member.service", MemberLinks::Other)]
    );
}

/// T24: an empty `Also=` in an `/etc` drop-in never clears, so the
/// fragment's member stays in `also`, and the drop-in is an install file.
#[test]
fn t24_an_empty_also_leaves_the_member_in_also() {
    let dir = format!("{ETC}/owner.service.d");
    let exec = also_host(
        &[("owner.service", "enabled")],
        "disable *\n",
        &[
            ("owner.service", &wants_with("Also=helper.service\n")),
            ("helper.service", WANTS_MULTI_USER),
        ],
        &["owner.service"],
    );
    let exec = with_drop_in(
        exec.with_dir_entry(ETC, "owner.service.d"),
        &dir,
        "10-also.conf",
        "[Install]\nAlso=\n",
    );
    let (section, _) = scan(&exec);
    let change = section
        .state_changes
        .iter()
        .find(|c| c.unit().name().as_str() == "owner.service")
        .expect("the owner's state change");
    assert_eq!(change.install_drop_ins, [format!("{dir}/10-also.conf")]);
    assert_eq!(
        also_of(&section, "owner.service"),
        [("helper.service", MemberLinks::Unlinked)]
    );
}

/// T25 (l): `Also=` naming a template is recorded as its
/// `DefaultInstance=`, read and linked as that instance; with no
/// `DefaultInstance=`, it is `Other` under the template's name.
#[test]
fn t25_l_an_also_template_is_its_default_instance() {
    let host = |template: &str, linked: &[&str]| {
        also_host(
            &[("owner.service", "enabled")],
            "disable *\n",
            &[
                ("owner.service", &wants_with("Also=t@.service\n")),
                ("t@.service", template),
            ],
            linked,
        )
    };
    let with_default = wants_with("DefaultInstance=x\n");
    let (section, _) = scan(&host(&with_default, &["owner.service"]));
    assert_eq!(
        also_of(&section, "owner.service"),
        [("t@x.service", MemberLinks::Unlinked)]
    );
    let (section, _) = scan(&host(&with_default, &["owner.service", "t@x.service"]));
    assert_eq!(
        also_of(&section, "owner.service"),
        [("t@x.service", MemberLinks::Installed)]
    );
    let (section, _) = scan(&host(WANTS_MULTI_USER, &["owner.service"]));
    assert_eq!(
        also_of(&section, "owner.service"),
        [("t@.service", MemberLinks::Other)]
    );
}

/// § 6.3: a member's vendor drop-in names are recorded on its entry.
#[test]
fn a_members_vendor_drop_ins_are_recorded() {
    let exec = also_host(
        &[("owner.service", "enabled")],
        "disable *\n",
        &[
            ("owner.service", &wants_with("Also=helper.service\n")),
            ("helper.service", WANTS_MULTI_USER),
        ],
        &["owner.service"],
    );
    let exec = with_drop_in(
        exec,
        &format!("{VENDOR}/helper.service.d"),
        "10-vendor.conf",
        "[Service]\nNice=5\n",
    );
    let (section, _) = scan(&exec);
    let change = section
        .state_changes
        .iter()
        .find(|c| c.unit().name().as_str() == "owner.service")
        .expect("the owner's state change");
    assert_eq!(change.also.len(), 1, "{:?}", change.also);
    assert_eq!(change.also[0].vendor_drop_ins, ["10-vendor.conf"]);
}

/// T39's owner cases, collector half (§ 7.4, "An owner at its preset whose
/// member is off"). A vendor `m.service` with `Also=m.socket`, enabled
/// under `enable m.service` with its link, and `m.socket` under
/// `disable *`: with `m.socket` unlinked, `m.service` is an `Enabled` row
/// with `m.socket` `Unlinked` and the attention note, and `m.socket`, in
/// the inventory as `Disabled`, has no row; with `m.socket` linked, and
/// with an `Inert` member, the owner is in the inventory and has no row.
#[test]
fn t39_an_owner_at_its_preset_whose_member_is_off() {
    let socket = "[Socket]\nListenStream=/run/m.sock\n\n[Install]\nWantedBy=sockets.target\n";
    let preset = "enable m.service\ndisable *\n";
    let host = |also: &str, socket_listed: &str| {
        also_host(
            &[("m.service", "enabled"), ("m.socket", socket_listed)],
            preset,
            &[
                ("m.service", &wants_with(also)),
                ("m.socket", socket),
                ("m-helper.service", NO_INSTALL),
            ],
            &["m.service"],
        )
    };

    let (section, _) = scan(&host("Also=m.socket\n", "disabled"));
    assert_eq!(
        entry_of(&section, "m.service"),
        Some((UnitState::Enabled, Some(PresetDefault::Enable)))
    );
    assert_eq!(
        change_of(&section, "m.service"),
        Some((UnitState::Enabled, Some(PresetDefault::Enable)))
    );
    assert_eq!(
        also_of(&section, "m.service"),
        [("m.socket", MemberLinks::Unlinked)]
    );
    let owner = section
        .state_changes
        .iter()
        .find(|c| c.unit().name().as_str() == "m.service")
        .expect("the owner's row");
    assert_eq!(owner.attention_reason.as_deref(), Some(MEMBER_OFF_NOTE));
    assert_eq!(
        entry_of(&section, "m.socket"),
        Some((UnitState::Disabled, Some(PresetDefault::Disable)))
    );
    assert_eq!(change_of(&section, "m.socket"), None);

    // The stock control: `m.socket` linked as the package left it.
    let linked = with_link_in(
        host("Also=m.socket\n", "enabled"),
        ETC,
        "sockets.target.wants",
        "m.socket",
    );
    let (section, _) = scan(&linked);
    assert_eq!(
        entry_of(&section, "m.service"),
        Some((UnitState::Enabled, Some(PresetDefault::Enable)))
    );
    assert_eq!(change_of(&section, "m.service"), None);

    // The `Inert` control: a member with no `[Install]` and no link.
    let (section, _) = scan(&host("Also=m-helper.service\n", "disabled"));
    assert_eq!(
        entry_of(&section, "m.service"),
        Some((UnitState::Enabled, Some(PresetDefault::Enable)))
    );
    assert_eq!(change_of(&section, "m.service"), None);
}

/// T39's owner case for an instance member, collector half: a vendor
/// `p.service` with `Also=t@two.service`, enabled under `enable p.service`
/// with its link, the package shipping only `t@.service` under
/// `disable *`. With no link for `t@two`, `p.service` is an `Enabled` row
/// with `t@two.service` `Unlinked`, resolved through `t@.service`; with
/// `t@two` linked, the owner is in the inventory and has no row.
#[test]
fn t39_an_owner_whose_instance_member_is_off() {
    let host = |linked: &[&str]| {
        also_host(
            &[("p.service", "enabled"), ("t@.service", "indirect")],
            "enable p.service\ndisable *\n",
            &[
                ("p.service", &wants_with("Also=t@two.service\n")),
                ("t@.service", WANTS_MULTI_USER),
            ],
            linked,
        )
    };
    let (section, _) = scan(&host(&["p.service"]));
    assert_eq!(
        entry_of(&section, "p.service"),
        Some((UnitState::Enabled, Some(PresetDefault::Enable)))
    );
    assert_eq!(
        change_of(&section, "p.service"),
        Some((UnitState::Enabled, Some(PresetDefault::Enable)))
    );
    assert_eq!(
        also_of(&section, "p.service"),
        [("t@two.service", MemberLinks::Unlinked)]
    );

    let (section, _) = scan(&host(&["p.service", "t@two.service"]));
    assert_eq!(
        entry_of(&section, "p.service"),
        Some((UnitState::Enabled, Some(PresetDefault::Enable)))
    );
    assert_eq!(change_of(&section, "p.service"), None);
}

/// § 7.4 with § 7.3's template rule, a T30 case: the package's
/// `app.service` names `Also=t@.service`, whose `DefaultInstance=` is `x`,
/// and a full shadow names `Also=t@x.service`, the same unit. Under an
/// `enable` preset with the unit and `t@x` linked, the two sections are
/// equal: no row and no finding. Control: a shadow naming `t@y.service`
/// differs, with one `Also` finding each way, named as instances.
#[test]
fn t30_an_also_template_compares_as_its_default_instance() {
    const VENDOR_ALSO: &str = "[Service]\nExecStart=/bin/true\n\n[Install]\n\
                               WantedBy=multi-user.target\nAlso=t@.service\n";
    let template = wants_with("DefaultInstance=x\n");
    let host = |shadow: &'static str| {
        let case = T30 {
            shadow: Some(shadow),
            links: &["multi-user.target.wants"],
            ..T30::new("an Also= template", VENDOR_ALSO)
        };
        let exec = case
            .host("enable app.service\ndisable *\n")
            .with_file(&format!("{VENDOR}/t@.service"), &template);
        let exec = with_link_in(exec, ETC, "multi-user.target.wants", "t@x.service");
        (case, exec)
    };

    let (case, exec) = host(
        "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\nAlso=t@x.service\n",
    );
    let (section, _) = scan(&exec);
    case.assert_collected(&section);
    assert_eq!(
        entry_of(&section, APP),
        Some((UnitState::Enabled, Some(PresetDefault::Enable)))
    );
    assert_eq!(change_of(&section, APP), None);
    assert_eq!(findings(&section), []);

    let (case, exec) = host(
        "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\nAlso=t@y.service\n",
    );
    let (section, _) = scan(&exec);
    case.assert_collected(&section);
    // The owner check applies where the comparison differs too: the
    // shadow's member `t@y.service` has no link, so the unit is an
    // `Enabled` row beside its findings.
    assert_eq!(
        change_of(&section, APP),
        Some((UnitState::Enabled, Some(PresetDefault::Enable)))
    );
    assert_eq!(
        also_of(&section, APP),
        [("t@y.service", MemberLinks::Unlinked)]
    );
    assert_eq!(
        findings(&section),
        [
            (
                LinkRelation::Also,
                APP,
                "t@y.service",
                ManualLinkCause::HostInstallOnly,
                vec![APP_SHADOW]
            ),
            (
                LinkRelation::Also,
                APP,
                "t@x.service",
                ManualLinkCause::VendorInstallOnly,
                vec![APP_SHADOW]
            ),
        ]
    );
}

// ── What an instance needs (§ 7.5, T28's collector cases) ──────────

/// A host with the vendor template `template` holding `text`, listed
/// `indirect` under `disable *`, and `instance` linked from
/// `multi-user.target.wants`, so the instance is an `Enabled` state
/// change against a `disable` preset.
fn instance_host(template: &str, text: &str, instance: &str) -> MockExecutor {
    also_host(
        &[(template, "indirect")],
        "disable *\n",
        &[(template, text)],
        &[instance],
    )
}

/// A template's `[Service]` lines, wanted by `multi-user.target`.
fn template_with(service: &str) -> String {
    format!("[Service]\n{service}\n[Install]\nWantedBy=multi-user.target\n")
}

/// The `required_config` and `data_paths` of `unit`'s state change.
fn needs_of(section: &UnitSection, unit: &str) -> (Vec<String>, Vec<String>) {
    let change = section
        .state_changes
        .iter()
        .find(|c| c.unit().name().as_str() == unit)
        .unwrap_or_else(|| panic!("{unit} should be a state change"));
    (change.required_config.clone(), change.data_paths.clone())
}

/// § 7.5's fixed table: `wg-quick@.service` names no path under `/etc`,
/// and `wg-quick@wg0` needs `/etc/wireguard/wg0.conf` all the same. The
/// path is kept though nothing exists there.
#[test]
fn t28_wg_quick_needs_its_configuration_from_the_table() {
    let text = template_with("Type=oneshot\nExecStart=/usr/bin/wg-quick up %i\n");
    let (section, _) = scan(&instance_host(
        "wg-quick@.service",
        &text,
        "wg-quick@wg0.service",
    ));
    assert_eq!(
        needs_of(&section, "wg-quick@wg0.service"),
        (vec!["/etc/wireguard/wg0.conf".to_owned()], vec![])
    );
}

/// § 7.5: the other two configuration rows of the fixed table.
#[test]
fn the_openvpn_templates_need_their_configuration_from_the_table() {
    let text = template_with("ExecStart=/usr/sbin/openvpn --config %i.conf\n");
    for (template, instance, expected) in [
        (
            "openvpn-client@.service",
            "openvpn-client@site.service",
            "/etc/openvpn/client/site.conf",
        ),
        (
            "openvpn-server@.service",
            "openvpn-server@site.service",
            "/etc/openvpn/server/site.conf",
        ),
    ] {
        let (section, _) = scan(&instance_host(template, &text, instance));
        assert_eq!(
            needs_of(&section, instance),
            (vec![expected.to_owned()], vec![]),
            "{instance}"
        );
    }
}

/// § 7.5: an `EnvironmentFile=` with the `-` prefix is optional to
/// systemd and is left out; one without it is required. A path that does
/// not hold `%i` or `%I` is not an instance's.
#[test]
fn an_optional_environment_file_is_not_required() {
    let text = template_with(
        "EnvironmentFile=/etc/x/%i.env\nEnvironmentFile=-/etc/y/%i.env\n\
         EnvironmentFile=/etc/sysconfig/worker\nExecStart=/usr/bin/worker\n",
    );
    let (section, _) = scan(&instance_host(
        "worker@.service",
        &text,
        "worker@site.service",
    ));
    assert_eq!(
        needs_of(&section, "worker@site.service"),
        (vec!["/etc/x/site.env".to_owned()], vec![])
    );
}

/// § 7.5's data row of the fixed table: `dirsrv@example` keeps
/// `/var/lib/dirsrv/slapd-example` when that exists on the host, and
/// records nothing for it when it does not. The template's own
/// `/etc/dirsrv/slapd-%i` is found by the derivation, and its `/run` path
/// is outside `/etc` and `/var` and dropped.
#[test]
fn t28_dirsrv_keeps_its_data_directory_when_it_exists() {
    let text = template_with(
        "ExecStart=/usr/sbin/ns-slapd -D /etc/dirsrv/slapd-%i -i /run/dirsrv/slapd-%i.pid\n",
    );
    let host = || instance_host("dirsrv@.service", &text, "dirsrv@example.service");
    let config = vec!["/etc/dirsrv/slapd-example".to_owned()];

    let (section, _) = scan(&host().with_dir("/var/lib/dirsrv/slapd-example", vec![]));
    assert_eq!(
        needs_of(&section, "dirsrv@example.service"),
        (
            config.clone(),
            vec!["/var/lib/dirsrv/slapd-example".to_owned()]
        )
    );

    let (section, _) = scan(&host());
    assert_eq!(
        needs_of(&section, "dirsrv@example.service"),
        (config, vec![])
    );
}

/// § 7.5: a path under `/var/run` or `/var/lock` resolves into `/run` and
/// is not data, even when something is there.
#[test]
fn t28_a_runtime_path_under_var_is_not_data() {
    let text = template_with(
        "PIDFile=/var/run/worker/%i.pid\nExecStartPre=/usr/bin/touch /var/lock/worker-%i\n\
         ExecStart=/usr/bin/worker\n",
    );
    let (section, _) = scan(
        &instance_host("worker@.service", &text, "worker@site.service")
            .with_file("/var/run/worker/site.pid", "1\n")
            .with_file("/var/lock/worker-site", ""),
    );
    assert_eq!(needs_of(&section, "worker@site.service"), (vec![], vec![]));
}

/// § 7.5: a `StateDirectory=` value that holds `%i` is data under
/// `/var/lib`, kept when it exists; a value without one is not the
/// instance's.
#[test]
fn a_state_directory_for_the_instance_is_data() {
    let text = template_with("StateDirectory=worker/%i shared\nExecStart=/usr/bin/worker\n");
    let host = || instance_host("worker@.service", &text, "worker@site.service");
    let (section, _) = scan(
        &host()
            .with_dir("/var/lib/worker/site", vec![])
            .with_dir("/var/lib/shared", vec![]),
    );
    assert_eq!(
        needs_of(&section, "worker@site.service"),
        (vec![], vec!["/var/lib/worker/site".to_owned()])
    );
    let (section, _) = scan(&host());
    assert_eq!(needs_of(&section, "worker@site.service"), (vec![], vec![]));
}

/// § 6.7: a value is stored only when it expands to an absolute path under
/// `/etc` or `/var`. A path elsewhere, one that climbs out with `..`
/// through `%I`, and one holding a specifier inspectah does not expand are
/// dropped; a path after `--option=` is read.
#[test]
fn a_value_outside_etc_or_var_is_dropped() {
    let text = template_with(
        "ExecStart=/usr/bin/worker --dir /srv/%i --conf=/etc/worker/%i.conf\n\
         ReadWritePaths=/var/lib/w/%I /etc/w/%h/%i\n",
    );
    let (section, _) = scan(
        &instance_host("worker@.service", &text, "worker@..-..-usr.service")
            .with_dir("/var/lib/w/../../usr", vec![]),
    );
    assert_eq!(
        needs_of(&section, "worker@..-..-usr.service"),
        (vec!["/etc/worker/..-..-usr.conf".to_owned()], vec![])
    );
}

/// § 7.2: the effective unit is the file and the drop-ins that apply to
/// it, so a drop-in in the instance's own `.d` under `/etc` adds what it
/// names.
#[test]
fn an_instance_drop_in_adds_what_it_names() {
    let text = template_with("ExecStart=/usr/bin/worker\n");
    let exec = with_drop_in(
        instance_host("worker@.service", &text, "worker@site.service")
            .with_dir_entry(ETC, "worker@site.service.d"),
        &format!("{ETC}/worker@site.service.d"),
        "10-env.conf",
        "[Service]\nEnvironmentFile=/etc/z/%i.env\n",
    );
    let (section, _) = scan(&exec);
    assert_eq!(
        needs_of(&section, "worker@site.service"),
        (vec!["/etc/z/site.env".to_owned()], vec![])
    );
}

/// Control: a unit that is not an instance records neither list, though
/// its file names `%i`.
#[test]
fn a_unit_that_is_not_an_instance_needs_nothing() {
    let text = template_with("EnvironmentFile=/etc/x/%i.env\nExecStart=/usr/bin/worker\n");
    let exec = also_host(
        &[("worker.service", "enabled")],
        "disable *\n",
        &[("worker.service", &text)],
        &["worker.service"],
    );
    let (section, _) = scan(&exec);
    assert_eq!(needs_of(&section, "worker.service"), (vec![], vec![]));
}
