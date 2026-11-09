use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::aggregate::AggregateSnapshotMeta;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::services::{
    PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
};
use inspectah_pipeline::render::service_intent::{
    AdvisoryReason, OmissionReason, effective_target_packages, is_package_installable,
    render_service_intent,
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
        disposition: FindingKind::included(),
        locked: false,
        owning_package: owning_package.map(str::to_string),
        aggregate: None,
        attention_reason: None,
        shadow_type: None,
        shadow_rationale: None,
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
                disposition: FindingKind::included(),
                locked: false,
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "excluded-app".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                disposition: FindingKind::excluded(),
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
        disposition: FindingKind::included(),
        locked: false,
        source_repo: "appstream".into(),
        ..Default::default()
    };
    let local = PackageEntry {
        name: "local-tool".into(),
        arch: "x86_64".into(),
        state: PackageState::LocalInstall,
        disposition: FindingKind::included(),
        locked: false,
        source_repo: String::new(),
        ..Default::default()
    };
    let empty_repo = PackageEntry {
        name: "mystery".into(),
        arch: "x86_64".into(),
        state: PackageState::Added,
        disposition: FindingKind::included(),
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
            disposition: FindingKind::included(),
            locked: false,
            source_repo: String::new(),
            ..Default::default()
        }],

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
            disposition: FindingKind::included(),
            locked: false,
            source_repo: "appstream".into(),
            ..Default::default()
        }],

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

/// Present package in config-tree timer → deferred, not omitted, not advisory.
/// Proves the refactor preserved the existing deferral path.
#[test]
fn test_service_render_plan_present_package_deferred_to_config_tree() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["timer-pkg".into()]),
        packages_added: vec![],

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
                disposition: FindingKind::excluded(), // excluded entry
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "httpd".into(),
                arch: "i686".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(), // included entry — should win
                source_repo: "appstream".into(),
                ..Default::default()
            },
        ],

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

/// Aggregate mode: `packages_added` is leaf-only, so `classify_service_presence`
/// would incorrectly omit services owned by auto (non-leaf) packages via
/// Tier 7. In aggregate mode only Tier 2 applies, which this owner (absent
/// from `packages_added`) does not reach, so the unit emits with no
/// omission and no package-derived advisory. The tier table pins Tier 2.
#[test]
fn test_aggregate_snapshot_skips_service_omission_and_advisories() {
    let mut snap = InspectionSnapshot::new();
    snap.aggregate_meta = Some(AggregateSnapshotMeta {
        label: "test-aggregate".into(),
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
            disposition: FindingKind::included(),
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

    // Aggregate: no omissions, no advisories, service must be emitted
    assert!(
        plan.omissions.is_empty(),
        "aggregate must not omit services"
    );
    assert!(
        plan.advisories.is_empty(),
        "aggregate must not emit package-derived advisories"
    );
    assert!(
        plan.lines
            .iter()
            .any(|l| l.contains("perl-related.service")),
        "aggregate must emit perl-related.service"
    );
}

/// The input this workstream exists for: a vendor RPM installed by local
/// path that ships a systemd unit. `owning_package` is set, `source_repo`
/// is `@commandline`, and `repo_enabling_from` is empty because a vendor
/// agent is not repo-enabling. It was tier 4 (proven present, clean emit)
/// before the repo-enabling work, because a non-empty `source_repo` was
/// enough on its own; it is tier 3 now, which is the conservative answer.
///
/// The paired positive is the same entry with a serving repo recorded: a
/// repo-enabling `@commandline` package *is* installable by name, so it
/// stays tier 4. Without it this test would pass against a blanket
/// refusal of every `@commandline` package.
#[test]
fn a_commandline_package_owning_a_unit_is_unreachable_unless_a_repo_serves_it() {
    let snapshot_with = |repo_enabling_from: Vec<inspectah_core::types::repo::RepoId>| {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            baseline_package_names: Some(vec!["firewalld".into()]),
            packages_added: vec![PackageEntry {
                name: "vendor-agent".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(),
                locked: false,
                source_repo: "@commandline".into(),
                repo_enabling_from,
                ..Default::default()
            }],
            ..Default::default()
        });
        snap.services = Some(ServiceSection {
            state_changes: vec![state_change(
                "vendor-agent.service",
                ServiceUnitState::Enabled,
                Some(PresetDefault::Disable),
                Some("vendor-agent"),
            )],
            enabled_units: vec![],
            disabled_units: vec![],
            drop_ins: vec![],
            preset_matched_units: vec![],
        });
        snap
    };

    let plan = render_service_intent(&snapshot_with(Vec::new()));
    assert!(plan.omissions.is_empty(), "the unit is still emitted");
    assert_eq!(plan.advisories.len(), 1, "{:?}", plan.advisories);
    assert_eq!(plan.advisories[0].unit, "vendor-agent.service");
    assert_eq!(
        plan.advisories[0].reasons,
        vec![AdvisoryReason::PackageUnreachable],
        "a locally installed vendor RPM is not reachable by name"
    );

    let served = render_service_intent(&snapshot_with(vec![
        inspectah_core::types::repo::RepoId::new("vendor"),
    ]));
    assert!(
        served.advisories.is_empty(),
        "a repo serves it by name, so it is proven present: {:?}",
        served.advisories
    );
    assert!(served.omissions.is_empty());
}

/// What the decision engine should do with one service.
#[derive(Debug, PartialEq)]
enum Expected {
    Omit(OmissionReason, &'static str),
    Advise(Vec<AdvisoryReason>),
    Clean,
}

/// Every tier of `classify_service_presence` in one table. Tier 2 (all
/// entries for the owning package excluded) omits the unit and records the
/// exclusion as the reason; the other tiers are pinned to show the change
/// left them alone.
#[test]
fn test_service_presence_tiers_table() {
    fn pkg(name: &str, disposition: FindingKind, state: PackageState) -> PackageEntry {
        PackageEntry {
            name: name.into(),
            arch: "x86_64".into(),
            state,
            disposition,
            source_repo: "appstream".into(),
            ..Default::default()
        }
    }
    let excluded = || FindingKind::excluded();
    let included = || FindingKind::included();

    struct Case {
        name: &'static str,
        baseline: Option<Vec<&'static str>>,
        added: Vec<PackageEntry>,
        owner: Option<&'static str>,
        expected: Expected,
        /// The same case on an aggregate snapshot, where only Tier 2 is
        /// applied and every other tier emits clean.
        in_aggregate: Expected,
        /// `Some(parent_included)`: a leaf `parent` whose dependency tree
        /// lists the owner, included or not.
        pulled_in_by: Option<bool>,
    }
    let cases = vec![
        Case {
            name: "tier 1: unknown owner emits clean",
            baseline: Some(vec![]),
            added: vec![],
            owner: None,
            expected: Expected::Clean,
            pulled_in_by: None,
            in_aggregate: Expected::Clean,
        },
        Case {
            name: "tier 2: excluded package omits with excluded reason",
            baseline: Some(vec![]),
            added: vec![pkg("nginx", excluded(), PackageState::Added)],
            owner: Some("nginx"),
            expected: Expected::Omit(
                OmissionReason::PackageExcluded,
                "# Omitted: unit.service (package 'nginx' was excluded)",
            ),
            pulled_in_by: None,
            in_aggregate: Expected::Omit(
                OmissionReason::PackageExcluded,
                "# Omitted: unit.service (package 'nginx' was excluded)",
            ),
        },
        Case {
            name: "tier 2: excluded package, baseline unavailable, still omits",
            baseline: None,
            added: vec![pkg("nginx", excluded(), PackageState::Added)],
            owner: Some("nginx"),
            expected: Expected::Omit(
                OmissionReason::PackageExcluded,
                "# Omitted: unit.service (package 'nginx' was excluded)",
            ),
            pulled_in_by: None,
            in_aggregate: Expected::Omit(
                OmissionReason::PackageExcluded,
                "# Omitted: unit.service (package 'nginx' was excluded)",
            ),
        },
        Case {
            name: "tier 2: excluded entry but baseline ships the package emits clean",
            baseline: Some(vec!["nginx"]),
            added: vec![pkg("nginx", excluded(), PackageState::Added)],
            owner: Some("nginx"),
            expected: Expected::Clean,
            pulled_in_by: None,
            in_aggregate: Expected::Clean,
        },
        Case {
            name: "tier 3: included but not installable advises unreachable",
            baseline: Some(vec![]),
            added: vec![pkg("app", included(), PackageState::LocalInstall)],
            owner: Some("app"),
            expected: Expected::Advise(vec![AdvisoryReason::PackageUnreachable]),
            pulled_in_by: None,
            in_aggregate: Expected::Clean,
        },
        Case {
            name: "tier 3: unreachable stacks baseline unavailable",
            baseline: None,
            added: vec![pkg("app", included(), PackageState::LocalInstall)],
            owner: Some("app"),
            expected: Expected::Advise(vec![
                AdvisoryReason::PackageUnreachable,
                AdvisoryReason::BaselineUnavailable,
            ]),
            pulled_in_by: None,
            in_aggregate: Expected::Clean,
        },
        Case {
            name: "tier 4: included and installable emits clean",
            baseline: Some(vec![]),
            added: vec![pkg("httpd", included(), PackageState::Added)],
            owner: Some("httpd"),
            expected: Expected::Clean,
            pulled_in_by: None,
            in_aggregate: Expected::Clean,
        },
        Case {
            name: "tier 5: baseline package emits clean",
            baseline: Some(vec!["sshd"]),
            added: vec![],
            owner: Some("sshd"),
            expected: Expected::Clean,
            pulled_in_by: None,
            in_aggregate: Expected::Clean,
        },
        Case {
            name: "tier 6: unknown package, no baseline, advises",
            baseline: None,
            added: vec![],
            owner: Some("mystery"),
            expected: Expected::Advise(vec![AdvisoryReason::BaselineUnavailable]),
            pulled_in_by: None,
            in_aggregate: Expected::Clean,
        },
        Case {
            name: "tier 7: unknown package with baseline omits as not in target",
            baseline: Some(vec![]),
            added: vec![],
            owner: Some("sssd"),
            expected: Expected::Omit(
                OmissionReason::NotInTargetImage,
                "# Omitted: unit.service (package 'sssd' not in target image)",
            ),
            pulled_in_by: None,
            in_aggregate: Expected::Clean,
        },
        Case {
            name: "tier 2: an excluded dependency of an included leaf is present",
            baseline: Some(vec![]),
            added: vec![pkg("cockpit-ws", excluded(), PackageState::Added)],
            owner: Some("cockpit-ws"),
            expected: Expected::Clean,
            pulled_in_by: Some(true),
            in_aggregate: Expected::Clean,
        },
        Case {
            name: "tier 2: an included dependency of an included leaf emits clean",
            baseline: Some(vec![]),
            added: vec![pkg("cockpit-ws", included(), PackageState::Added)],
            owner: Some("cockpit-ws"),
            expected: Expected::Clean,
            pulled_in_by: Some(true),
            in_aggregate: Expected::Clean,
        },
        Case {
            name: "tier 2: an excluded dependency of an excluded leaf omits",
            baseline: Some(vec![]),
            added: vec![pkg("cockpit-ws", excluded(), PackageState::Added)],
            owner: Some("cockpit-ws"),
            expected: Expected::Omit(
                OmissionReason::PackageExcluded,
                "# Omitted: unit.service (package 'cockpit-ws' was excluded)",
            ),
            pulled_in_by: Some(false),
            in_aggregate: Expected::Omit(
                OmissionReason::PackageExcluded,
                "# Omitted: unit.service (package 'cockpit-ws' was excluded)",
            ),
        },
    ];

    for (action_state, preset) in [
        (ServiceUnitState::Enabled, PresetDefault::Disable),
        (ServiceUnitState::Disabled, PresetDefault::Enable),
    ] {
        for (case, aggregate) in cases
            .iter()
            .flat_map(|c| [(c, false), (c, true)].into_iter())
        {
            let mut snap = InspectionSnapshot::new();
            if aggregate {
                snap.aggregate_meta = Some(AggregateSnapshotMeta {
                    label: "tiers".into(),
                    host_count: 3,
                    hostnames: vec!["a".into(), "b".into(), "c".into()],
                    merged_at: "2026-11-08T00:00:00Z".into(),
                    baseline_provisional: false,
                    section_host_counts: Default::default(),
                });
            }
            let (expected, mode) = if aggregate {
                (&case.in_aggregate, "aggregate")
            } else {
                (&case.expected, "single host")
            };
            let label = format!("{} ({mode})", case.name);
            let mut added = case.added.clone();
            let mut leaf_dep_tree = serde_json::json!({});
            if let (Some(parent_included), Some(owner)) = (case.pulled_in_by, case.owner) {
                added.push(pkg(
                    "parent",
                    FindingKind::from_bool(parent_included),
                    PackageState::Added,
                ));
                leaf_dep_tree = serde_json::json!({ "parent.x86_64": [format!("{owner}.x86_64")] });
            }
            snap.rpm = Some(RpmSection {
                baseline_package_names: case
                    .baseline
                    .as_ref()
                    .map(|b| b.iter().map(|s| s.to_string()).collect()),
                packages_added: added,
                leaf_dep_tree,
                ..Default::default()
            });
            snap.services = Some(ServiceSection {
                state_changes: vec![state_change(
                    "unit.service",
                    action_state,
                    Some(preset),
                    case.owner,
                )],
                enabled_units: vec![],
                disabled_units: vec![],
                drop_ins: vec![],
                preset_matched_units: vec![],
            });

            let plan = render_service_intent(&snap);
            let in_systemctl = plan
                .lines
                .iter()
                .any(|l| !l.starts_with('#') && l.contains("unit.service"));

            match expected {
                Expected::Omit(reason, comment) => {
                    assert_eq!(plan.omissions.len(), 1, "{}", label);
                    assert_eq!(plan.omissions[0].reason, *reason, "{}", label);
                    assert!(
                        plan.lines.iter().any(|l| l == comment),
                        "{}: missing {comment:?} in {:?}",
                        label,
                        plan.lines
                    );
                    assert!(!in_systemctl, "{}: omitted unit still acted on", label);
                    assert!(plan.advisories.is_empty(), "{}", label);
                }
                Expected::Advise(reasons) => {
                    assert!(plan.omissions.is_empty(), "{}", label);
                    assert_eq!(plan.advisories.len(), 1, "{}", label);
                    assert_eq!(&plan.advisories[0].reasons, reasons, "{}", label);
                    assert!(in_systemctl, "{}: advised unit must stay", label);
                }
                Expected::Clean => {
                    assert!(plan.omissions.is_empty(), "{}", label);
                    assert!(plan.advisories.is_empty(), "{}", label);
                    assert!(in_systemctl, "{}: unit must be acted on", label);
                }
            }
        }
    }
}
