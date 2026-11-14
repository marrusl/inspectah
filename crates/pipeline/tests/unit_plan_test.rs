use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::aggregate::AggregateSnapshotMeta;
use inspectah_core::types::config::{ConfigCategory, ConfigFileEntry, ConfigSection};
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::units::{
    CarriedUnit, PresetDefault, UnitAction, UnitFragment, UnitName, UnitSection, UnitState,
    UnitStateChange, UnitSubject,
};
use inspectah_pipeline::render::comment::LINE_TERMINATORS;
use inspectah_pipeline::render::configtree::staged_writes;
use inspectah_pipeline::render::containerfile::render_containerfile;
use inspectah_pipeline::render::not_carried::apply_scan_package_defaults;
use inspectah_pipeline::render::not_carried::not_carried_packages;
use inspectah_pipeline::render::unit_plan::{
    AdvisoryReason, OmissionReason, ServiceAdvisory, effective_target_packages,
    is_package_installable, plan_units, pulled_in_by_included, unit_state_lines,
};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// What the presence tests read from a plan: the unit state section's
/// lines, and the declined commands and advisories.
struct Rendered {
    lines: Vec<String>,
    omissions: Vec<Omission>,
    advisories: Vec<ServiceAdvisory>,
}

/// One declined command, by unit name, with the owning package a presence
/// reason names (empty for a reason about the export's files).
#[derive(Debug)]
struct Omission {
    unit: String,
    owning_package: String,
    reason: OmissionReason,
}

/// `plan_units` for `snap` against the files its own export stages, and
/// the section's lines.
fn render(snap: &InspectionSnapshot) -> Rendered {
    let plan = plan_units(snap, &staged_writes(snap));
    Rendered {
        lines: unit_state_lines(&plan),
        omissions: plan
            .declined
            .iter()
            .map(|(name, _, reason)| Omission {
                unit: name.as_str().to_owned(),
                owning_package: reason.owning_package().unwrap_or_default().to_owned(),
                reason: reason.clone(),
            })
            .collect(),
        advisories: plan.advisories,
    }
}

fn state_change(
    unit: &str,
    current_state: UnitState,
    default_state: Option<PresetDefault>,
    owning_package: Option<&str>,
) -> UnitStateChange {
    UnitStateChange::new(
        test_carried_unit(
            unit.to_owned().try_into().unwrap(),
            owning_package.map(str::to_string),
        ),
        current_state,
        default_state,
    )
    .unwrap()
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
    snap.units = Some(UnitSection {
        state_changes: vec![state_change(
            "sssd-kcm.service",
            UnitState::Disabled,
            Some(PresetDefault::Enable),
            Some("sssd"),
        )],
        inventory: vec![],
        drop_ins: vec![],
        mount_findings: Vec::new(),
    });

    let plan = render(&snap);

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
    snap.units = Some(UnitSection {
        state_changes: vec![state_change(
            "local-tool.service",
            UnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("local-tool"),
        )],
        inventory: vec![],
        drop_ins: vec![],
        mount_findings: Vec::new(),
    });

    let plan = render(&snap);

    assert!(plan.omissions.is_empty());
    assert_eq!(plan.advisories.len(), 1);
    assert_eq!(plan.advisories[0].unit.as_str(), "local-tool.service");
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
    snap.units = Some(UnitSection {
        state_changes: vec![state_change(
            "mystery.service",
            UnitState::Enabled,
            Some(PresetDefault::Disable),
            None, // no owning package
        )],
        inventory: vec![],
        drop_ins: vec![],
        mount_findings: Vec::new(),
    });

    let plan = render(&snap);

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
    snap.units = Some(UnitSection {
        state_changes: vec![state_change(
            "absent-timer.service",
            UnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("absent-pkg"),
        )],
        inventory: vec![],
        drop_ins: vec![],
        mount_findings: Vec::new(),
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

    let plan = render(&snap);

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
    snap.units = Some(UnitSection {
        state_changes: vec![
            state_change(
                "firewalld.service",
                UnitState::Enabled,
                Some(PresetDefault::Disable),
                Some("firewalld"),
            ),
            state_change(
                "httpd.service",
                UnitState::Enabled,
                Some(PresetDefault::Disable),
                Some("httpd"),
            ),
        ],
        inventory: vec![],
        drop_ins: vec![],
        mount_findings: Vec::new(),
    });

    let plan = render(&snap);

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
    snap.units = Some(UnitSection {
        state_changes: vec![state_change(
            "unknown-pkg.service",
            UnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("unknown-pkg"),
        )],
        inventory: vec![],
        drop_ins: vec![],
        mount_findings: Vec::new(),
    });

    let plan = render(&snap);

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
    snap.units = Some(UnitSection {
        state_changes: vec![state_change(
            "advisory-timer.service",
            UnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("advisory-pkg"),
        )],
        inventory: vec![],
        drop_ins: vec![],
        mount_findings: Vec::new(),
    });
    snap.scheduled_tasks = Some(inspectah_core::types::scheduled::ScheduledTaskSection {
        systemd_timers: vec![inspectah_core::types::scheduled::SystemdTimer {
            name: "advisory-timer".into(),
            source: "local".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    let plan = render(&snap);

    // Advisory must survive even though the service is deferred
    assert_eq!(plan.advisories.len(), 1);
    assert_eq!(plan.advisories[0].unit.as_str(), "advisory-timer.service");
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
    snap.units = Some(UnitSection {
        state_changes: vec![state_change(
            "timer-pkg.service",
            UnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("timer-pkg"),
        )],
        inventory: vec![],
        drop_ins: vec![],
        mount_findings: Vec::new(),
    });
    snap.scheduled_tasks = Some(inspectah_core::types::scheduled::ScheduledTaskSection {
        systemd_timers: vec![inspectah_core::types::scheduled::SystemdTimer {
            name: "timer-pkg".into(),
            source: "local".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    let plan = render(&snap);

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
    snap.units = Some(UnitSection {
        state_changes: vec![state_change(
            "httpd.service",
            UnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("httpd"),
        )],
        inventory: vec![],
        drop_ins: vec![],
        mount_findings: Vec::new(),
    });

    let plan = render(&snap);

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
    snap.units = Some(UnitSection {
        state_changes: vec![state_change(
            "perl-related.service",
            UnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("perl-libs"),
        )],
        inventory: vec![],
        drop_ins: vec![],
        mount_findings: Vec::new(),
    });

    let plan = render(&snap);

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
        snap.units = Some(UnitSection {
            state_changes: vec![state_change(
                "vendor-agent.service",
                UnitState::Enabled,
                Some(PresetDefault::Disable),
                Some("vendor-agent"),
            )],
            inventory: vec![],
            drop_ins: vec![],
            mount_findings: Vec::new(),
        });
        snap
    };

    let plan = render(&snapshot_with(Vec::new()));
    assert!(plan.omissions.is_empty(), "the unit is still emitted");
    assert_eq!(plan.advisories.len(), 1, "{:?}", plan.advisories);
    assert_eq!(plan.advisories[0].unit.as_str(), "vendor-agent.service");
    assert_eq!(
        plan.advisories[0].reasons,
        vec![AdvisoryReason::PackageUnreachable],
        "a locally installed vendor RPM is not reachable by name"
    );

    let served = render(&snapshot_with(vec![
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
                OmissionReason::PackageExcluded {
                    package: "nginx".into(),
                },
                "# Omitted: unit.service (package 'nginx' was excluded)",
            ),
            pulled_in_by: None,
            in_aggregate: Expected::Omit(
                OmissionReason::PackageExcluded {
                    package: "nginx".into(),
                },
                "# Omitted: unit.service (package 'nginx' was excluded)",
            ),
        },
        Case {
            name: "tier 2: excluded package, baseline unavailable, still omits",
            baseline: None,
            added: vec![pkg("nginx", excluded(), PackageState::Added)],
            owner: Some("nginx"),
            expected: Expected::Omit(
                OmissionReason::PackageExcluded {
                    package: "nginx".into(),
                },
                "# Omitted: unit.service (package 'nginx' was excluded)",
            ),
            pulled_in_by: None,
            in_aggregate: Expected::Omit(
                OmissionReason::PackageExcluded {
                    package: "nginx".into(),
                },
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
                OmissionReason::NotInTargetImage {
                    package: "sssd".into(),
                },
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
                OmissionReason::PackageExcluded {
                    package: "cockpit-ws".into(),
                },
                "# Omitted: unit.service (package 'cockpit-ws' was excluded)",
            ),
            pulled_in_by: Some(false),
            in_aggregate: Expected::Omit(
                OmissionReason::PackageExcluded {
                    package: "cockpit-ws".into(),
                },
                "# Omitted: unit.service (package 'cockpit-ws' was excluded)",
            ),
        },
    ];

    for (action_state, preset) in [
        (UnitState::Enabled, PresetDefault::Disable),
        (UnitState::Disabled, PresetDefault::Enable),
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
            snap.units = Some(UnitSection {
                state_changes: vec![state_change(
                    "unit.service",
                    action_state,
                    Some(preset),
                    case.owner,
                )],
                inventory: vec![],
                drop_ins: vec![],
                mount_findings: Vec::new(),
            });

            let plan = render(&snap);
            let in_systemctl = plan
                .lines
                .iter()
                .any(|l| !l.starts_with('#') && l.contains("unit.service"));

            match expected {
                Expected::Omit(reason, comment) => {
                    assert_eq!(plan.omissions.len(), 1, "{}", label);
                    assert_eq!(&plan.omissions[0].reason, reason, "{}", label);
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
    assert_eq!(
        OmissionReason::NotCarried {
            package: "rhc".into()
        }
        .describe(),
        RHC_NOT_CARRIED
    );
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
    changes: Vec<UnitStateChange>,
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
    snap.units = Some(UnitSection {
        state_changes: changes,
        inventory: vec![],
        drop_ins: vec![],
        mount_findings: Vec::new(),
    });
    snap
}

/// Every `systemctl` instruction in `lines`: each non-comment instruction
/// joined with its `\` continuation lines, kept when it runs `systemctl`.
fn systemctl_instructions(lines: &[String]) -> Vec<String> {
    let mut instructions = Vec::new();
    let mut current = String::new();
    for line in lines {
        if current.is_empty() && line.trim_start().starts_with('#') {
            continue;
        }
        let line = line.trim_end();
        current.push_str(line.trim_end_matches('\\'));
        current.push(' ');
        if !line.ends_with('\\') {
            let instruction = std::mem::take(&mut current);
            if instruction.contains("systemctl") {
                instructions.push(instruction);
            }
        }
    }
    if current.contains("systemctl") {
        instructions.push(current);
    }
    instructions
}

/// Whether the unit is a quoted operand, after `--`, of a
/// `RUN systemctl <verb> --` instruction of the plan.
fn acted_on(lines: &[String], unit: &str) -> bool {
    let operand = format!("'{unit}'");
    systemctl_instructions(lines).iter().any(|i| {
        i.strip_prefix("RUN systemctl ")
            .and_then(|rest| rest.split_once(" -- "))
            .is_some_and(|(_, operands)| operands.split_whitespace().any(|w| w == operand))
    })
}

/// Whether the unit's name appears in no `systemctl` instruction of the
/// plan, in any form: quoted, bare or inside another word. The absence
/// half of [`acted_on`], so a regression that wrote a bare name fails it.
fn in_no_systemctl_instruction(lines: &[String], unit: &str) -> bool {
    systemctl_instructions(lines)
        .iter()
        .all(|i| !i.contains(unit))
}

/// Units owned by an auto package nothing included pulls in are omitted on
/// the scan path as not carried, for enable and disable. A mask is carried
/// whatever its package: § 10.2 rule 5, which holds the presence tiers,
/// applies to `enable`, `disable` and `set-default`, since `mask` reads no
/// `[Install]` and succeeds on an absent unit. The mask's control is the
/// same unit of the same package enabled, which is omitted.
#[test]
fn units_of_a_not_carried_package_are_omitted_as_not_carried_on_the_scan_path() {
    let cases = [
        (
            "rhcd.service",
            "rhc",
            UnitState::Enabled,
            Some(PresetDefault::Disable),
        ),
        (
            "firewalld.service",
            "firewalld",
            UnitState::Disabled,
            Some(PresetDefault::Enable),
        ),
        ("kdump.service", "kexec-tools", UnitState::Masked, None),
        (
            "kdump.service",
            "kexec-tools",
            UnitState::Enabled,
            Some(PresetDefault::Disable),
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

        let plan = render(&snap);
        if state == UnitState::Masked {
            assert!(acted_on(&plan.lines, unit), "{unit}: {:?}", plan.lines);
            assert!(plan.omissions.is_empty(), "{unit}: {:?}", plan.lines);
            continue;
        }
        assert!(
            in_no_systemctl_instruction(&plan.lines, unit),
            "{unit}: {:?}",
            plan.lines
        );
        assert_eq!(plan.omissions.len(), 1, "{unit}");
        assert_eq!(
            plan.omissions[0].reason,
            OmissionReason::NotCarried {
                package: owner.into()
            },
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

/// The mask exemption under `PackageExcluded`: with `kexec-tools`
/// excluded, `kdump.service` masked is carried, and the same unit of the
/// same package enabled is omitted for the exclusion (the control).
#[test]
fn a_mask_is_carried_when_its_package_is_excluded_and_an_enable_is_not() {
    for (state, preset) in [
        (UnitState::Masked, None),
        (UnitState::Enabled, Some(PresetDefault::Disable)),
    ] {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            baseline_package_names: Some(vec!["bash".into()]),
            packages_added: vec![added("kexec-tools", "x86_64", FindingKind::excluded())],
            ..Default::default()
        });
        snap.units = Some(UnitSection {
            state_changes: vec![state_change(
                "kdump.service",
                state,
                preset,
                Some("kexec-tools"),
            )],
            ..Default::default()
        });

        let plan = render(&snap);
        if state == UnitState::Masked {
            assert!(
                acted_on(&plan.lines, "kdump.service"),
                "{state:?}: {:?}",
                plan.lines
            );
            assert!(plan.omissions.is_empty(), "{state:?}: {:?}", plan.lines);
            continue;
        }
        assert!(
            in_no_systemctl_instruction(&plan.lines, "kdump.service"),
            "{state:?}: {:?}",
            plan.lines
        );
        assert_eq!(
            plan.lines,
            ["# Omitted: kdump.service (package 'kexec-tools' was excluded)"],
            "{state:?}"
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
            UnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("rhc"),
        )],
    );
    apply_scan_package_defaults(&mut snap);
    let plan = render(&snap);
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
            UnitState::Enabled,
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

    let plan = render(&snap);
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
                UnitState::Enabled,
                Some(PresetDefault::Disable),
                Some("cockpit-ws"),
            ),
            state_change(
                "cockpit-leaf.service",
                UnitState::Enabled,
                Some(PresetDefault::Disable),
                Some("cockpit"),
            ),
        ],
    );
    apply_scan_package_defaults(&mut snap);
    let plan = render(&snap);

    let reasons: Vec<(&str, OmissionReason)> = plan
        .omissions
        .iter()
        .map(|o| (o.unit.as_str(), o.reason.clone()))
        .collect();
    assert_eq!(
        reasons,
        vec![
            (
                "cockpit.socket",
                OmissionReason::NotCarried {
                    package: "cockpit-ws".into()
                }
            ),
            (
                "cockpit-leaf.service",
                OmissionReason::PackageExcluded {
                    package: "cockpit".into()
                }
            ),
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
            UnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("foo"),
        )],
    );
    apply_scan_package_defaults(&mut snap);
    let plan = render(&snap);
    assert_eq!(plan.omissions.len(), 1, "{:?}", plan.lines);
    assert_eq!(
        plan.omissions[0].reason,
        OmissionReason::PackageExcluded {
            package: "foo".into()
        }
    );
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
                UnitState::Enabled,
                Some(PresetDefault::Disable),
                Some("cockpit-ws"),
            )],
        );
        apply_scan_package_defaults(&mut snap);
        let plan = render(&snap);
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
            UnitState::Enabled,
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
    let plan = render(&snap);
    assert_eq!(plan.omissions.len(), 1, "{:?}", plan.lines);
    assert_eq!(
        plan.omissions[0].reason,
        OmissionReason::NotCarried {
            package: "dnf-plugins-core".into()
        }
    );
    assert!(in_no_systemctl_instruction(
        &plan.lines,
        "dnf-plugins-core.timer"
    ));
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
                UnitState::Enabled,
                Some(PresetDefault::Disable),
                Some(owner.as_str()),
            )
        };
        let services = || UnitSection {
            state_changes: vec![change()],
            inventory: vec![],
            drop_ins: vec![],
            mount_findings: Vec::new(),
        };

        let mut excluded = InspectionSnapshot::new();
        excluded.rpm = Some(RpmSection {
            baseline_package_names: Some(vec![]),
            packages_added: vec![added(&owner, "x86_64", FindingKind::excluded())],
            ..Default::default()
        });
        excluded.units = Some(services());

        let mut not_in_target = InspectionSnapshot::new();
        not_in_target.rpm = Some(RpmSection {
            baseline_package_names: Some(vec![]),
            ..Default::default()
        });
        not_in_target.units = Some(services());

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

        let package = owner.clone();
        for (reason, snap) in [
            (OmissionReason::PackageExcluded { package }, excluded),
            (
                OmissionReason::NotInTargetImage {
                    package: owner.clone(),
                },
                not_in_target,
            ),
            (
                OmissionReason::NotCarried {
                    package: owner.clone(),
                },
                not_carried,
            ),
        ] {
            let label = format!("{reason:?}, {sep:?}");
            let plan = render(&snap);
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

/// A carried unit from the vendor directory, owned as given.
fn test_carried_unit(
    name: inspectah_core::types::units::UnitName,
    owning_package: Option<String>,
) -> inspectah_core::types::units::CarriedUnit {
    use inspectah_core::types::units::{CarriedUnit, UnitFragment, UnitSubject};
    let fragment = UnitFragment::Vendor {
        file: name.clone(),
        owning_package,
    };
    CarriedUnit::try_from(UnitSubject::new(name, fragment).expect("file is the unit"))
        .expect("a test unit is carried")
}

// ---------------------------------------------------------------------------
// § 10.1 lines and § 10.2 rule 5's fragment half
// ---------------------------------------------------------------------------

/// A state change for `unit` with `fragment`, included.
fn change_with(unit: &str, fragment: UnitFragment, state: UnitState) -> UnitStateChange {
    let name: UnitName = unit.to_owned().try_into().expect("a unit name");
    let carried = CarriedUnit::try_from(UnitSubject::new(name, fragment).expect("a subject"))
        .expect("a carried unit");
    UnitStateChange::new(carried, state, None).expect("a state change")
}

fn local(unit: &str) -> UnitFragment {
    UnitFragment::Local {
        file: unit.to_owned().try_into().expect("a unit name"),
    }
}

fn vendor(unit: &str) -> UnitFragment {
    UnitFragment::Vendor {
        file: unit.to_owned().try_into().expect("a unit name"),
        owning_package: None,
    }
}

/// A snapshot with these state changes and config entries, and no package
/// section: the route § 10.2 says runs every rule.
fn units_snapshot(changes: Vec<UnitStateChange>, files: Vec<(&str, &str)>) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: files
            .into_iter()
            .map(|(path, content)| ConfigFileEntry {
                path: path.into(),
                category: ConfigCategory::Other,
                content: content.into(),
                disposition: FindingKind::included(),
                ..Default::default()
            })
            .collect(),
    });
    snap.units = Some(UnitSection {
        state_changes: changes,
        ..Default::default()
    });
    snap
}

/// § 10.1: one `RUN` per verb (enable, disable, mask), each unit
/// single-quoted on its own line after `--`, ordered by kind (service,
/// socket, path, timer, target, slice, then the storage kinds) and then by
/// name, whatever order the snapshot lists them in.
#[test]
fn one_run_per_verb_with_units_in_kind_then_name_order() {
    use UnitState::{Disabled, Enabled, Masked};
    let snap = units_snapshot(
        vec![
            change_with("report.timer", vendor("report.timer"), Enabled),
            change_with("myapp.path", vendor("myapp.path"), Enabled),
            change_with("cockpit.socket", vendor("cockpit.socket"), Enabled),
            change_with("zeta.service", vendor("zeta.service"), Enabled),
            change_with("alpha.service", vendor("alpha.service"), Enabled),
            change_with("kdump.service", vendor("kdump.service"), Disabled),
            change_with(
                "dev-hugepages.mount",
                UnitFragment::Vendor {
                    file: "dev-hugepages.mount".to_owned().try_into().unwrap(),
                    owning_package: Some("systemd".into()),
                },
                Masked,
            ),
            change_with("bluetooth.service", vendor("bluetooth.service"), Masked),
        ],
        vec![],
    );
    assert_eq!(
        render(&snap).lines,
        [
            "RUN systemctl enable -- \\",
            "    'alpha.service' \\",
            "    'zeta.service' \\",
            "    'cockpit.socket' \\",
            "    'myapp.path' \\",
            "    'report.timer'",
            "RUN systemctl disable -- \\",
            "    'kdump.service'",
            "RUN systemctl mask -- \\",
            "    'bluetooth.service' \\",
            "    'dev-hugepages.mount'",
        ]
    );
}

/// The administrator's unit file the conflict cases share, and a config
/// entry that needs it as a directory.
const WORKER_UNIT: &str = "/etc/systemd/system/worker.service";
const WORKER_CHILD: &str = "/etc/systemd/system/worker.service/extra.conf";

/// T15, conflict case: `worker.service/extra.conf` and `worker.service`,
/// in either order, are both withheld by the stager (the staging half is
/// `configtree_single_source_test`), so the local unit's enable is declined
/// with `UnitFileNotStaged` and named in an `# Omitted:` line. Without the
/// child, the enable is carried.
#[test]
fn t15_a_withheld_unit_file_declines_its_enable() {
    let worker = || {
        change_with(
            "worker.service",
            local("worker.service"),
            UnitState::Enabled,
        )
    };
    for (label, files) in [
        (
            "child first",
            vec![(WORKER_CHILD, "[Service]\n"), (WORKER_UNIT, "[Unit]\n")],
        ),
        (
            "unit first",
            vec![(WORKER_UNIT, "[Unit]\n"), (WORKER_CHILD, "[Service]\n")],
        ),
    ] {
        let snap = units_snapshot(vec![worker()], files);
        let plan = plan_units(&snap, &staged_writes(&snap));
        assert!(plan.planned.is_empty(), "{label}: {:?}", plan.planned);
        assert_eq!(plan.declined.len(), 1, "{label}");
        assert_eq!(plan.declined[0].0.as_str(), "worker.service", "{label}");
        assert_eq!(plan.declined[0].1, UnitAction::Enable, "{label}");
        assert_eq!(
            plan.declined[0].2,
            OmissionReason::UnitFileNotStaged,
            "{label}"
        );
        let lines = unit_state_lines(&plan);
        assert_eq!(
            lines,
            ["# Omitted: worker.service (its unit file is not in the export)"],
            "{label}"
        );
    }

    let snap = units_snapshot(vec![worker()], vec![(WORKER_UNIT, "[Unit]\n")]);
    let lines = render(&snap).lines;
    assert_eq!(
        lines,
        ["RUN systemctl enable -- \\", "    'worker.service'"],
        "control: the staged unit file carries its enable"
    );
}

/// Rule 5 asks the stager's question by `Destination`: a unit file offered
/// as `/etc/systemd/./system//<name>` is the file at the plain spelling. A
/// local unit's file at the odd spelling is staged, so its enable is
/// carried; a file at the odd spelling of a vendor unit's name is a unit
/// file staged at its name, so its enable is declined with the plain path.
/// Control: with nothing at the vendor unit's name, it is enabled.
#[test]
fn rule_5_reads_a_path_spelled_two_ways_as_one_file() {
    const ODD_LOCAL: &str = "/etc/systemd/./system//app.service";
    const ODD_VENDOR: &str = "/etc//systemd/system/./sshd.service";
    let snap = units_snapshot(
        vec![
            change_with("app.service", local("app.service"), UnitState::Enabled),
            change_with("sshd.service", vendor("sshd.service"), UnitState::Enabled),
        ],
        vec![(ODD_LOCAL, "[Unit]\n"), (ODD_VENDOR, "[Unit]\n")],
    );
    let plan = plan_units(&snap, &staged_writes(&snap));
    let planned: Vec<&str> = plan.planned.iter().map(|u| u.name().as_str()).collect();
    assert_eq!(planned, ["app.service"]);
    assert_eq!(
        plan.declined,
        [(
            UnitName::try_from("sshd.service".to_owned()).unwrap(),
            UnitAction::Enable,
            OmissionReason::InstallEvidenceChanged {
                path: "/etc/systemd/system/sshd.service".into()
            }
        )]
    );
    assert_eq!(
        unit_state_lines(&plan)[0],
        "# Omitted: sshd.service (the [Install] section in the image would not be the one \
         recorded on the source host (/etc/systemd/system/sshd.service))"
    );

    let control = units_snapshot(
        vec![change_with(
            "sshd.service",
            vendor("sshd.service"),
            UnitState::Enabled,
        )],
        vec![],
    );
    assert_eq!(
        render(&control).lines,
        ["RUN systemctl enable -- \\", "    'sshd.service'"]
    );
}

/// § 10.2 rule 5, the package half: the presence tiers are asked about the
/// owner only for a `Vendor` or package-owned storage unit. A `Shadowed`
/// unit whose shadow is staged keeps its command under each package reason
/// the tiers give (excluded, not carried, not in the target image), since
/// the image loads the staged copy; the `Vendor` twin of the same package
/// is declined with that reason (the control).
#[test]
fn a_staged_shadow_needs_no_package_under_any_presence_reason() {
    const OWNER: &str = "twinpkg";
    const SHADOW: &str = "/etc/systemd/system/shadow.service";
    let owned = |unit: &str, shadowed: bool| {
        let file: UnitName = unit.to_owned().try_into().expect("a unit name");
        let owning_package = Some(OWNER.to_owned());
        let fragment = if shadowed {
            UnitFragment::Shadowed {
                file,
                owning_package,
            }
        } else {
            UnitFragment::Vendor {
                file,
                owning_package,
            }
        };
        change_with(unit, fragment, UnitState::Enabled)
    };
    let changes = || vec![owned("shadow.service", true), owned("twin.service", false)];
    let with_packages = |packages: Vec<PackageEntry>| {
        let mut snap = units_snapshot(changes(), vec![(SHADOW, "[Unit]\n")]);
        snap.rpm = Some(RpmSection {
            baseline_package_names: Some(vec!["bash".into()]),
            packages_added: packages,
            ..Default::default()
        });
        snap
    };
    let mut not_carried = leaf_snapshot(
        vec![
            added("vim", "x86_64", FindingKind::included()),
            added(OWNER, "x86_64", FindingKind::included()),
        ],
        &["vim.x86_64"],
        &[&format!("{OWNER}.x86_64")],
        serde_json::json!({ "vim.x86_64": [] }),
        changes(),
    );
    not_carried.config = units_snapshot(vec![], vec![(SHADOW, "[Unit]\n")]).config;
    apply_scan_package_defaults(&mut not_carried);
    let package = OWNER.to_owned();
    let cases = [
        (
            "excluded",
            with_packages(vec![added(OWNER, "x86_64", FindingKind::excluded())]),
            OmissionReason::PackageExcluded {
                package: package.clone(),
            },
        ),
        (
            "not carried",
            not_carried,
            OmissionReason::NotCarried {
                package: package.clone(),
            },
        ),
        (
            "not in the target image",
            with_packages(vec![]),
            OmissionReason::NotInTargetImage { package },
        ),
    ];
    for (label, snap, reason) in cases {
        let plan = plan_units(&snap, &staged_writes(&snap));
        let planned: Vec<&str> = plan.planned.iter().map(|u| u.name().as_str()).collect();
        assert_eq!(planned, ["shadow.service"], "{label}");
        assert_eq!(
            plan.declined,
            [(
                UnitName::try_from("twin.service".to_owned()).unwrap(),
                UnitAction::Enable,
                reason
            )],
            "{label}"
        );
        assert!(plan.advisories.is_empty(), "{label}: {:?}", plan.advisories);
    }
}

/// T2's signature, checked in the source: `PlannedUnit` has no public
/// field and nothing outside `plan_units` builds one. The `compile_fail`
/// doctests on the type pass on any compile error and cannot see a
/// constructor added later; this fails if the struct gains a `pub` field,
/// its derives change (a `Default` or `Deserialize` derive is a
/// constructor), any `impl` block for it holds a function without a
/// `&self` receiver, a trait is implemented for it by hand (`From`,
/// `Default`), or a public function returns one.
#[test]
fn planned_unit_has_no_public_field_or_constructor() {
    const SOURCE: &str = include_str!("../src/render/unit_plan.rs");
    const STRUCT: &str = "pub struct PlannedUnit {";
    const DERIVES: &str = "#[derive(Debug, PartialEq, Eq)]";
    let lines: Vec<&str> = SOURCE.lines().collect();

    let at = lines
        .iter()
        .position(|l| *l == STRUCT)
        .expect("the struct is declared at the top level");
    assert_eq!(lines[at - 1], DERIVES, "the derives changed");
    let fields: Vec<&str> = lines[at + 1..]
        .iter()
        .take_while(|l| **l != "}")
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .collect();
    assert_eq!(fields, ["name: UnitName,", "action: UnitAction,"]);

    let impls: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with("impl") && l.contains("PlannedUnit"))
        .map(|(i, _)| i)
        .collect();
    assert!(!impls.is_empty(), "the accessors are in an impl block");
    for start in impls {
        assert_eq!(
            lines[start], "impl PlannedUnit {",
            "only an inherent impl block"
        );
        for line in lines[start + 1..].iter().take_while(|l| **l != "}") {
            if line.contains("fn ") {
                assert!(
                    line.contains("(&self"),
                    "a function with no receiver: {line}"
                );
            }
        }
    }

    // Every function signature, from `fn ` to its body's `{`, over as many
    // lines as it takes. `plan_units` returns a `UnitPlan`, the one route.
    for (start, _) in SOURCE.match_indices("fn ") {
        let signature = SOURCE[start..].split('{').next().unwrap_or_default();
        let returns = signature.split_once("->").map_or("", |(_, r)| r);
        assert!(
            !returns.contains("PlannedUnit"),
            "a function returns a PlannedUnit: {signature}"
        );
    }
}

// ---------------------------------------------------------------------------
// § 10.2 rules 2, 3 and 4, and § 10.3
// ---------------------------------------------------------------------------

/// A vendor unit owned by `owner`: a package-owned storage unit for a
/// storage kind, an ordinary vendor unit otherwise.
fn owned(unit: &str, owner: &str) -> UnitFragment {
    UnitFragment::Vendor {
        file: unit.to_owned().try_into().expect("a unit name"),
        owning_package: Some(owner.into()),
    }
}

/// The plan's declined entries as `(unit, action, reason)` strings and
/// reasons, for exact comparison.
fn declined_of(snap: &InspectionSnapshot) -> Vec<(String, UnitAction, OmissionReason)> {
    plan_units(snap, &staged_writes(snap))
        .declined
        .into_iter()
        .map(|(name, action, reason)| (name.as_str().to_owned(), action, reason))
        .collect()
}

/// T12: for each protected name, a `disable` and a `mask` give § 10.3's
/// FIXME and no command; an `enable` of the same unit is carried (the
/// control). The names cover each entry of the list: both globs and the
/// whole name.
#[test]
fn t12_a_protected_unit_is_never_disabled_or_masked() {
    for unit in [
        "bootc-fetch-apply-updates.timer",
        "bootc-fetch-apply-updates.service",
        "ostree-remount.service",
        "ostree-finalize-staged.service",
        "systemd-remount-fs.service",
    ] {
        for (state, verb, participle) in [
            (UnitState::Disabled, UnitAction::Disable, "disabled"),
            (UnitState::Masked, UnitAction::Mask, "masked"),
        ] {
            let snap = units_snapshot(vec![change_with(unit, vendor(unit), state)], vec![]);
            let lines = render(&snap).lines;
            assert_eq!(
                lines,
                [format!(
                    "# FIXME: {unit} is {participle} on the source host. Not carried: the \
                     image needs this unit."
                )],
                "{unit} {verb:?}"
            );
            assert!(in_no_systemctl_instruction(&lines, unit), "{unit}");
            assert_eq!(
                declined_of(&snap),
                [(unit.to_owned(), verb, OmissionReason::TargetNeedsUnit)],
                "{unit} {verb:?}"
            );
        }
        let snap = units_snapshot(
            vec![change_with(unit, vendor(unit), UnitState::Enabled)],
            vec![],
        );
        assert_eq!(
            render(&snap).lines,
            [
                "RUN systemctl enable -- \\".to_owned(),
                format!("    '{unit}'")
            ],
            "{unit}: the enable is carried"
        );
    }
}

/// The FIXME is the spec's text, character for character.
#[test]
fn t12_the_fixme_is_the_spec_text() {
    let snap = units_snapshot(
        vec![change_with(
            "bootc-fetch-apply-updates.timer",
            vendor("bootc-fetch-apply-updates.timer"),
            UnitState::Disabled,
        )],
        vec![],
    );
    assert_eq!(
        render(&snap).lines,
        [
            "# FIXME: bootc-fetch-apply-updates.timer is disabled on the source host. Not \
             carried: the image needs this unit."
        ]
    );
}

/// § 10.3: a masked `tmp.mount` gives its own FIXME and no command, while
/// its `enable` and `disable` are carried, and a mask of another
/// package-owned storage unit, `dev-hugepages.mount`, is carried.
#[test]
fn t12_a_masked_tmp_mount_is_refused_and_its_other_states_are_carried() {
    let tmp = |state| {
        units_snapshot(
            vec![change_with(
                "tmp.mount",
                owned("tmp.mount", "systemd"),
                state,
            )],
            vec![],
        )
    };
    let masked = tmp(UnitState::Masked);
    let lines = render(&masked).lines;
    assert_eq!(
        lines,
        [
            "# FIXME: tmp.mount is masked on the source host, where /tmp is then a directory on \
             the root filesystem. Not carried: the bootc base image mounts /tmp from this unit, \
             and without it /tmp is read-only."
        ]
    );
    assert!(in_no_systemctl_instruction(&lines, "tmp.mount"));
    assert_eq!(
        declined_of(&masked),
        [(
            "tmp.mount".to_owned(),
            UnitAction::Mask,
            OmissionReason::TargetNeedsUnit
        )]
    );
    for (state, verb) in [
        (UnitState::Enabled, "enable"),
        (UnitState::Disabled, "disable"),
    ] {
        assert_eq!(
            render(&tmp(state)).lines,
            [
                format!("RUN systemctl {verb} -- \\"),
                "    'tmp.mount'".to_owned()
            ],
            "{verb}"
        );
    }
    let hugepages = units_snapshot(
        vec![change_with(
            "dev-hugepages.mount",
            owned("dev-hugepages.mount", "systemd"),
            UnitState::Masked,
        )],
        vec![],
    );
    assert_eq!(
        render(&hugepages).lines,
        ["RUN systemctl mask -- \\", "    'dev-hugepages.mount'"]
    );
}

/// T8, render: a package-owned mount unit is acted on like a service:
/// enabled in `RUN systemctl enable`, disabled in `disable`, masked in
/// `mask`. With an administrator's unit file staged at its name, each
/// command is declined with `StorageFileStaged` and the file's path, since
/// the image would load that file (rule 2).
#[test]
fn t8_a_package_owned_mount_unit_acts_only_on_the_package_file() {
    const UNIT: &str = "var-lib-pkg.mount";
    const PATH: &str = "/etc/systemd/system/var-lib-pkg.mount";
    for (state, verb, action) in [
        (UnitState::Enabled, "enable", UnitAction::Enable),
        (UnitState::Disabled, "disable", UnitAction::Disable),
        (UnitState::Masked, "mask", UnitAction::Mask),
    ] {
        let change = || change_with(UNIT, owned(UNIT, "pkgmount"), state);
        let carried = units_snapshot(vec![change()], vec![]);
        assert_eq!(
            render(&carried).lines,
            [
                format!("RUN systemctl {verb} -- \\"),
                format!("    '{UNIT}'")
            ],
            "{verb}: the package's file"
        );

        let staged = units_snapshot(vec![change()], vec![(PATH, "[Mount]\nWhat=/dev/x\n")]);
        let lines = render(&staged).lines;
        assert_eq!(
            lines,
            [format!(
                "# Omitted: {UNIT} (the export carries an administrator's unit file for it \
                 ({PATH}))"
            )],
            "{verb}"
        );
        assert!(in_no_systemctl_instruction(&lines, UNIT), "{verb}");
        assert_eq!(
            declined_of(&staged),
            [(
                UNIT.to_owned(),
                action,
                OmissionReason::StorageFileStaged { path: PATH.into() }
            )],
            "{verb}"
        );
    }
}

/// T16, render: a masked unit and a staged unit file of the same name give
/// `MaskPathOccupied` and no mask, since `systemctl mask` exits 1 on a
/// regular file there (rule 4). The file is found by `Destination`, so an
/// odd spelling of its path counts. Control: without the file the mask is
/// carried.
#[test]
fn t16_a_mask_needs_its_path_free() {
    const PATH: &str = "/etc/systemd/system/kdump.service";
    for spelling in [PATH, "/etc/systemd//system/./kdump.service"] {
        let snap = units_snapshot(
            vec![change_with(
                "kdump.service",
                vendor("kdump.service"),
                UnitState::Masked,
            )],
            vec![(spelling, "[Service]\nExecStart=/bin/true\n")],
        );
        let lines = render(&snap).lines;
        assert_eq!(
            lines,
            [format!(
                "# Omitted: kdump.service (the export also carries a unit file at {PATH}; \
                 exclude one of the two)"
            )],
            "{spelling}"
        );
        assert!(in_no_systemctl_instruction(&lines, "kdump.service"));
        assert_eq!(
            declined_of(&snap),
            [(
                "kdump.service".to_owned(),
                UnitAction::Mask,
                OmissionReason::MaskPathOccupied { path: PATH.into() }
            )],
            "{spelling}"
        );
    }
    let control = units_snapshot(
        vec![change_with(
            "kdump.service",
            vendor("kdump.service"),
            UnitState::Masked,
        )],
        vec![],
    );
    assert_eq!(
        render(&control).lines,
        ["RUN systemctl mask -- \\", "    'kdump.service'"]
    );
}

/// The rules run in the spec's order and the first that fails decides: a
/// package-owned storage mask with a file at its name is rule 2's, not
/// rule 4's; a protected unit's mask with a file at its name is rule 3's,
/// not rule 4's; a protected unit's `disable` whose recorded file is not
/// staged is rule 3's, not rule 5's.
#[test]
fn the_first_failing_rule_decides() {
    let storage = units_snapshot(
        vec![change_with(
            "dev-hugepages.mount",
            owned("dev-hugepages.mount", "systemd"),
            UnitState::Masked,
        )],
        vec![("/etc/systemd/system/dev-hugepages.mount", "[Mount]\n")],
    );
    assert_eq!(
        declined_of(&storage),
        [(
            "dev-hugepages.mount".to_owned(),
            UnitAction::Mask,
            OmissionReason::StorageFileStaged {
                path: "/etc/systemd/system/dev-hugepages.mount".into()
            }
        )]
    );
    let protected_mask = units_snapshot(
        vec![change_with(
            "ostree-remount.service",
            vendor("ostree-remount.service"),
            UnitState::Masked,
        )],
        vec![("/etc/systemd/system/ostree-remount.service", "[Unit]\n")],
    );
    assert_eq!(
        declined_of(&protected_mask),
        [(
            "ostree-remount.service".to_owned(),
            UnitAction::Mask,
            OmissionReason::TargetNeedsUnit
        )]
    );
    let protected_local = units_snapshot(
        vec![change_with(
            "bootc-local.service",
            local("bootc-local.service"),
            UnitState::Disabled,
        )],
        vec![],
    );
    assert_eq!(
        declined_of(&protected_local),
        [(
            "bootc-local.service".to_owned(),
            UnitAction::Disable,
            OmissionReason::TargetNeedsUnit
        )]
    );
}
