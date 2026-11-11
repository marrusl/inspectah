use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::aggregate::AggregateSnapshotMeta;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::services::{
    PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
};
use inspectah_pipeline::render::comment::LINE_TERMINATORS;
use inspectah_pipeline::render::containerfile::render_containerfile;
use inspectah_pipeline::render::not_carried::apply_scan_package_defaults;
use inspectah_pipeline::render::not_carried::not_carried_packages;
use inspectah_pipeline::render::service_intent::{
    AdvisoryReason, OmissionReason, effective_target_packages, is_package_installable,
    pulled_in_by_included, render_service_intent,
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

// ---------------------------------------------------------------------------
// Not carried: the omission reason for an auto package nothing needs
// ---------------------------------------------------------------------------

const RHC_NOT_CARRIED: &str = "package 'rhc' is not carried: not user-installed on the source host, and nothing this image installs needs it";

#[test]
fn not_carried_describes_the_class_in_the_shared_wording() {
    assert_eq!(OmissionReason::NotCarried.describe("rhc"), RHC_NOT_CARRIED);
}

fn added(name: &str, arch: &str, disposition: FindingKind) -> PackageEntry {
    PackageEntry {
        name: name.into(),
        arch: arch.into(),
        state: PackageState::Added,
        disposition,
        source_repo: "appstream".into(),
        ..Default::default()
    }
}

/// A scan snapshot with full leaf data: `packages` are every added entry
/// (all included, as the collector records them), `leaves` and `autos` are
/// `name.arch` ids, and `tree` is the leaf dependency tree. The baseline
/// ships none of them.
fn leaf_snapshot(
    packages: Vec<PackageEntry>,
    leaves: &[&str],
    autos: &[&str],
    tree: serde_json::Value,
    changes: Vec<ServiceStateChange>,
) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["bash".into()]),
        packages_added: packages,
        leaf_packages: Some(leaves.iter().map(|s| s.to_string()).collect()),
        auto_packages: Some(autos.iter().map(|s| s.to_string()).collect()),
        leaf_dep_tree: tree,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: changes,
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });
    snap
}

/// The unit is in no `systemctl` line of the plan.
fn acted_on(lines: &[String], unit: &str) -> bool {
    lines
        .iter()
        .any(|l| !l.starts_with('#') && l.split_whitespace().any(|w| w == unit))
}

/// Units owned by an auto package nothing included pulls in are omitted on
/// the scan path as not carried, whatever the action: enable, disable or
/// mask.
#[test]
fn units_of_a_not_carried_package_are_omitted_as_not_carried_on_the_scan_path() {
    let cases = [
        (
            "rhcd.service",
            "rhc",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
        ),
        (
            "firewalld.service",
            "firewalld",
            ServiceUnitState::Disabled,
            Some(PresetDefault::Enable),
        ),
        (
            "kdump.service",
            "kexec-tools",
            ServiceUnitState::Masked,
            None,
        ),
    ];
    for (unit, owner, state, preset) in cases {
        let mut snap = leaf_snapshot(
            vec![
                added("vim", "x86_64", FindingKind::included()),
                added(owner, "x86_64", FindingKind::included()),
            ],
            &["vim.x86_64"],
            &[&format!("{owner}.x86_64")],
            serde_json::json!({ "vim.x86_64": [] }),
            vec![state_change(unit, state, preset, Some(owner))],
        );
        apply_scan_package_defaults(&mut snap);

        let plan = render_service_intent(&snap);
        assert!(!acted_on(&plan.lines, unit), "{unit}: {:?}", plan.lines);
        assert_eq!(plan.omissions.len(), 1, "{unit}");
        assert_eq!(
            plan.omissions[0].reason,
            OmissionReason::NotCarried,
            "{unit}"
        );
        let comment = format!(
            "# Omitted: {unit} (package '{owner}' is not carried: not user-installed on the source host, and nothing this image installs needs it)"
        );
        assert!(
            plan.lines.contains(&comment),
            "{unit}: missing {comment:?} in {:?}",
            plan.lines
        );
    }
}

#[test]
fn rhcd_renders_the_exact_not_carried_comment() {
    let mut snap = leaf_snapshot(
        vec![
            added("vim", "x86_64", FindingKind::included()),
            added("rhc", "x86_64", FindingKind::included()),
        ],
        &["vim.x86_64"],
        &["rhc.x86_64"],
        serde_json::json!({ "vim.x86_64": [] }),
        vec![state_change(
            "rhcd.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("rhc"),
        )],
    );
    apply_scan_package_defaults(&mut snap);
    let plan = render_service_intent(&snap);
    assert!(
        plan.lines
            .contains(&format!("# Omitted: rhcd.service ({RHC_NOT_CARRIED})")),
        "{:?}",
        plan.lines
    );
}

/// The presence rules decide omission, not the list: `foo.i686` is not
/// carried, but an included leaf pulls in `foo.x86_64`, so `foo` is in the
/// image and its unit stays.
#[test]
fn a_not_carried_arch_does_not_omit_a_unit_another_arch_carries() {
    let mut snap = leaf_snapshot(
        vec![
            added("app", "x86_64", FindingKind::included()),
            added("foo", "x86_64", FindingKind::included()),
            added("foo", "i686", FindingKind::included()),
        ],
        &["app.x86_64"],
        &["foo.x86_64", "foo.i686"],
        serde_json::json!({ "app.x86_64": ["foo.x86_64"] }),
        vec![state_change(
            "foo.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("foo"),
        )],
    );
    apply_scan_package_defaults(&mut snap);
    let listed: Vec<String> = inspectah_pipeline::render::not_carried::not_carried_packages(&snap)
        .iter()
        .map(|p| format!("{}.{}", p.name, p.arch))
        .collect();
    assert_eq!(
        listed,
        vec!["foo.i686".to_string()],
        "the fixture's premise"
    );

    let plan = render_service_intent(&snap);
    assert!(plan.omissions.is_empty(), "{:?}", plan.omissions);
    assert!(acted_on(&plan.lines, "foo.service"), "{:?}", plan.lines);
}

/// Excluding a leaf makes its auto dependency unneeded, so the
/// dependency's unit is omitted as not carried, not as excluded: nothing
/// included needs the package, which is why it is absent. The leaf's own
/// unit keeps `PackageExcluded`, since a leaf is outside the class.
#[test]
fn an_excluded_leaf_makes_its_dependency_not_carried_and_stays_excluded_itself() {
    let mut snap = leaf_snapshot(
        vec![
            added("cockpit", "x86_64", FindingKind::excluded()),
            added("cockpit-ws", "x86_64", FindingKind::included()),
        ],
        &["cockpit.x86_64"],
        &["cockpit-ws.x86_64"],
        serde_json::json!({ "cockpit.x86_64": ["cockpit-ws.x86_64"] }),
        vec![
            state_change(
                "cockpit.socket",
                ServiceUnitState::Enabled,
                Some(PresetDefault::Disable),
                Some("cockpit-ws"),
            ),
            state_change(
                "cockpit-leaf.service",
                ServiceUnitState::Enabled,
                Some(PresetDefault::Disable),
                Some("cockpit"),
            ),
        ],
    );
    apply_scan_package_defaults(&mut snap);
    let plan = render_service_intent(&snap);

    let reasons: Vec<(&str, OmissionReason)> = plan
        .omissions
        .iter()
        .map(|o| (o.unit.as_str(), o.reason))
        .collect();
    assert_eq!(
        reasons,
        vec![
            ("cockpit.socket", OmissionReason::NotCarried),
            ("cockpit-leaf.service", OmissionReason::PackageExcluded),
        ]
    );
    assert!(plan.lines.contains(&"# Omitted: cockpit.socket (package 'cockpit-ws' is not carried: not user-installed on the source host, and nothing this image installs needs it)".to_string()));
    assert!(
        plan.lines.contains(
            &"# Omitted: cockpit-leaf.service (package 'cockpit' was excluded)".to_string()
        )
    );
}

/// `NotCarried` needs every entry of the owner's name in the list. An
/// operator-excluded leaf of the same name is outside the class, so the
/// reason stays `PackageExcluded`.
#[test]
fn an_owner_with_an_entry_outside_the_list_stays_excluded() {
    let mut snap = leaf_snapshot(
        vec![
            added("vim", "x86_64", FindingKind::included()),
            added("foo", "x86_64", FindingKind::included()),
            added("foo", "i686", FindingKind::excluded()),
        ],
        &["vim.x86_64", "foo.i686"],
        &["foo.x86_64"],
        serde_json::json!({ "vim.x86_64": [], "foo.i686": [] }),
        vec![state_change(
            "foo.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("foo"),
        )],
    );
    apply_scan_package_defaults(&mut snap);
    let plan = render_service_intent(&snap);
    assert_eq!(plan.omissions.len(), 1, "{:?}", plan.lines);
    assert_eq!(plan.omissions[0].reason, OmissionReason::PackageExcluded);
}

/// An auto package an included leaf pulls in is in the image: its unit
/// keeps its `systemctl` line, excluded or not.
#[test]
fn a_pulled_in_auto_package_keeps_its_unit() {
    for disposition in [FindingKind::included(), FindingKind::excluded()] {
        let mut snap = leaf_snapshot(
            vec![
                added("cockpit", "x86_64", FindingKind::included()),
                added("cockpit-ws", "x86_64", disposition.clone()),
            ],
            &["cockpit.x86_64"],
            &["cockpit-ws.x86_64"],
            serde_json::json!({ "cockpit.x86_64": ["cockpit-ws.x86_64"] }),
            vec![state_change(
                "cockpit.socket",
                ServiceUnitState::Enabled,
                Some(PresetDefault::Disable),
                Some("cockpit-ws"),
            )],
        );
        apply_scan_package_defaults(&mut snap);
        let plan = render_service_intent(&snap);
        assert!(
            plan.omissions.is_empty(),
            "{disposition:?}: {:?}",
            plan.lines
        );
        assert!(acted_on(&plan.lines, "cockpit.socket"), "{:?}", plan.lines);
    }
}

// ---------------------------------------------------------------------------
// Which leaves contribute to `pulled_in_by_included`
// ---------------------------------------------------------------------------

/// The annotation the scan's dnf-cache pass writes on a repo-less package.
const REPOLESS_NOTE: &str = "No repo source";

/// An included leaf the export renders as a manual step is never
/// installed, so dnf never resolves its dependencies: they are not pulled
/// in. A leaf the export does install, on whatever route, pulls them in.
#[test]
fn only_a_leaf_the_export_installs_pulls_in_its_dependencies() {
    fn repoless(pkg: &mut PackageEntry) {
        pkg.source_repo = "@commandline".into();
        pkg.repoless_annotation = REPOLESS_NOTE.into();
    }
    type Root = fn(&mut PackageEntry);
    let cases: [(&str, Root, bool); 8] = [
        ("installable by name", |_| {}, true),
        (
            "repo-less with a cached RPM",
            |pkg| {
                repoless(pkg);
                pkg.repoless_cached = true;
            },
            true,
        ),
        (
            "repo-less with an uploaded RPM",
            |pkg| {
                repoless(pkg);
                pkg.uploaded = true;
            },
            true,
        ),
        ("repo-less with no RPM (MANUAL)", repoless, false),
        (
            "installed from a local file, no repo (TODO)",
            |pkg| pkg.state = PackageState::LocalInstall,
            false,
        ),
        (
            "repo-enabling, serving repo dropped (FIXME)",
            |pkg| {
                pkg.source_repo = "@commandline".into();
                pkg.repo_enabling_from = vec![inspectah_core::types::repo::RepoId::new("epel")];
            },
            false,
        ),
        (
            "repo-enabling, serving repo dropped, uploaded",
            |pkg| {
                pkg.source_repo = "@commandline".into();
                pkg.repo_enabling_from = vec![inspectah_core::types::repo::RepoId::new("epel")];
                pkg.repoless_annotation = REPOLESS_NOTE.into();
                pkg.uploaded = true;
            },
            true,
        ),
        (
            "excluded",
            |pkg| pkg.disposition = FindingKind::excluded(),
            false,
        ),
    ];

    for (root_kind, make_root, pulls_in) in cases {
        let mut root = added("vendor-app", "x86_64", FindingKind::included());
        make_root(&mut root);
        let rpm = RpmSection {
            packages_added: vec![root, added("vendor-lib", "x86_64", FindingKind::excluded())],
            leaf_packages: Some(vec!["vendor-app.x86_64".into()]),
            auto_packages: Some(vec!["vendor-lib.x86_64".into()]),
            leaf_dep_tree: serde_json::json!({ "vendor-app.x86_64": ["vendor-lib.x86_64"] }),
            ..Default::default()
        };
        assert_eq!(
            pulled_in_by_included(&rpm).contains("vendor-lib.x86_64"),
            pulls_in,
            "{root_kind}"
        );
    }
}

/// The scan path's half of the case seen on a RHEL 10 host: an included
/// leaf the scan renders as MANUAL (`epel-release`) used to make its only
/// dependency (`dnf-plugins-core`) read as pulled in, so the scan neither
/// installed nor listed it. It is now not carried, and its unit is
/// omitted as such.
#[test]
fn a_dependency_of_a_manual_only_leaf_is_not_carried() {
    let mut epel = added("epel-release", "noarch", FindingKind::included());
    epel.source_repo = "@commandline".into();
    epel.repoless_annotation = REPOLESS_NOTE.into();
    let mut snap = leaf_snapshot(
        vec![
            epel,
            added("dnf-plugins-core", "noarch", FindingKind::included()),
        ],
        &["epel-release.noarch"],
        &["dnf-plugins-core.noarch"],
        serde_json::json!({ "epel-release.noarch": ["dnf-plugins-core.noarch"] }),
        vec![state_change(
            "dnf-plugins-core.timer",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("dnf-plugins-core"),
        )],
    );
    apply_scan_package_defaults(&mut snap);

    let listed: Vec<String> = not_carried_packages(&snap)
        .into_iter()
        .map(|pkg| pkg.name)
        .collect();
    assert_eq!(listed, ["dnf-plugins-core"]);
    let plan = render_service_intent(&snap);
    assert_eq!(plan.omissions.len(), 1, "{:?}", plan.lines);
    assert_eq!(plan.omissions[0].reason, OmissionReason::NotCarried);
    assert!(!acted_on(&plan.lines, "dnf-plugins-core.timer"));
}

// ---------------------------------------------------------------------------
// A hostile owning package stays inside the `# Omitted:` comment
// ---------------------------------------------------------------------------

/// The owning package comes from the snapshot unchecked, and an imported
/// one can hold any bytes. For every omission reason and every terminator
/// in `LINE_TERMINATORS`, and CRLF, the owner must not end the `# Omitted:`
/// comment: the comment stays one line, no terminator but the newline the
/// file is joined with reaches the Containerfile, and every line that
/// carries the payload, split on every terminator, is a comment. The plan's structured omission keeps
/// the raw name, since refine escapes it for its own surface.
#[test]
fn omitted_comment_keeps_a_hostile_owner_inside_the_comment() {
    const RUN: &str = "RUN touch /tmp/injected";

    let separators = LINE_TERMINATORS
        .iter()
        .map(char::to_string)
        .chain(std::iter::once("\r\n".to_string()));
    for sep in separators {
        let owner = format!("x{sep}{RUN}{sep}#");
        let change = || {
            state_change(
                "unit.service",
                ServiceUnitState::Enabled,
                Some(PresetDefault::Disable),
                Some(owner.as_str()),
            )
        };
        let services = || ServiceSection {
            state_changes: vec![change()],
            enabled_units: vec![],
            disabled_units: vec![],
            drop_ins: vec![],
            preset_matched_units: vec![],
        };

        let mut excluded = InspectionSnapshot::new();
        excluded.rpm = Some(RpmSection {
            baseline_package_names: Some(vec![]),
            packages_added: vec![added(&owner, "x86_64", FindingKind::excluded())],
            ..Default::default()
        });
        excluded.services = Some(services());

        let mut not_in_target = InspectionSnapshot::new();
        not_in_target.rpm = Some(RpmSection {
            baseline_package_names: Some(vec![]),
            ..Default::default()
        });
        not_in_target.services = Some(services());

        let mut not_carried = leaf_snapshot(
            vec![
                added("vim", "x86_64", FindingKind::included()),
                added(&owner, "x86_64", FindingKind::included()),
            ],
            &["vim.x86_64"],
            &[&format!("{owner}.x86_64")],
            serde_json::json!({ "vim.x86_64": [] }),
            vec![change()],
        );
        apply_scan_package_defaults(&mut not_carried);

        for (reason, snap) in [
            (OmissionReason::PackageExcluded, excluded),
            (OmissionReason::NotInTargetImage, not_in_target),
            (OmissionReason::NotCarried, not_carried),
        ] {
            let label = format!("{reason:?}, {sep:?}");
            let plan = render_service_intent(&snap);
            assert_eq!(plan.omissions.len(), 1, "{label}: {:?}", plan.lines);
            assert_eq!(plan.omissions[0].reason, reason, "{label}");
            assert_eq!(
                plan.omissions[0].owning_package, owner,
                "{label}: the structured omission keeps the raw name"
            );

            let comments: Vec<&String> = plan
                .lines
                .iter()
                .filter(|l| l.starts_with("# Omitted:"))
                .collect();
            assert_eq!(comments.len(), 1, "{label}: {:?}", plan.lines);
            assert!(
                !comments[0].contains(LINE_TERMINATORS),
                "{label}: the comment holds a line terminator: {:?}",
                comments[0]
            );
            assert!(
                comments[0].contains(RUN),
                "{label}: the payload is listed, escaped: {:?}",
                comments[0]
            );

            let output = render_containerfile(&snap, None, None);
            let terminators: Vec<char> = sep.chars().filter(|c| *c != '\n').collect();
            assert!(
                !output.contains(terminators.as_slice()),
                "{label}: the terminator reached the Containerfile:\n{output}"
            );
            assert!(
                output
                    .split(LINE_TERMINATORS)
                    .filter(|l| l.contains("injected"))
                    .all(|l| l.trim_start().starts_with('#')),
                "{label}: the payload left the comment:\n{output}"
            );
        }
    }
}
