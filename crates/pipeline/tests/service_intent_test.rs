use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::services::{
    PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
};
use inspectah_pipeline::render::service_intent::{
    AdvisoryReason, effective_target_packages, is_package_installable, render_service_intent,
};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn state_change(
    unit: &str,
    current_state: ServiceUnitState,
    default_state: Option<PresetDefault>,
    owning_package: Option<&str>,
) -> ServiceStateChange {
    ServiceStateChange {
        unit: unit.into(),
        current_state,
        default_state,
        include: true,
        locked: false,
        owning_package: owning_package.map(str::to_string),
        fleet: None,
        attention_reason: None,
    }
}

// ---------------------------------------------------------------------------
// Prior task tests (unchanged)
// ---------------------------------------------------------------------------

#[test]
fn test_effective_target_packages_uses_plain_names_and_include_true() {
    let rpm = RpmSection {
        baseline_package_names: Some(vec!["firewalld".into(), "systemd".into()]),
        packages_added: vec![
            PackageEntry {
                name: "custom-app".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                locked: false,
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "excluded-app".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: false,
                locked: false,
                source_repo: "appstream".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let names = effective_target_packages(&rpm);

    assert!(names.contains("firewalld"));
    assert!(names.contains("systemd"));
    assert!(names.contains("custom-app"));
    assert!(!names.contains("excluded-app"));
}

#[test]
fn test_is_package_installable_matches_manual_follow_up_contract() {
    let installable = PackageEntry {
        name: "httpd".into(),
        arch: "x86_64".into(),
        state: PackageState::Added,
        include: true,
        locked: false,
        source_repo: "appstream".into(),
        ..Default::default()
    };
    let local = PackageEntry {
        name: "local-tool".into(),
        arch: "x86_64".into(),
        state: PackageState::LocalInstall,
        include: true,
        locked: false,
        source_repo: String::new(),
        ..Default::default()
    };
    let empty_repo = PackageEntry {
        name: "mystery".into(),
        arch: "x86_64".into(),
        state: PackageState::Added,
        include: true,
        locked: false,
        source_repo: String::new(),
        ..Default::default()
    };

    assert!(is_package_installable(&installable));
    assert!(!is_package_installable(&local));
    assert!(!is_package_installable(&empty_repo));
}

// ---------------------------------------------------------------------------
// Task 4: Service render plan tests
// ---------------------------------------------------------------------------

/// Tier 7: package not in baseline, not in packages_added, baseline
/// available → proven absent → omit.
#[test]
fn test_service_render_plan_omits_proven_absent_service() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["firewalld".into()]),
        packages_added: vec![],
        no_baseline: false,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "sssd-kcm.service",
            ServiceUnitState::Disabled,
            Some(PresetDefault::Enable),
            Some("sssd"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    // Omission comment IS in lines — user sees what was excluded
    assert!(
        plan.lines
            .iter()
            .any(|line| line.contains("# Omitted: sssd-kcm.service")),
        "omission comment must appear in output lines"
    );
    // No systemctl command references the omitted unit
    assert!(
        plan.lines
            .iter()
            .all(|line| !line.contains("systemctl") || !line.contains("sssd-kcm.service")),
        "omitted service must not appear in systemctl commands"
    );
    assert_eq!(plan.omissions.len(), 1);
    assert_eq!(plan.omissions[0].unit, "sssd-kcm.service");
    assert_eq!(plan.omissions[0].owning_package, "sssd");
}

/// Tiers 2+6 stacked: package excluded AND baseline unavailable → advisory
/// with both reasons, service still emitted.
#[test]
fn test_service_render_plan_stacks_package_excluded_and_baseline_unavailable() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec![]),
        packages_added: vec![PackageEntry {
            name: "custom-app".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: false,
            locked: false,
            source_repo: "appstream".into(),
            ..Default::default()
        }],
        no_baseline: true,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "custom-app.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("custom-app"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    assert!(
        plan.omissions.is_empty(),
        "advisory service must not be omitted"
    );
    assert_eq!(plan.advisories.len(), 1);
    assert_eq!(
        plan.advisories[0].reasons,
        vec![
            AdvisoryReason::PackageExcluded,
            AdvisoryReason::BaselineUnavailable
        ]
    );
    // Service must still be emitted
    assert!(
        plan.lines.iter().any(|l| l.contains("custom-app.service")),
        "advisory service must still appear in output"
    );
}

/// Tier 3: LocalInstall package → PackageUnreachable advisory, service
/// still emitted.
#[test]
fn test_service_render_plan_emits_package_unreachable_service() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["firewalld".into()]),
        packages_added: vec![PackageEntry {
            name: "local-tool".into(),
            arch: "x86_64".into(),
            state: PackageState::LocalInstall,
            include: true,
            locked: false,
            source_repo: String::new(),
            ..Default::default()
        }],
        no_baseline: false,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "local-tool.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("local-tool"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    assert!(plan.omissions.is_empty());
    assert_eq!(plan.advisories.len(), 1);
    assert_eq!(plan.advisories[0].unit, "local-tool.service");
    assert_eq!(
        plan.advisories[0].reasons,
        vec![AdvisoryReason::PackageUnreachable]
    );
    assert!(
        plan.lines.iter().any(|l| l.contains("local-tool.service")),
        "unreachable service must still be emitted"
    );
}

/// Tier 1: owning_package: None → emitted conservatively, zero omissions,
/// zero advisories.
#[test]
fn test_service_render_plan_keeps_unknown_owner_conservative() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["firewalld".into()]),
        packages_added: vec![],
        no_baseline: false,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "mystery.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            None, // no owning package
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    assert!(
        plan.omissions.is_empty(),
        "unknown owner must not be omitted"
    );
    assert!(
        plan.advisories.is_empty(),
        "unknown owner must not get advisory"
    );
    assert!(
        plan.lines.iter().any(|l| l.contains("mystery.service")),
        "unknown owner must be emitted"
    );
}

/// Proven-absent package with config-tree timer → omitted (not deferred).
/// "Suppress beats defer" — omission is evaluated BEFORE config-tree deferral.
#[test]
fn test_service_render_plan_suppresses_before_config_tree_deferral() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["firewalld".into()]),
        packages_added: vec![],
        no_baseline: false,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "absent-timer.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("absent-pkg"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });
    // Make this unit also a config-tree timer unit
    snap.scheduled_tasks = Some(inspectah_core::types::scheduled::ScheduledTaskSection {
        systemd_timers: vec![inspectah_core::types::scheduled::SystemdTimer {
            name: "absent-timer".into(),
            source: "local".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    let plan = render_service_intent(&snap);

    // Must be omitted, NOT deferred
    assert_eq!(
        plan.omissions.len(),
        1,
        "proven-absent must be omitted even with timer"
    );
    assert_eq!(plan.omissions[0].unit, "absent-timer.service");
    // Omission comment IS in lines
    assert!(
        plan.lines
            .iter()
            .any(|l| l.contains("# Omitted: absent-timer.service")),
        "omission comment must appear in output lines"
    );
    // But no systemctl or deferred line references it
    assert!(
        plan.lines.iter().all(|l| {
            if l.starts_with("# Omitted:") {
                return true;
            }
            !l.contains("absent-timer")
        }),
        "omitted service must not appear in systemctl or deferred lines"
    );
}

/// Tiers 4+5: baseline package + included-installable package → zero
/// omissions, zero advisories.
#[test]
fn test_service_render_plan_proven_present_emits_clean() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["firewalld".into()]),
        packages_added: vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: true,
            locked: false,
            source_repo: "appstream".into(),
            ..Default::default()
        }],
        no_baseline: false,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![
            state_change(
                "firewalld.service",
                ServiceUnitState::Enabled,
                Some(PresetDefault::Disable),
                Some("firewalld"),
            ),
            state_change(
                "httpd.service",
                ServiceUnitState::Enabled,
                Some(PresetDefault::Disable),
                Some("httpd"),
            ),
        ],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    assert!(
        plan.omissions.is_empty(),
        "proven-present must not be omitted"
    );
    assert!(
        plan.advisories.is_empty(),
        "proven-present must not get advisory"
    );
    assert!(
        plan.lines.iter().any(|l| l.contains("firewalld.service")),
        "baseline service must be emitted"
    );
    assert!(
        plan.lines.iter().any(|l| l.contains("httpd.service")),
        "added-installable service must be emitted"
    );
}

/// Tier 6 in isolation: package not found anywhere + baseline unavailable
/// → BaselineUnavailable advisory only (not stacked).
#[test]
fn test_service_render_plan_pure_baseline_unavailable_advisory() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: None, // no baseline at all
        packages_added: vec![],
        no_baseline: true,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "unknown-pkg.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("unknown-pkg"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    assert!(
        plan.omissions.is_empty(),
        "can't prove absence without baseline"
    );
    assert_eq!(plan.advisories.len(), 1);
    assert_eq!(
        plan.advisories[0].reasons,
        vec![AdvisoryReason::BaselineUnavailable]
    );
    assert!(
        plan.lines.iter().any(|l| l.contains("unknown-pkg.service")),
        "advisory service must still be emitted"
    );
}

/// Advisory service with config-tree timer → advisory survives, service is
/// deferred (not suppressed). Advisory annotates, it doesn't remove.
#[test]
fn test_service_render_plan_advisory_survives_config_tree_deferral() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: None,
        packages_added: vec![],
        no_baseline: true,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "advisory-timer.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("advisory-pkg"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });
    snap.scheduled_tasks = Some(inspectah_core::types::scheduled::ScheduledTaskSection {
        systemd_timers: vec![inspectah_core::types::scheduled::SystemdTimer {
            name: "advisory-timer".into(),
            source: "local".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    let plan = render_service_intent(&snap);

    // Advisory must survive even though the service is deferred
    assert_eq!(plan.advisories.len(), 1);
    assert_eq!(plan.advisories[0].unit, "advisory-timer.service");
    assert_eq!(
        plan.advisories[0].reasons,
        vec![AdvisoryReason::BaselineUnavailable]
    );
    // Service is deferred, not in systemctl lines
    assert!(
        plan.lines
            .iter()
            .all(|l| !l.contains("systemctl") || !l.contains("advisory-timer")),
        "deferred service should not be in systemctl lines"
    );
    // But it should appear in the deferred comment
    assert!(
        plan.lines
            .iter()
            .any(|l| l.contains("deferred") && l.contains("advisory-timer")),
        "deferred service should appear in deferred comment"
    );
}

/// Tiers 2+6 stacked: verify both PackageExcluded and BaselineUnavailable
/// are present, service is emitted.
#[test]
fn test_service_render_plan_stacked_advisory_verifies_multi_reason() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec![]),
        packages_added: vec![PackageEntry {
            name: "stacked-pkg".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: false,
            locked: false,
            source_repo: "appstream".into(),
            ..Default::default()
        }],
        no_baseline: true,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "stacked-pkg.service",
            ServiceUnitState::Disabled,
            Some(PresetDefault::Enable),
            Some("stacked-pkg"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    assert!(plan.omissions.is_empty());
    assert_eq!(plan.advisories.len(), 1);
    let advisory = &plan.advisories[0];
    assert_eq!(advisory.unit, "stacked-pkg.service");
    assert_eq!(advisory.owning_package, "stacked-pkg");
    assert_eq!(advisory.reasons.len(), 2);
    assert!(advisory.reasons.contains(&AdvisoryReason::PackageExcluded));
    assert!(
        advisory
            .reasons
            .contains(&AdvisoryReason::BaselineUnavailable)
    );
    assert!(
        plan.lines.iter().any(|l| l.contains("stacked-pkg.service")),
        "stacked-advisory service must be emitted"
    );
}

/// Present package in config-tree timer → deferred, not omitted, not advisory.
/// Proves the refactor preserved the existing deferral path.
#[test]
fn test_service_render_plan_present_package_deferred_to_config_tree() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["timer-pkg".into()]),
        packages_added: vec![],
        no_baseline: false,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "timer-pkg.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("timer-pkg"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });
    snap.scheduled_tasks = Some(inspectah_core::types::scheduled::ScheduledTaskSection {
        systemd_timers: vec![inspectah_core::types::scheduled::SystemdTimer {
            name: "timer-pkg".into(),
            source: "local".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    let plan = render_service_intent(&snap);

    assert!(
        plan.omissions.is_empty(),
        "present package must not be omitted"
    );
    assert!(
        plan.advisories.is_empty(),
        "present package must not get advisory"
    );
    // Should be deferred, not in systemctl enable
    assert!(
        plan.lines
            .iter()
            .all(|l| !l.contains("systemctl enable") || !l.contains("timer-pkg")),
        "deferred service must not appear in systemctl enable"
    );
    assert!(
        plan.lines
            .iter()
            .any(|l| l.contains("deferred") && l.contains("timer-pkg")),
        "deferred service must appear in deferred comment"
    );
}

/// Duplicate same-name packages: one excluded, one included+installable.
/// The included entry should win — package is present, no advisory.
#[test]
fn test_service_render_plan_duplicate_package_uses_best_entry() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec![]),
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: false, // excluded entry
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "httpd".into(),
                arch: "i686".into(),
                state: PackageState::Added,
                include: true, // included entry — should win
                source_repo: "appstream".into(),
                ..Default::default()
            },
        ],
        no_baseline: false,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "httpd.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("httpd"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    assert!(
        plan.omissions.is_empty(),
        "included entry should make package present"
    );
    assert!(
        plan.advisories.is_empty(),
        "included+installable entry should emit clean"
    );
    assert!(plan.lines.iter().any(|l| l.contains("httpd.service")));
}

/// Fleet mode: `packages_added` is leaf-only, so `classify_service_presence`
/// would incorrectly omit services owned by auto (non-leaf) packages via
/// Tier 7. In fleet mode, skip classification entirely — all services Emit,
/// no omissions, no package-derived advisories.
#[test]
fn test_fleet_snapshot_skips_service_omission_and_advisories() {
    use inspectah_core::types::fleet::FleetSnapshotMeta;

    let mut snap = InspectionSnapshot::new();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "test-fleet".into(),
        host_count: 2,
        hostnames: vec!["alpha".into(), "beta".into()],
        merged_at: "2026-06-09T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: Default::default(),
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "git".into(),
            arch: "x86_64".into(),
            include: true,
            ..Default::default()
        }],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        baseline_package_names: Some(vec!["systemd".into()]),
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "perl-related.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("perl-libs"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    // Fleet: no omissions, no advisories, service must be emitted
    assert!(plan.omissions.is_empty(), "fleet must not omit services");
    assert!(
        plan.advisories.is_empty(),
        "fleet must not emit package-derived advisories"
    );
    assert!(
        plan.lines
            .iter()
            .any(|l| l.contains("perl-related.service")),
        "fleet must emit perl-related.service"
    );
}
