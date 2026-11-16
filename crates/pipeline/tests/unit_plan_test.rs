use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::aggregate::AggregateSnapshotMeta;
use inspectah_core::types::config::{ConfigCategory, ConfigFileEntry, ConfigSection};
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::units::{
    AdvisoryUnit, AlsoMember, CarriedUnit, MountUnitFinding, PresetDefault, SystemdDropIn,
    UnitAction, UnitFragment, UnitName, UnitSection, UnitState, UnitStateChange, UnitSubject,
};
use inspectah_pipeline::render::comment::LINE_TERMINATORS;
use inspectah_pipeline::render::configtree::staged_writes;
use inspectah_pipeline::render::containerfile::render_containerfile;
use inspectah_pipeline::render::not_carried::apply_scan_package_defaults;
use inspectah_pipeline::render::not_carried::not_carried_packages;
use inspectah_pipeline::render::unit_plan::{
    AdvisoryReason, AlsoWhy, DeclinedMarker, OmissionReason, ServiceAdvisory, UnitPlan,
    declined_line, effective_target_packages, is_package_installable, mount_advisory_lines,
    plan_units, pulled_in_by_included, unit_state_lines,
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
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
    });

    let plan = render(&snap);

    // Omission comment IS in lines: user sees what was excluded
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
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
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
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
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
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
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
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
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

/// Duplicate same-name packages: one excluded, one included+installable.
/// The included entry should win: package is present, no advisory.
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
                disposition: FindingKind::included(), // included entry: should win
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
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
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
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
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
            manual_links: Vec::new(),
            undetermined_installs: Vec::new(),
            default_target: None,
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
                manual_links: Vec::new(),
                undetermined_installs: Vec::new(),
                default_target: None,
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

/// One function gives a declined command's marker and full text, and the
/// unit state section renders exactly that: a FIXME for a refusal the
/// target needs (with `tmp.mount`'s own sentence for its mask), an
/// `# Omitted:` line naming the unit for every other reason.
#[test]
fn a_declined_command_renders_from_one_line_function() {
    let name = |unit: &str| -> UnitName { unit.to_owned().try_into().expect("a unit name") };
    let cases = [
        (
            name("tmp.mount"),
            UnitAction::Mask,
            OmissionReason::TargetNeedsUnit,
            DeclinedMarker::Fixme,
            "tmp.mount is masked on the source host, where /tmp is then a directory on the root \
             filesystem. Not carried: the bootc base image mounts /tmp from this unit, and \
             without it /tmp is read-only."
                .to_owned(),
        ),
        (
            name("ostree-remount.service"),
            UnitAction::Disable,
            OmissionReason::TargetNeedsUnit,
            DeclinedMarker::Fixme,
            "ostree-remount.service is disabled on the source host. Not carried: the image \
             needs this unit."
                .to_owned(),
        ),
        (
            name("rhcd.service"),
            UnitAction::Enable,
            OmissionReason::NotCarried {
                package: "rhc".into(),
            },
            DeclinedMarker::Omitted,
            format!("rhcd.service ({RHC_NOT_CARRIED})"),
        ),
        (
            name("bundle.service"),
            UnitAction::Enable,
            OmissionReason::AlsoMember {
                member: name("helper.service"),
                why: AlsoWhy::RowNotCarried,
                verb: UnitAction::Enable,
                named: vec![name("helper.service")],
                further: None,
                add: Vec::new(),
            },
            DeclinedMarker::Advisory,
            "bundle.service is enabled on the source host. Not carried: systemctl enable on it \
             also enables helper.service (named by Also=), whose own row is not carried with \
             this command."
                .to_owned(),
        ),
    ];
    for (unit, action, reason, marker, text) in cases {
        let line = declined_line(&unit, action, &reason);
        assert_eq!(line.marker, marker, "{unit:?}");
        assert_eq!(line.text, text, "{unit:?}");
        let prefix = match marker {
            DeclinedMarker::Fixme => "# FIXME: ",
            DeclinedMarker::Omitted => "# Omitted: ",
            DeclinedMarker::Advisory => "# Advisory: ",
        };
        assert_eq!(line.comment(), format!("{prefix}{text}"), "{unit:?}");
        let plan = UnitPlan {
            declined: vec![(unit.clone(), action, reason)],
            ..Default::default()
        };
        assert_eq!(unit_state_lines(&plan), [line.comment()], "{unit:?}");
    }
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
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
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
/// the scan path as not carried, for enable, disable and mask: § 10.2 rule
/// 5's package half applies to `mask` through the same presence tiers
/// ("A mask follows the package check"). The carried side of a mask is
/// T34's included-package and in-baseline controls.
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

/// § 10.2 rule 5 under `PackageExcluded`: with `kexec-tools` excluded and
/// not in the baseline, `kdump.service` is omitted for the exclusion
/// whether it is masked or enabled. A mask follows the package check as an
/// `enable` does; the carried side is T34's controls.
#[test]
fn a_mask_and_an_enable_are_omitted_when_their_package_is_excluded() {
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
            manual_links: Vec::new(),
            undetermined_installs: Vec::new(),
            default_target: None,
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
    snap.scheduled_tasks = Some(timer_descriptions(&changes));
    snap.units = Some(UnitSection {
        state_changes: changes,
        ..Default::default()
    });
    snap
}

/// The scheduled section a scan records beside `changes`: a description of
/// each timer, with its default trigger, the same-named service, so the
/// trigger check of rule 5 reads what the scheduled inspector would have
/// recorded for a timer that sets no `Unit=`.
fn timer_descriptions(
    changes: &[UnitStateChange],
) -> inspectah_core::types::scheduled::ScheduledTaskSection {
    use inspectah_core::types::scheduled::{ScheduledTaskSection, SystemdTimer, TimerTrigger};
    ScheduledTaskSection {
        systemd_timers: changes
            .iter()
            .filter_map(|sc| sc.unit().name().as_str().strip_suffix(".timer"))
            .map(|stem| SystemdTimer {
                name: stem.to_owned(),
                source: "local".into(),
                trigger: TimerTrigger::Unit(
                    format!("{stem}.service").try_into().expect("a unit name"),
                ),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
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
/// staged is rule 3's, not rule 5's; a masked package-owned `tmp.mount`
/// with a file at its name is rule 2's, not rule 3's.
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
    let storage_mask_with_file = units_snapshot(
        vec![change_with(
            "tmp.mount",
            owned("tmp.mount", "systemd"),
            UnitState::Masked,
        )],
        vec![("/etc/systemd/system/tmp.mount", "[Mount]\nWhere=/tmp\n")],
    );
    assert_eq!(
        declined_of(&storage_mask_with_file),
        [(
            "tmp.mount".to_owned(),
            UnitAction::Mask,
            OmissionReason::StorageFileStaged {
                path: "/etc/systemd/system/tmp.mount".into()
            }
        )],
        "rule 2 decides before rule 3"
    );
}

/// The generated `cron-backup` pair of one cron entry.
fn with_generated_cron_backup(mut snap: InspectionSnapshot) -> InspectionSnapshot {
    use inspectah_core::types::scheduled::GeneratedTimerUnit;
    snap.scheduled_tasks
        .get_or_insert_default()
        .generated_timer_units
        .push(GeneratedTimerUnit {
            name: "cron-backup".into(),
            cron_expr: "0 2 * * *".into(),
            timer_content: "[Timer]\nOnCalendar=daily\n".into(),
            service_content: "[Service]\nExecStart=/usr/bin/backup\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        });
    snap
}

/// What lands at each path of the generated `cron-backup` pair, and why
/// each file there is withheld.
fn cron_backup_paths(
    snap: &InspectionSnapshot,
) -> Vec<(
    Option<inspectah_pipeline::render::configtree::StagedFileKind>,
    Vec<String>,
)> {
    use inspectah_pipeline::render::configtree::Destination;
    let staged = staged_writes(snap);
    [
        "/etc/systemd/system/cron-backup.timer",
        "/etc/systemd/system/cron-backup.service",
    ]
    .into_iter()
    .map(|path| {
        let dest = Destination::new(path).expect("a destination");
        let why = staged
            .withheld
            .iter()
            .filter(|(at, _, _)| *at == dest)
            .map(|(_, _, why)| why.name().to_owned())
            .collect();
        (staged.writes.get(&dest).map(|w| w.source), why)
    })
    .collect()
}

/// The reason a generated request is declined for when a collected unit
/// with a row, `name`, has one of its files' names.
fn row_collision(name: &str) -> OmissionReason {
    OmissionReason::GeneratedNameCollision {
        name: name.to_owned().try_into().expect("a unit name"),
        reserved_by: inspectah_pipeline::render::unit_plan::Reservation::Row,
    }
}

/// The generated requests the plan declined, by unit and reason.
fn generated_declined_of(snap: &InspectionSnapshot) -> Vec<(String, OmissionReason)> {
    plan_units(snap, &staged_writes(snap))
        .generated_declined
        .into_iter()
        .map(|d| (d.unit.as_str().to_owned(), d.reason))
        .collect()
}

/// § 10.2 rules 1 and 4 with a generated unit of the masked unit's name: a
/// zero-length masked `cron-backup.timer` is withheld (rule 1), and the
/// generated pair of that name is withheld for the collision, so nothing
/// lands at the path, the mask is planned, and the generated request is
/// declined with `GeneratedNameCollision`. Control: the same mask with no
/// generated timer is planned the same way and nothing is declined.
#[test]
fn a_masked_unit_withholds_a_generated_unit_of_its_name() {
    use inspectah_pipeline::render::configtree::StagedFileKind;
    const UNIT: &str = "cron-backup.timer";
    const PATH: &str = "/etc/systemd/system/cron-backup.timer";
    let masked = || {
        units_snapshot(
            vec![change_with(UNIT, UnitFragment::Absent, UnitState::Masked)],
            vec![(PATH, "")],
        )
    };
    let mask_lines = [
        "RUN systemctl mask -- \\".to_owned(),
        format!("    '{UNIT}'"),
    ];

    let with_generated = with_generated_cron_backup(masked());
    assert_eq!(
        cron_backup_paths(&with_generated),
        [
            (
                None,
                vec![
                    "GeneratedNameCollision".to_owned(),
                    "ZeroLengthUnitFile".to_owned()
                ]
            ),
            (None, vec!["GeneratedNameCollision".to_owned()]),
        ]
    );
    assert_eq!(render(&with_generated).lines, mask_lines);
    assert_eq!(
        generated_declined_of(&with_generated),
        [(UNIT.to_owned(), row_collision(UNIT))]
    );

    let control = masked();
    assert_eq!(
        cron_backup_paths(&control),
        [
            (None, vec!["ZeroLengthUnitFile".to_owned()]),
            (None, vec![])
        ]
    );
    assert_eq!(render(&control).lines, mask_lines);
    assert!(generated_declined_of(&control).is_empty());

    // The same generated timer with the collision removed, the mask row and
    // its file excluded: the generated pair is staged and its request
    // planned, so the collision, not the request, made the difference.
    let mut released = with_generated_cron_backup(masked());
    for sc in &mut released.units.as_mut().expect("units").state_changes {
        sc.disposition = FindingKind::excluded();
    }
    for f in &mut released.config.as_mut().expect("config").files {
        f.disposition = FindingKind::excluded();
    }
    assert_eq!(
        cron_backup_paths(&released),
        [
            (Some(StagedFileKind::GeneratedUnit), vec![]),
            (Some(StagedFileKind::GeneratedUnit), vec![])
        ]
    );
    assert!(generated_declined_of(&released).is_empty());
    assert!(
        plan_units(&released, &staged_writes(&released))
            .scheduled
            .iter()
            .any(|p| p.name().as_str() == UNIT),
        "the generated request is planned"
    );
}

/// An imported `Local` row, a zero-length config entry at its path, and a
/// generated unit of its name: rule 1 withholds the zero-length file, the
/// generated pair is withheld for the collision and its request declined,
/// and the row's enable is omitted as `UnitFileNotStaged`, so no enable
/// acts on a file nobody recorded. Control: the row's file with content is
/// staged and the enable is planned, with the generated request declined
/// all the same.
#[test]
fn an_imported_local_row_over_a_mask_file_is_not_enabled_against_a_generated_file() {
    const UNIT: &str = "cron-backup.timer";
    const PATH: &str = "/etc/systemd/system/cron-backup.timer";
    let snap = |content: &str| {
        with_generated_cron_backup(units_snapshot(
            vec![change_with(UNIT, local(UNIT), UnitState::Enabled)],
            vec![(PATH, content)],
        ))
    };

    let empty = snap("");
    assert_eq!(cron_backup_paths(&empty)[0].0, None, "nothing lands");
    assert_eq!(
        declined_of(&empty),
        [(
            UNIT.to_owned(),
            UnitAction::Enable,
            OmissionReason::UnitFileNotStaged
        )]
    );
    assert!(
        in_no_systemctl_instruction(&render(&empty).lines, UNIT),
        "{:?}",
        render(&empty).lines
    );
    assert_eq!(
        generated_declined_of(&empty),
        [(UNIT.to_owned(), row_collision(UNIT))]
    );

    let control = snap("[Timer]\nOnCalendar=weekly\n");
    assert_eq!(
        cron_backup_paths(&control)[0].0,
        Some(inspectah_pipeline::render::configtree::StagedFileKind::ConfigFile)
    );
    assert!(declined_of(&control).is_empty());
    assert_eq!(
        render(&control).lines,
        [
            "RUN systemctl enable -- \\".to_owned(),
            format!("    '{UNIT}'")
        ]
    );
    assert_eq!(
        generated_declined_of(&control),
        [(UNIT.to_owned(), row_collision(UNIT))]
    );
}

// ---------------------------------------------------------------------------
// § 10.6 mount, automount and swap advisories
// ---------------------------------------------------------------------------

/// § 10.6's lines, verbatim. The first two take the unit, its state and
/// (for the carry line) the verb; the layout lines take a path.
fn advisory_fact(unit: &str, state: &str) -> String {
    format!(
        "# Advisory: {unit} is {state} on the source host. inspectah does not enable, disable \
         or mask a mount, automount or swap unit the administrator wrote."
    )
}
const FILE_STAGED_LINE: &str = "#   Its unit file is copied into the image. No enable line is \
     written, but the unit still starts if another unit requires it or names its mount point \
     in RequiresMountsFor=, and it replaces a unit of the same name that the image ships.";
fn usr_line(path: &str) -> String {
    format!(
        "#   /usr is part of the read-only image on the target. The mount point must already \
         exist in the image, and a mount there hides the image's own content under {path}."
    )
}
const BOOTC_OWNED_LINE: &str = "#   bootc and ostree own this mount point on the target.";
fn var_symlink_line(top: &str) -> String {
    format!(
        "#   {top} is a symlink into /var in the RHEL bootc base image. The unit must be renamed \
         for the resolved path (for /srv: var-srv.mount with Where=/var/srv)."
    )
}
/// The rename line for an automount, which keeps its suffix and has a
/// matching mount unit.
fn automount_var_symlink_line(top: &str) -> String {
    format!(
        "#   {top} is a symlink into /var in the RHEL bootc base image. The unit must be renamed \
         for the resolved path (for /srv: var-srv.automount with Where=/var/srv), and so must \
         its matching mount unit (var-srv.mount)."
    )
}
/// The line for a finding merged from more than one host.
const MERGED_LINE: &str =
    "#   This describes one host's copy of the unit; the copy staged may differ.";
fn opt_line(path: &str) -> String {
    format!(
        "#   /opt is a directory of the read-only image in the RHEL bootc base image, not a link \
         into /var. The unit keeps its name. The mount point must already exist in the image, \
         and a mount there hides the image's own content under {path}."
    )
}
const TMP_MOUNT_LINE: &str = "#   The bootc base image starts tmp.mount from a link it ships, \
     so this file replaces the unit that mounts /tmp on the target.";
const NOFAIL_LINE: &str = "#   No nofail option in the unit file. If the unit is started at \
     boot and its device is missing, units that require it fail, and boot can stop in emergency \
     mode.";
fn carry_line(verb: &str, unit: &str) -> String {
    format!("#   To carry it, add after the configuration COPY: RUN systemctl {verb} -- '{unit}'")
}

/// A finding for `unit` as the collector records one whose unit file it
/// read, so `nofail` is known.
fn mount_finding(
    unit: &str,
    fragment: UnitFragment,
    state: Option<UnitState>,
    mount_point: Option<&str>,
    nofail: bool,
) -> MountUnitFinding {
    let name: UnitName = unit.to_owned().try_into().expect("a unit name");
    let subject = UnitSubject::new(name, fragment).expect("a subject");
    MountUnitFinding {
        unit: AdvisoryUnit::try_from(subject).expect("an advisory unit"),
        state_change: state.map(|s| (s, None)),
        mount_point: mount_point.map(str::to_owned),
        nofail: Some(nofail),
        aggregate: None,
    }
}

fn shadowed(unit: &str) -> UnitFragment {
    UnitFragment::Shadowed {
        file: unit.to_owned().try_into().expect("a unit name"),
        owning_package: Some("systemd".into()),
    }
}

/// § 10.6's table, row by row, over findings as the collector records
/// them. The file line and the `tmp.mount` line follow `staged_writes`:
/// each case is checked with the unit file staged by a config entry and
/// without it. A masked finding gets no layout line and no `nofail` line, since a
/// masked unit does not mount; a mount unit with no mount point (its file
/// was not read) gets no `nofail` line. Layout lines match the mount point
/// in its normal form. A finding told to rename its unit for `/var` gets
/// no recipe under its old name, while one whose mount point needs no
/// rename keeps it.
#[test]
fn mount_advisory_lines_follow_the_table() {
    use UnitState::{Disabled, Enabled, Masked};
    let unowned = |unit: &str| UnitFragment::Vendor {
        file: unit.to_owned().try_into().expect("a unit name"),
        owning_package: None,
    };
    let cases: Vec<(MountUnitFinding, Vec<String>)> = vec![
        (
            mount_finding(
                "srv.mount",
                local("srv.mount"),
                Some(Enabled),
                Some("/srv"),
                false,
            ),
            vec![
                advisory_fact("srv.mount", "enabled"),
                var_symlink_line("/srv"),
                NOFAIL_LINE.into(),
            ],
        ),
        (
            mount_finding(
                "home-alice.mount",
                local("home-alice.mount"),
                Some(Enabled),
                Some("/home/alice"),
                true,
            ),
            vec![
                advisory_fact("home-alice.mount", "enabled"),
                var_symlink_line("/home"),
            ],
        ),
        (
            mount_finding(
                "opt-vendor.mount",
                local("opt-vendor.mount"),
                Some(Enabled),
                Some("/opt/vendor"),
                true,
            ),
            vec![
                advisory_fact("opt-vendor.mount", "enabled"),
                opt_line("/opt/vendor"),
                carry_line("enable", "opt-vendor.mount"),
            ],
        ),
        (
            mount_finding(
                "usr-share-data.mount",
                local("usr-share-data.mount"),
                Some(Enabled),
                Some("/usr/share/data"),
                true,
            ),
            vec![
                advisory_fact("usr-share-data.mount", "enabled"),
                usr_line("/usr/share/data"),
                carry_line("enable", "usr-share-data.mount"),
            ],
        ),
        (
            mount_finding(
                "usr-local.mount",
                local("usr-local.mount"),
                Some(Enabled),
                Some("/usr/local"),
                true,
            ),
            vec![
                advisory_fact("usr-local.mount", "enabled"),
                usr_line("/usr/local"),
                carry_line("enable", "usr-local.mount"),
            ],
        ),
        (
            mount_finding(
                "var.mount",
                local("var.mount"),
                Some(Enabled),
                Some("/var"),
                true,
            ),
            vec![
                advisory_fact("var.mount", "enabled"),
                BOOTC_OWNED_LINE.into(),
                carry_line("enable", "var.mount"),
            ],
        ),
        (
            mount_finding(
                "boot-efi.mount",
                local("boot-efi.mount"),
                None,
                Some("/boot/efi"),
                true,
            ),
            vec![
                advisory_fact("boot-efi.mount", "not enabled"),
                BOOTC_OWNED_LINE.into(),
            ],
        ),
        (
            // A path under a bootc-owned mount point is not the mount
            // point, and `/srvdata` is not under `/srv`.
            mount_finding(
                "var-lib-data.mount",
                local("var-lib-data.mount"),
                Some(Enabled),
                Some("/var/lib/data"),
                true,
            ),
            vec![
                advisory_fact("var-lib-data.mount", "enabled"),
                carry_line("enable", "var-lib-data.mount"),
            ],
        ),
        (
            mount_finding(
                "srvdata.mount",
                local("srvdata.mount"),
                Some(Enabled),
                Some("/srvdata"),
                true,
            ),
            vec![
                advisory_fact("srvdata.mount", "enabled"),
                carry_line("enable", "srvdata.mount"),
            ],
        ),
        (
            // An automount has no `Options=`: no `nofail` line. Its rename
            // line keeps the `.automount` suffix and names its matching
            // mount unit, not the mount unit's line.
            mount_finding(
                "srv-data.automount",
                local("srv-data.automount"),
                Some(Enabled),
                Some("/srv/data"),
                false,
            ),
            vec![
                advisory_fact("srv-data.automount", "enabled"),
                automount_var_symlink_line("/srv"),
            ],
        ),
        (
            // The control: a mount unit at the same mount point gets the
            // mount unit's rename line.
            mount_finding(
                "srv-data.mount",
                local("srv-data.mount"),
                Some(Enabled),
                Some("/srv/data"),
                true,
            ),
            vec![
                advisory_fact("srv-data.mount", "enabled"),
                var_symlink_line("/srv"),
            ],
        ),
        (
            mount_finding(
                "dev-vdb1.swap",
                local("dev-vdb1.swap"),
                Some(Enabled),
                None,
                false,
            ),
            vec![
                advisory_fact("dev-vdb1.swap", "enabled"),
                NOFAIL_LINE.into(),
                carry_line("enable", "dev-vdb1.swap"),
            ],
        ),
        (
            // A swap whose unit file was not read (not text, over the size
            // cap, or a read error): nothing is known of its options, so no
            // `nofail` line. The controls are `dev-vdb1.swap` above, read
            // without `nofail`, and `dev-vdb2.swap` below, read with it.
            MountUnitFinding {
                nofail: None,
                ..mount_finding(
                    "dev-vdb4.swap",
                    local("dev-vdb4.swap"),
                    Some(Enabled),
                    None,
                    false,
                )
            },
            vec![
                advisory_fact("dev-vdb4.swap", "enabled"),
                carry_line("enable", "dev-vdb4.swap"),
            ],
        ),
        (
            mount_finding(
                "dev-vdb2.swap",
                local("dev-vdb2.swap"),
                Some(Enabled),
                None,
                true,
            ),
            vec![
                advisory_fact("dev-vdb2.swap", "enabled"),
                carry_line("enable", "dev-vdb2.swap"),
            ],
        ),
        (
            // A masked unit does not mount: no layout line and no `nofail`
            // line, whatever its mount point.
            mount_finding(
                "data.mount",
                unowned("data.mount"),
                Some(Masked),
                Some("/srv/data"),
                false,
            ),
            vec![
                advisory_fact("data.mount", "masked"),
                carry_line("mask", "data.mount"),
            ],
        ),
        (
            // The control: the same unit enabled.
            mount_finding(
                "data.mount",
                unowned("data.mount"),
                Some(Enabled),
                Some("/srv/data"),
                false,
            ),
            vec![
                advisory_fact("data.mount", "enabled"),
                var_symlink_line("/srv"),
                NOFAIL_LINE.into(),
            ],
        ),
        (
            mount_finding(
                "usr-share-data.mount",
                unowned("usr-share-data.mount"),
                Some(Masked),
                Some("/usr/share/data"),
                false,
            ),
            vec![
                advisory_fact("usr-share-data.mount", "masked"),
                carry_line("mask", "usr-share-data.mount"),
            ],
        ),
        (
            mount_finding(
                "opt-vendor.mount",
                unowned("opt-vendor.mount"),
                Some(Masked),
                Some("/opt/vendor"),
                false,
            ),
            vec![
                advisory_fact("opt-vendor.mount", "masked"),
                carry_line("mask", "opt-vendor.mount"),
            ],
        ),
        (
            mount_finding(
                "var.mount",
                unowned("var.mount"),
                Some(Masked),
                Some("/var"),
                false,
            ),
            vec![
                advisory_fact("var.mount", "masked"),
                carry_line("mask", "var.mount"),
            ],
        ),
        (
            mount_finding(
                "dev-vdb3.swap",
                unowned("dev-vdb3.swap"),
                Some(Masked),
                None,
                false,
            ),
            vec![
                advisory_fact("dev-vdb3.swap", "masked"),
                carry_line("mask", "dev-vdb3.swap"),
            ],
        ),
        (
            // A mount point as written is matched in its normal form: empty
            // and `.` components dropped. The line shows the normal form.
            mount_finding(
                "var.mount",
                local("var.mount"),
                Some(Enabled),
                Some("/var/"),
                true,
            ),
            vec![
                advisory_fact("var.mount", "enabled"),
                BOOTC_OWNED_LINE.into(),
                carry_line("enable", "var.mount"),
            ],
        ),
        (
            mount_finding(
                "boot-efi.mount",
                local("boot-efi.mount"),
                Some(Enabled),
                Some("/boot//efi"),
                true,
            ),
            vec![
                advisory_fact("boot-efi.mount", "enabled"),
                BOOTC_OWNED_LINE.into(),
                carry_line("enable", "boot-efi.mount"),
            ],
        ),
        (
            mount_finding("-.mount", local("-.mount"), Some(Enabled), Some("/"), true),
            vec![
                advisory_fact("-.mount", "enabled"),
                BOOTC_OWNED_LINE.into(),
                carry_line("enable", "-.mount"),
            ],
        ),
        (
            mount_finding(
                "srv-data.mount",
                local("srv-data.mount"),
                Some(Enabled),
                Some("/srv//data"),
                true,
            ),
            vec![
                advisory_fact("srv-data.mount", "enabled"),
                var_symlink_line("/srv"),
            ],
        ),
        (
            mount_finding(
                "usr-share-data.mount",
                local("usr-share-data.mount"),
                Some(Enabled),
                Some("/usr/./share//data/"),
                true,
            ),
            vec![
                advisory_fact("usr-share-data.mount", "enabled"),
                usr_line("/usr/share/data"),
                carry_line("enable", "usr-share-data.mount"),
            ],
        ),
        (
            mount_finding(
                "opt-vendor.mount",
                local("opt-vendor.mount"),
                Some(Enabled),
                Some("/opt//vendor/"),
                true,
            ),
            vec![
                advisory_fact("opt-vendor.mount", "enabled"),
                opt_line("/opt/vendor"),
                carry_line("enable", "opt-vendor.mount"),
            ],
        ),
        (
            // A mount unit with no mount point: its file was not read, so
            // nothing is known of its options and the `nofail` line is not
            // given. The control is the next case.
            mount_finding(
                "unread.mount",
                local("unread.mount"),
                Some(Enabled),
                None,
                false,
            ),
            vec![
                advisory_fact("unread.mount", "enabled"),
                carry_line("enable", "unread.mount"),
            ],
        ),
        (
            mount_finding(
                "data.mount",
                local("data.mount"),
                Some(Enabled),
                Some("/data"),
                false,
            ),
            vec![
                advisory_fact("data.mount", "enabled"),
                NOFAIL_LINE.into(),
                carry_line("enable", "data.mount"),
            ],
        ),
        (
            mount_finding(
                "ghost.mount",
                UnitFragment::Absent,
                Some(Masked),
                None,
                false,
            ),
            vec![
                advisory_fact("ghost.mount", "masked"),
                carry_line("mask", "ghost.mount"),
            ],
        ),
        (
            mount_finding("tmp.mount", UnitFragment::Absent, Some(Masked), None, false),
            vec![advisory_fact("tmp.mount", "masked")],
        ),
        (
            mount_finding(
                "tmp.mount",
                unowned("tmp.mount"),
                Some(Enabled),
                Some("/tmp"),
                true,
            ),
            vec![
                advisory_fact("tmp.mount", "enabled"),
                carry_line("enable", "tmp.mount"),
            ],
        ),
        (
            mount_finding("old.mount", local("old.mount"), Some(Disabled), None, true),
            vec![advisory_fact("old.mount", "disabled")],
        ),
    ];
    let empty = InspectionSnapshot::new();
    let nothing_staged = staged_writes(&empty);
    for (finding, expected) in &cases {
        let unit = finding.unit.name().as_unit_name().as_str();
        assert_eq!(
            &mount_advisory_lines(finding, &nothing_staged, &[]),
            expected,
            "{unit}, file not staged"
        );
    }

    // With the unit file staged, the file line follows the first line,
    // and for `tmp.mount` the `tmp.mount` line follows the layout lines.
    let tmp = mount_finding(
        "tmp.mount",
        shadowed("tmp.mount"),
        None,
        Some("/tmp"),
        false,
    );
    let srv = &cases[0].0;
    let staged_snap = units_snapshot(
        vec![],
        vec![
            ("/etc/systemd/system/srv.mount", "[Mount]\nWhere=/srv\n"),
            ("/etc/systemd/system/tmp.mount", "[Mount]\nWhere=/tmp\n"),
        ],
    );
    let staged = staged_writes(&staged_snap);
    assert_eq!(
        mount_advisory_lines(srv, &staged, &[]),
        [
            advisory_fact("srv.mount", "enabled"),
            FILE_STAGED_LINE.into(),
            var_symlink_line("/srv"),
            NOFAIL_LINE.into(),
        ]
    );
    assert_eq!(
        mount_advisory_lines(&tmp, &staged, &[]),
        [
            advisory_fact("tmp.mount", "not enabled"),
            FILE_STAGED_LINE.into(),
            TMP_MOUNT_LINE.into(),
            NOFAIL_LINE.into(),
        ]
    );
    assert_eq!(
        mount_advisory_lines(&tmp, &nothing_staged, &[]),
        [
            advisory_fact("tmp.mount", "not enabled"),
            NOFAIL_LINE.into(),
        ],
        "tmp.mount, file not staged"
    );

    // A masked finding beside a staged unit file of its name (a fleet
    // aggregate: one host masked it, another wrote it): `systemctl mask`
    // fails on the occupied path, so no mask recipe. The reversed control
    // is the masked `data.mount` row above, with no file staged, which
    // keeps its recipe.
    let masked = mount_finding(
        "data.mount",
        unowned("data.mount"),
        Some(Masked),
        Some("/srv/data"),
        false,
    );
    let data_staged = units_snapshot(
        vec![],
        vec![(
            "/etc/systemd/system/data.mount",
            "[Mount]\nWhere=/srv/data\n",
        )],
    );
    assert_eq!(
        mount_advisory_lines(&masked, &staged_writes(&data_staged), &[]),
        [
            advisory_fact("data.mount", "masked"),
            FILE_STAGED_LINE.into(),
        ]
    );
    // A mount unit with drop-ins: the collector reads `Options=` from the
    // unit file alone, and a drop-in may set `nofail`, so no `nofail` line
    // is given. The reversed control is the same finding with no drop-in
    // row, the `data.mount` row of the table, which has the line.
    let data = mount_finding(
        "data.mount",
        local("data.mount"),
        Some(Enabled),
        Some("/data"),
        false,
    );
    let drop_in = SystemdDropIn {
        unit: "data.mount".into(),
        path: "/etc/systemd/system/data.mount.d/options.conf".into(),
        content: "[Mount]\nOptions=nofail\n".into(),
        disposition: FindingKind::included(),
        ..Default::default()
    };
    assert_eq!(
        mount_advisory_lines(&data, &nothing_staged, std::slice::from_ref(&drop_in)),
        [
            advisory_fact("data.mount", "enabled"),
            carry_line("enable", "data.mount"),
        ]
    );
    assert!(mount_advisory_lines(&data, &nothing_staged, &[]).contains(&NOFAIL_LINE.to_owned()));
    // A drop-in row for another unit does not count.
    let other = SystemdDropIn {
        unit: "other.mount".into(),
        ..drop_in.clone()
    };
    assert!(
        mount_advisory_lines(&data, &nothing_staged, &[other]).contains(&NOFAIL_LINE.to_owned())
    );
}

/// A finding more than one host recorded says, directly after its first
/// line, that it describes one host's copy. A finding with no fleet counts
/// (a single-host scan), or one host's finding on an aggregate, does not:
/// its copy is the only one.
#[test]
fn a_finding_more_than_one_host_recorded_says_so() {
    use inspectah_core::types::aggregate::AggregatePrevalence;
    let on_hosts = |hosts: &[&str]| {
        let mut finding = mount_finding(
            "srv.mount",
            local("srv.mount"),
            Some(UnitState::Enabled),
            Some("/srv"),
            false,
        );
        finding.aggregate = Some(AggregatePrevalence {
            count: hosts.len() as i32,
            total: 2,
            hosts: hosts.iter().map(|h| (*h).to_owned()).collect(),
            ..Default::default()
        });
        finding
    };
    let empty = InspectionSnapshot::new();
    let nothing_staged = staged_writes(&empty);
    assert_eq!(
        mount_advisory_lines(&on_hosts(&["host-a", "host-b"]), &nothing_staged, &[]),
        [
            advisory_fact("srv.mount", "enabled"),
            MERGED_LINE.into(),
            var_symlink_line("/srv"),
            NOFAIL_LINE.into(),
        ]
    );
    let one_host = mount_finding(
        "srv.mount",
        local("srv.mount"),
        Some(UnitState::Enabled),
        Some("/srv"),
        false,
    );
    for (case, finding) in [
        ("single-host scan", one_host),
        ("one host of an aggregate", on_hosts(&["host-a"])),
    ] {
        assert_eq!(
            mount_advisory_lines(&finding, &nothing_staged, &[]),
            [
                advisory_fact("srv.mount", "enabled"),
                var_symlink_line("/srv"),
                NOFAIL_LINE.into(),
            ],
            "{case}"
        );
    }
}

/// The unit state section header.
const UNIT_STATE_HEADER: &str = "# === Systemd Unit State ===";

/// The lines of `containerfile`'s unit state section, header excluded.
fn unit_state_section(containerfile: &str) -> Vec<String> {
    let mut lines = containerfile
        .lines()
        .skip_while(|l| *l != UNIT_STATE_HEADER);
    assert_eq!(
        lines.next(),
        Some(UNIT_STATE_HEADER),
        "no unit state section:\n{containerfile}"
    );
    lines
        .take_while(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// § 10.1: the unit state function emits the mount advisories (§ 10.6)
/// under the one section header, in this order: the declined lines, the
/// `systemctl` lines, then one block per finding, each block its first
/// line and the indented lines after it. No other section header is
/// written for them, and no instruction names the unit. The control on the
/// same snapshot: a package-owned mount unit is in `RUN systemctl enable`.
#[test]
fn mount_advisories_render_in_the_unit_state_section_after_its_commands() {
    const FIXME: &str = "# FIXME: bootc-fetch-apply-updates.timer is disabled on the source \
         host. Not carried: the image needs this unit.";
    let mut snap = units_snapshot(
        vec![
            change_with(
                "var-lib-pkg.mount",
                owned("var-lib-pkg.mount", "pkgmount"),
                UnitState::Enabled,
            ),
            change_with(
                "bootc-fetch-apply-updates.timer",
                vendor("bootc-fetch-apply-updates.timer"),
                UnitState::Disabled,
            ),
        ],
        vec![("/etc/systemd/system/srv.mount", "[Mount]\nWhere=/srv\n")],
    );
    let findings = &mut snap.units.as_mut().expect("a units section").mount_findings;
    findings.push(mount_finding(
        "srv.mount",
        local("srv.mount"),
        Some(UnitState::Enabled),
        Some("/srv"),
        true,
    ));
    findings.push(mount_finding(
        "old.mount",
        local("old.mount"),
        Some(UnitState::Disabled),
        None,
        true,
    ));
    let expected = [
        FIXME.to_owned(),
        "RUN systemctl enable -- \\".to_owned(),
        "    'var-lib-pkg.mount'".to_owned(),
        advisory_fact("srv.mount", "enabled"),
        FILE_STAGED_LINE.to_owned(),
        var_symlink_line("/srv"),
        advisory_fact("old.mount", "disabled"),
    ];
    assert_eq!(render(&snap).lines, expected, "unit_state_lines");

    let containerfile = render_containerfile(&snap, None, None);
    assert_eq!(unit_state_section(&containerfile), expected);
    let headers: Vec<&str> = containerfile
        .lines()
        .filter(|l| l.starts_with("# === "))
        .collect();
    assert_eq!(
        headers.iter().filter(|h| h.contains("Mount")).count(),
        0,
        "no section of their own: {headers:?}"
    );
    let lines: Vec<String> = containerfile.lines().map(str::to_owned).collect();
    assert!(acted_on(&lines, "var-lib-pkg.mount"), "{containerfile}");
    for unit in ["srv.mount", "old.mount"] {
        assert!(
            lines
                .iter()
                .filter(|l| l.contains(unit))
                .all(|l| l.starts_with('#')),
            "only comments name {unit}:\n{containerfile}"
        );
    }
}

/// A snapshot whose only unit content is a mount finding has the unit
/// state section, holding the advisory and nothing else. The plan carries
/// no command, which is what the README's unit state sentences read.
/// Control: the same snapshot with no finding has no section.
#[test]
fn an_advisory_alone_makes_a_comment_only_unit_state_section() {
    let mut snap = units_snapshot(vec![], vec![]);
    let control = render_containerfile(&snap, None, None);
    assert!(!control.contains(UNIT_STATE_HEADER), "{control}");

    snap.units
        .as_mut()
        .expect("a units section")
        .mount_findings
        .push(mount_finding(
            "srv.mount",
            local("srv.mount"),
            Some(UnitState::Enabled),
            Some("/srv"),
            true,
        ));
    let containerfile = render_containerfile(&snap, None, None);
    assert_eq!(
        unit_state_section(&containerfile),
        [
            advisory_fact("srv.mount", "enabled"),
            var_symlink_line("/srv"),
        ]
    );
    assert!(plan_units(&snap, &staged_writes(&snap)).planned.is_empty());
}

// ---------------------------------------------------------------------------
// § 10.4 runtime package updaters
// ---------------------------------------------------------------------------

/// An excluded runtime updater is not planned, and the section names it in
/// § 10.4's `# Omitted:` line, whatever its verb. Controls: the same row
/// included is planned with no line; an excluded row of a unit not on the
/// list renders nothing, as every excluded row does.
#[test]
fn an_excluded_runtime_updater_is_named_in_an_omitted_line() {
    let row = |unit: &str, state, include: bool| {
        let mut change = change_with(unit, vendor(unit), state);
        change.disposition = FindingKind::from_bool(include);
        change
    };
    let omitted = |unit: &str| {
        format!(
            "# Omitted: {unit} (updates packages in place, which an image-based host does not \
             support; excluded by default)"
        )
    };
    for (unit, state, verb) in [
        ("dnf-automatic.timer", UnitState::Enabled, "enable"),
        (
            "packagekit-offline-update.service",
            UnitState::Disabled,
            "disable",
        ),
    ] {
        let snap = units_snapshot(vec![row(unit, state, false)], vec![]);
        let plan = plan_units(&snap, &staged_writes(&snap));
        assert!(plan.planned.is_empty(), "{unit}");
        assert!(plan.declined.is_empty(), "{unit}: not a declined command");
        assert_eq!(unit_state_lines(&plan), [omitted(unit)], "{unit}");

        let snap = units_snapshot(vec![row(unit, state, true)], vec![]);
        assert_eq!(
            render(&snap).lines,
            [
                format!("RUN systemctl {verb} -- \\"),
                format!("    '{unit}'")
            ],
            "{unit} included"
        );
    }
    let other = units_snapshot(
        vec![row("dnf-automatic.service", UnitState::Enabled, false)],
        vec![],
    );
    assert!(render(&other).lines.is_empty());
}

// ---------------------------------------------------------------------------
// § 10.2 "Never the machine-id" (T33, the stager's half)
// ---------------------------------------------------------------------------

/// Each spelling T33 names for `path`: plain, relative, with a doubled
/// separator, with an interior `.`, and with a trailing separator.
fn t33_spellings(path: &str) -> [String; 5] {
    let (dir, name) = path.rsplit_once('/').expect("an absolute path");
    [
        path.to_owned(),
        path.trim_start_matches('/').to_owned(),
        format!("{dir}//{name}"),
        format!("{dir}/./{name}"),
        format!("{path}/"),
    ]
}

/// A snapshot that offers `path`, at every [`t33_spellings`] spelling and
/// with one content, through a config entry, a drop-in row, a repo file and
/// an unmanaged file: every section of an imported snapshot that can name
/// a path under `/etc`.
fn offering_everywhere(path: &str, content: &str) -> InspectionSnapshot {
    use inspectah_core::types::nonrpm::{FileType, UnmanagedFile, UnmanagedFileSection};
    use inspectah_core::types::rpm::RepoFile;
    let spellings = t33_spellings(path);
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: spellings
            .iter()
            .map(|p| ConfigFileEntry {
                path: p.clone(),
                category: ConfigCategory::Other,
                content: content.into(),
                disposition: FindingKind::included(),
                ..Default::default()
            })
            .collect(),
    });
    snap.units = Some(UnitSection {
        drop_ins: spellings
            .iter()
            .map(|p| SystemdDropIn {
                unit: "sshd.service".into(),
                path: p.clone(),
                content: content.into(),
                disposition: FindingKind::included(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    });
    snap.rpm = Some(RpmSection {
        repo_files: vec![RepoFile {
            path: path.into(),
            content: content.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap.unmanaged_files = Some(UnmanagedFileSection {
        items: spellings
            .iter()
            .map(|p| UnmanagedFile {
                path: p.clone(),
                file_type: FileType::Config,
                disposition: FindingKind::included(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    });
    snap
}

/// T33, the stager: `/etc/machine-id`, offered by every section that can
/// name it and in every spelling, is never a write. Every offer is
/// withheld as `MachineId`, one entry per offer: the guard runs before
/// offers of one destination are deduplicated, so offers with one content
/// do not collapse into one write and keep their reason each. The FIXME
/// text says why. Control: `/etc/machine-info`, offered the same ways, is
/// one write and withholds nothing (its unmanaged offers are the
/// unmanaged section's to copy).
#[test]
fn t33_the_stager_never_writes_the_machine_id() {
    use inspectah_pipeline::render::configtree::{Destination, StagedFileKind, WithheldWhy};
    let machine_id = Destination::new("/etc/machine-id").expect("a destination");
    let snap = offering_everywhere("/etc/machine-id", "uninitialized\n");
    let staged = staged_writes(&snap);
    assert!(
        !staged.writes.contains_key(&machine_id),
        "{:?}",
        staged.writes
    );
    assert!(staged.refused.is_empty(), "{:?}", staged.refused);
    let mut kinds: Vec<StagedFileKind> = Vec::new();
    for (dest, kind, why) in &staged.withheld {
        assert_eq!(dest, &machine_id, "{kind:?}");
        assert_eq!(why, &WithheldWhy::MachineId, "{kind:?}");
        kinds.push(*kind);
    }
    kinds.sort();
    let mut expected = [
        [StagedFileKind::ConfigFile; 5].as_slice(),
        &[StagedFileKind::RepoFile],
        &[StagedFileKind::DropIn; 5],
        &[StagedFileKind::UnmanagedFile; 5],
    ]
    .concat();
    expected.sort();
    assert_eq!(kinds, expected, "one withheld entry per offer");
    assert_eq!(WithheldWhy::MachineId.name(), "MachineId");
    assert_eq!(
        WithheldWhy::MachineId.describe(&machine_id),
        "the image keeps the base image's empty /etc/machine-id, which is what makes first \
         boot keep the rendered unit states; the source host's machine identity is not copied"
    );

    let control = offering_everywhere("/etc/machine-info", "PRETTY_HOSTNAME=web\n");
    let staged = staged_writes(&control);
    let machine_info = Destination::new("/etc/machine-info").expect("a destination");
    assert!(
        staged.writes.contains_key(&machine_info),
        "control: {:?}",
        staged.writes
    );
    assert!(staged.withheld.is_empty(), "control: {:?}", staged.withheld);
}

// ---------------------------------------------------------------------------
// § 10.2 rule 5 "A mask follows the package check" (T34)
// ---------------------------------------------------------------------------

/// A snapshot with the baseline `baseline`, one added package `owner` with
/// `disposition`, and `changes`, and the config entries `files`.
fn mask_snapshot(
    baseline: &[&str],
    owner: &str,
    disposition: FindingKind,
    changes: Vec<UnitStateChange>,
    files: Vec<(&str, &str)>,
) -> InspectionSnapshot {
    let mut snap = units_snapshot(changes, files);
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(baseline.iter().map(|s| s.to_string()).collect()),
        packages_added: vec![added(owner, "x86_64", disposition)],
        ..Default::default()
    });
    snap
}

/// T34 at the plan: a masked vendor unit whose owning package is excluded
/// and not in the baseline gets no `mask` and an `# Omitted:` line with the
/// package reason. Carried: the package included; excluded and in the
/// baseline; no known owner; an `Absent` fragment. Rules 3 and 4 decide
/// first: a protected unit whose package is excluded gets rule 3's FIXME,
/// and a mask whose path a staged file occupies, package excluded, is
/// `MaskPathOccupied`.
#[test]
fn t34_a_mask_follows_its_package() {
    const UNIT: &str = "kdump.service";
    const OWNER: &str = "kexec-tools";
    let masked =
        |unit: &str, fragment: UnitFragment| change_with(unit, fragment, UnitState::Masked);
    let mask_line = |unit: &str| {
        vec![
            "RUN systemctl mask -- \\".to_owned(),
            format!("    '{unit}'"),
        ]
    };

    let excluded = mask_snapshot(
        &["bash"],
        OWNER,
        FindingKind::excluded(),
        vec![masked(UNIT, owned(UNIT, OWNER))],
        vec![],
    );
    let lines = render(&excluded).lines;
    assert_eq!(
        lines,
        [format!(
            "# Omitted: {UNIT} (package '{OWNER}' was excluded)"
        )]
    );
    assert!(in_no_systemctl_instruction(&lines, UNIT));
    assert_eq!(
        declined_of(&excluded),
        [(
            UNIT.to_owned(),
            UnitAction::Mask,
            OmissionReason::PackageExcluded {
                package: OWNER.into()
            }
        )]
    );

    for (label, baseline, disposition, fragment) in [
        (
            "package included",
            vec!["bash"],
            FindingKind::included(),
            owned(UNIT, OWNER),
        ),
        (
            "excluded and in the baseline",
            vec!["bash", OWNER],
            FindingKind::excluded(),
            owned(UNIT, OWNER),
        ),
        (
            "no known owner",
            vec!["bash"],
            FindingKind::excluded(),
            vendor(UNIT),
        ),
        (
            "absent",
            vec!["bash"],
            FindingKind::excluded(),
            UnitFragment::Absent,
        ),
    ] {
        let snap = mask_snapshot(
            &baseline,
            OWNER,
            disposition,
            vec![masked(UNIT, fragment)],
            vec![],
        );
        assert_eq!(render(&snap).lines, mask_line(UNIT), "{label}");
        assert!(declined_of(&snap).is_empty(), "{label}");
    }

    const PROTECTED: &str = "ostree-remount.service";
    let protected = mask_snapshot(
        &["bash"],
        "ostree",
        FindingKind::excluded(),
        vec![masked(PROTECTED, owned(PROTECTED, "ostree"))],
        vec![],
    );
    assert_eq!(
        declined_of(&protected),
        [(
            PROTECTED.to_owned(),
            UnitAction::Mask,
            OmissionReason::TargetNeedsUnit
        )]
    );

    const PATH: &str = "/etc/systemd/system/kdump.service";
    let occupied = mask_snapshot(
        &["bash"],
        OWNER,
        FindingKind::excluded(),
        vec![masked(UNIT, owned(UNIT, OWNER))],
        vec![(PATH, "[Service]\nExecStart=/bin/true\n")],
    );
    assert_eq!(
        declined_of(&occupied),
        [(
            UNIT.to_owned(),
            UnitAction::Mask,
            OmissionReason::MaskPathOccupied { path: PATH.into() }
        )]
    );
}

// ---------------------------------------------------------------------------
// § 10.2 rule 5, for every fragment: the install drop-ins
// ---------------------------------------------------------------------------

/// The `/etc` drop-in directory of `unit`.
fn etc_drop_in(dir_unit: &str, file: &str) -> String {
    format!("/etc/systemd/system/{dir_unit}.d/{file}")
}

/// `change` with its recorded install drop-ins and vendor drop-in names.
fn with_install_files(
    mut change: UnitStateChange,
    install_drop_ins: &[&str],
    vendor_drop_ins: &[&str],
) -> UnitStateChange {
    change.install_drop_ins = install_drop_ins.iter().map(|p| (*p).to_owned()).collect();
    change.vendor_drop_ins = vendor_drop_ins.iter().map(|n| (*n).to_owned()).collect();
    change
}

/// Whether the plan carries `unit`, or the reason it declines it.
fn rule_5_outcome(snap: &InspectionSnapshot, unit: &str) -> Result<(), OmissionReason> {
    let plan = plan_units(snap, &staged_writes(snap));
    if plan.planned.iter().any(|u| u.name().as_str() == unit) {
        return Ok(());
    }
    plan.declined
        .into_iter()
        .find(|(name, _, _)| name.as_str() == unit)
        .map(|(_, _, reason)| Err(reason))
        .unwrap_or_else(|| panic!("{unit} is neither planned nor declined"))
}

/// T24, render half, with the T25 (k) first half and Review focus 3: the
/// install drop-ins the export stages for a unit are exactly the row's
/// `install_drop_ins`. A recorded file missing, an unrecorded install file
/// staged in the unit's own or the template's `.d`, and a recorded path
/// that is not a destination are each `InstallEvidenceChanged` naming the
/// path. A staged file is an install file when it holds an `[Install]`
/// section, by the lexical test, or carries a recorded vendor drop-in's
/// name. Files in the type-wide and dash-prefix directories, and a
/// `[Service]` override under a name of its own, are not install files.
/// For an instance: an unrecorded install file in the template's `.d`
/// fails, the same file recorded and staged passes, a local template
/// recorded for the instance and staged alone passes, and a unit file
/// staged at the instance's own name fails, as does a template file staged
/// under `/etc` for an instance recorded from its vendor template (T25 (h)).
#[test]
fn rule_5_needs_exactly_the_recorded_install_drop_ins() {
    use UnitState::{Disabled, Enabled};
    let install = "[Install]\nWantedBy=multi-user.target\n";
    let service_only = "[Service]\nNice=5\n";
    let listed = etc_drop_in("app.service", "10-install.conf");
    let extra = etc_drop_in("app.service", "20-more.conf");
    let vendor_named = etc_drop_in("app.service", "10-members.conf");
    let dotdot = "/etc/systemd/system/app.service.d/../10-install.conf";
    let typewide = "/etc/systemd/system/service.d/10-install.conf";
    let template_dir = etc_drop_in("w@.service", "10-install.conf");
    let etc_template = "/etc/systemd/system/w@.service";
    let own_name = "/etc/systemd/system/w@a.service";
    let dash = "/etc/systemd/system/a-.service.d/10-install.conf";
    let evidence = |path: &str| {
        Err(OmissionReason::InstallEvidenceChanged {
            path: path.to_owned(),
        })
    };
    let app = |state| change_with("app.service", vendor("app.service"), state);

    type Case<'a> = (
        &'a str,
        UnitStateChange,
        Vec<(&'a str, &'a str)>,
        Result<(), OmissionReason>,
    );
    let cases: Vec<Case> = vec![
        (
            "a WantedBy= only from a recorded, staged drop-in",
            with_install_files(app(Enabled), &[&listed], &[]),
            vec![(&listed, install)],
            Ok(()),
        ),
        (
            "the recorded drop-in excluded",
            with_install_files(app(Enabled), &[&listed], &[]),
            vec![],
            evidence(&listed),
        ),
        (
            "the recorded drop-in excluded, disable",
            with_install_files(app(Disabled), &[&listed], &[]),
            vec![],
            evidence(&listed),
        ),
        (
            "a second [Install] drop-in staged that the row does not list",
            with_install_files(app(Enabled), &[&listed], &[]),
            vec![(&listed, install), (&extra, install)],
            evidence(&extra),
        ),
        (
            "a recorded path holding ..",
            with_install_files(app(Enabled), &[dotdot], &[]),
            vec![],
            evidence(dotdot),
        ),
        (
            "an unrecorded [Service]-only drop-in with a vendor drop-in's name",
            with_install_files(app(Enabled), &[], &["10-members.conf"]),
            vec![(&vendor_named, service_only)],
            evidence(&vendor_named),
        ),
        (
            "T25 (k): the same file recorded and staged",
            with_install_files(app(Enabled), &[&vendor_named], &["10-members.conf"]),
            vec![(&vendor_named, service_only)],
            Ok(()),
        ),
        (
            "T25 (k): the same file recorded and excluded",
            with_install_files(app(Enabled), &[&vendor_named], &["10-members.conf"]),
            vec![],
            evidence(&vendor_named),
        ),
        (
            "an unrecorded [Service]-only drop-in with a name of its own",
            with_install_files(app(Enabled), &[], &["10-members.conf"]),
            vec![(&extra, service_only)],
            Ok(()),
        ),
        (
            "an [Install] drop-in in the type-wide directory",
            app(Enabled),
            vec![(typewide, install)],
            Ok(()),
        ),
        (
            "a type-wide file named like a vendor drop-in",
            with_install_files(app(Enabled), &[], &["10-install.conf"]),
            vec![(typewide, service_only)],
            Ok(()),
        ),
        (
            "an [Install] drop-in in a dash-prefix directory",
            change_with("a-b.service", vendor("a-b.service"), Enabled),
            vec![(dash, install)],
            Ok(()),
        ),
        (
            "the lexical test: surrounding whitespace",
            app(Enabled),
            vec![(&extra, "  [Install]  \nWantedBy=a.target\n")],
            evidence(&extra),
        ),
        (
            "the lexical test: a trailing CR",
            app(Enabled),
            vec![(&extra, "[Install]\r\nWantedBy=a.target\r\n")],
            evidence(&extra),
        ),
        (
            "the lexical test: a backslash-continued assignment",
            app(Enabled),
            vec![(&extra, "[Install]\nWantedBy=a.target \\\n  b.target\n")],
            evidence(&extra),
        ),
        (
            "an instance: an unrecorded [Install] drop-in in the template's .d",
            change_with("w@a.service", vendor("w@.service"), Enabled),
            vec![(&template_dir, install)],
            evidence(&template_dir),
        ),
        (
            "an instance: the template's .d drop-in recorded and staged",
            with_install_files(
                change_with("w@a.service", vendor("w@.service"), Enabled),
                &[&template_dir],
                &[],
            ),
            vec![(&template_dir, install)],
            Ok(()),
        ),
        (
            "an instance: a local template recorded for it, staged alone",
            change_with("w@a.service", local("w@.service"), Enabled),
            vec![(etc_template, install)],
            Ok(()),
        ),
        (
            "an instance: a unit file staged at its own name",
            change_with("w@a.service", local("w@.service"), Enabled),
            vec![(etc_template, install), (own_name, install)],
            evidence(own_name),
        ),
        (
            "an instance recorded from its vendor template, a template file staged",
            change_with("w@a.service", vendor("w@.service"), Enabled),
            vec![(etc_template, "[Install]\nAlso=x.service\n")],
            evidence(etc_template),
        ),
    ];
    for (label, change, files, expected) in cases {
        let unit = change.unit().name().as_str().to_owned();
        let snap = units_snapshot(vec![change], files);
        assert_eq!(rule_5_outcome(&snap, &unit), expected, "{label}");
    }
}

/// T44: a reset drop-in holding `[Install]` keeps `InstallEvidenceChanged`.
/// Rule 5 for what an instance reads: the collector records among a row's
/// `install_drop_ins` a runtime drop-in whose reset, replacement or hiding
/// changed a path the instance reads. One the export does not stage is
/// `InstanceNeedsEvidenceChanged` naming it when it is no install file: in
/// the unit's own or template `.d` under `/etc` with no `[Install]` and no
/// vendor drop-in's name, in a type-wide or dash-prefix directory, or above
/// `/etc` hiding no recorded `/etc` file of its name. An install file, and
/// a drop-in above `/etc` that hides one, keep `InstallEvidenceChanged`.
/// Each staged, the enable is carried.
#[test]
fn rule_5_names_a_runtime_drop_in_by_what_it_changes() {
    const UNIT: &str = "w@a.service";
    let reset = "[Service]\nEnvironmentFile=\n";
    let install = "[Install]\nWantedBy=multi-user.target\n";
    let own = etc_drop_in(UNIT, "10-reset.conf");
    let own_install = etc_drop_in(UNIT, "50-x.conf");
    let template_vendor_named = etc_drop_in("w@.service", "10-members.conf");
    let etc_type_wide = "/etc/systemd/system/service.d/90-reset.conf";
    let run_type_wide = "/run/systemd/system/service.d/90-reset.conf";
    let run_own = "/run/systemd/system/w@a.service.d/90-reset.conf";
    let control_hiding = "/etc/systemd/system.control/w@a.service.d/50-x.conf";
    let needs = |path: &str| {
        Err(OmissionReason::InstanceNeedsEvidenceChanged {
            path: path.to_owned(),
        })
    };
    let evidence = |path: &str| {
        Err(OmissionReason::InstallEvidenceChanged {
            path: path.to_owned(),
        })
    };
    let instance = |recorded: &[&str], vendor_names: &[&str]| {
        with_install_files(
            change_with(UNIT, vendor("w@.service"), UnitState::Enabled),
            recorded,
            vendor_names,
        )
    };
    type Case<'a> = (
        &'a str,
        UnitStateChange,
        Vec<(&'a str, &'a str)>,
        Vec<SystemdDropIn>,
        Result<(), OmissionReason>,
    );
    let cases: Vec<Case> = vec![
        (
            "a reset in the own .d, its row excluded",
            instance(&[&own], &[]),
            vec![],
            vec![drop_in_row(UNIT, &own, reset, false)],
            needs(&own),
        ),
        (
            "a reset in the own .d, its row included",
            instance(&[&own], &[]),
            vec![],
            vec![drop_in_row(UNIT, &own, reset, true)],
            Ok(()),
        ),
        (
            "an [Install] drop-in in the own .d, its row excluded",
            instance(&[&own], &[]),
            vec![],
            vec![drop_in_row(UNIT, &own, install, false)],
            evidence(&own),
        ),
        (
            "a [Service] drop-in with a vendor drop-in's name, excluded",
            instance(&[&template_vendor_named], &["10-members.conf"]),
            vec![],
            vec![drop_in_row(UNIT, &template_vendor_named, reset, false)],
            evidence(&template_vendor_named),
        ),
        (
            "a type-wide reset under /etc, its entry excluded",
            instance(&[etc_type_wide], &[]),
            vec![],
            vec![],
            needs(etc_type_wide),
        ),
        (
            "a type-wide reset under /etc, staged",
            instance(&[etc_type_wide], &[]),
            vec![(etc_type_wide, reset)],
            vec![],
            Ok(()),
        ),
        (
            "a type-wide reset under /run",
            instance(&[run_type_wide], &[]),
            vec![],
            vec![],
            needs(run_type_wide),
        ),
        (
            "a reset in the own .d under /run, hiding nothing",
            instance(&[run_own], &[]),
            vec![],
            vec![],
            needs(run_own),
        ),
        (
            "a system.control drop-in hiding a recorded /etc install drop-in",
            instance(&[&own_install, control_hiding], &[]),
            vec![(&own_install, install)],
            vec![],
            evidence(control_hiding),
        ),
    ];
    for (label, change, files, rows, expected) in cases {
        let snap = with_drop_in_rows(units_snapshot(vec![change], files), rows);
        assert_eq!(rule_5_outcome(&snap, UNIT), expected, "{label}");
    }
}

/// § 7.5, phase 5: an instance row is planned like any row. Its
/// `disable`, and its `mask` over the template's file or with no file, are
/// carried; nothing declines a row for being an instance.
#[test]
fn an_instance_disable_or_mask_is_planned_like_any_row() {
    use UnitState::{Disabled, Masked};
    for (label, change) in [
        (
            "a disable",
            change_with("w@a.service", vendor("w@.service"), Disabled),
        ),
        (
            "a mask over the template",
            change_with("w@a.service", vendor("w@.service"), Masked),
        ),
        (
            "a mask with no file",
            change_with("w@a.service", UnitFragment::Absent, Masked),
        ),
    ] {
        let snap = units_snapshot(vec![change], vec![]);
        assert_eq!(rule_5_outcome(&snap, "w@a.service"), Ok(()), "{label}");
    }
}

/// § 10.2 rule 6 and § 10.8's table, on the plan: an instance's `enable`
/// is carried only when every `required_config` path is in the export as
/// the host had it. Each failing case names the path; a directory passes
/// on the entries under it. A `Flagged` finding and a scan taken with no
/// redaction carry the enable with a note naming the path. Rule 6 asks
/// nothing of a `disable`, nor of a unit that is not an instance.
#[test]
fn rule_6_needs_each_instance_configuration_in_the_export() {
    use inspectah_core::types::redaction::{
        DetectionMethod, RedactionFinding, RedactionKind, RedactionState,
    };
    use inspectah_pipeline::render::unit_plan::ConfigGap;
    const CONF: &str = "/etc/w/a.conf";
    const DIR: &str = "/etc/w/a.d";
    let instance = |required: &[&str], state| {
        let mut change = change_with("w@a.service", vendor("w@.service"), state);
        change.required_config = required.iter().map(|p| (*p).to_owned()).collect();
        change
    };
    let finding = |path: &str, kind| RedactionFinding {
        path: path.into(),
        source: "file".into(),
        kind,
        pattern: "test".into(),
        remediation: String::new(),
        line: None,
        replacement: None,
        detection_method: DetectionMethod::Pattern,
        confidence: None,
        finding_kind: None,
    };
    let gap = |path: &str, gap| {
        Err(OmissionReason::InstanceConfigNotStaged {
            path: path.to_owned(),
            gap,
        })
    };
    let notes = |snap: &InspectionSnapshot| plan_units(snap, &staged_writes(snap)).instance_notes;

    let clean = units_snapshot(
        vec![instance(&[CONF], UnitState::Enabled)],
        vec![(CONF, "x")],
    );
    assert_eq!(rule_5_outcome(&clean, "w@a.service"), Ok(()), "clean");
    assert!(notes(&clean).is_empty());

    let missing = units_snapshot(vec![instance(&[CONF], UnitState::Enabled)], vec![]);
    assert_eq!(
        rule_5_outcome(&missing, "w@a.service"),
        gap(CONF, ConfigGap::NotInSnapshot),
        "not in the snapshot"
    );

    let mut excluded = clean.clone();
    excluded.config.as_mut().unwrap().files[0].disposition = FindingKind::excluded();
    assert_eq!(
        rule_5_outcome(&excluded, "w@a.service"),
        gap(CONF, ConfigGap::Excluded),
        "excluded"
    );

    let mut uncaptured = clean.clone();
    uncaptured.config.as_mut().unwrap().files[0].disposition = FindingKind::advisory(
        inspectah_core::types::AdvisoryType::ContentNotCaptured,
        "the read failed",
    );
    assert_eq!(
        rule_5_outcome(&uncaptured, "w@a.service"),
        gap(CONF, ConfigGap::NotCaptured),
        "not captured"
    );

    for kind in [RedactionKind::Inline, RedactionKind::Excluded] {
        let mut redacted = clean.clone();
        redacted.redactions = vec![finding(CONF, kind.clone())];
        assert_eq!(
            rule_5_outcome(&redacted, "w@a.service"),
            gap(CONF, ConfigGap::Redacted),
            "{kind:?}"
        );
    }

    let mut flagged = clean.clone();
    flagged.redactions = vec![finding(CONF, RedactionKind::Flagged)];
    assert_eq!(rule_5_outcome(&flagged, "w@a.service"), Ok(()), "flagged");
    assert_eq!(
        notes(&flagged),
        [(
            UnitName::try_from("w@a.service".to_owned()).unwrap(),
            format!(
                "Its configuration {CONF} holds a value the redaction pass flagged and did not \
                 change. Check it before the image is used."
            )
        )]
    );

    let mut raw = clean.clone();
    raw.redaction_state = Some(RedactionState::Raw);
    assert_eq!(rule_5_outcome(&raw, "w@a.service"), Ok(()), "raw");
    assert_eq!(
        notes(&raw)[0].1,
        format!(
            "Its configuration {CONF} is staged as the source host had it: the scan was taken \
             with --no-redaction, so no secret in it was redacted."
        )
    );

    // A directory: the entries under it decide, and the one that falls
    // short is named.
    let under = |files| units_snapshot(vec![instance(&[DIR], UnitState::Enabled)], files);
    let both = under(vec![
        ("/etc/w/a.d/one.conf", "x"),
        ("/etc/w/a.d/two.conf", "y"),
    ]);
    assert_eq!(rule_5_outcome(&both, "w@a.service"), Ok(()), "directory");
    let mut one_out = both.clone();
    one_out.config.as_mut().unwrap().files[1].disposition = FindingKind::excluded();
    assert_eq!(
        rule_5_outcome(&one_out, "w@a.service"),
        gap("/etc/w/a.d/two.conf", ConfigGap::Excluded),
        "a file under the directory excluded"
    );
    let sibling = under(vec![("/etc/w/a.dx", "x")]);
    assert_eq!(
        rule_5_outcome(&sibling, "w@a.service"),
        gap(DIR, ConfigGap::NotInSnapshot),
        "a sibling with a longer name is not under it"
    );
    // A member the stager refuses (a line break in its name) is not
    // staged, so it declines the enable naming it, though the other member
    // is staged. Control: the same member with an acceptable name.
    let refused = under(vec![
        ("/etc/w/a.d/good.conf", "x"),
        ("/etc/w/a.d/bad\nname.conf", "y"),
    ]);
    assert_eq!(
        rule_5_outcome(&refused, "w@a.service"),
        gap("/etc/w/a.d/bad\nname.conf", ConfigGap::Excluded),
        "a refused member"
    );
    let renamed = under(vec![
        ("/etc/w/a.d/good.conf", "x"),
        ("/etc/w/a.d/badname.conf", "y"),
    ]);
    assert_eq!(rule_5_outcome(&renamed, "w@a.service"), Ok(()), "renamed");
    // A path with a `..` component cannot be verified: its own reason,
    // although the file it reads as is staged. Control: the plain path.
    let parent = units_snapshot(
        vec![instance(&["/etc/w/../w/a.conf"], UnitState::Enabled)],
        vec![(CONF, "x")],
    );
    assert_eq!(
        rule_5_outcome(&parent, "w@a.service"),
        gap("/etc/w/../w/a.conf", ConfigGap::Unverifiable),
        "a .. component"
    );
    let plain_path = units_snapshot(
        vec![instance(&[CONF], UnitState::Enabled)],
        vec![(CONF, "x")],
    );
    assert_eq!(
        rule_5_outcome(&plain_path, "w@a.service"),
        Ok(()),
        "plain path"
    );
    let refused_elsewhere = under(vec![
        ("/etc/w/a.d/good.conf", "x"),
        ("/etc/w/other/bad\nname.conf", "y"),
    ]);
    assert_eq!(
        rule_5_outcome(&refused_elsewhere, "w@a.service"),
        Ok(()),
        "a refused file outside the directory"
    );

    // Rule 6 asks nothing of a `disable`, nor of a unit that is not an
    // instance.
    let disabled = units_snapshot(vec![instance(&[CONF], UnitState::Disabled)], vec![]);
    assert_eq!(rule_5_outcome(&disabled, "w@a.service"), Ok(()), "disable");
    let mut plain = change_with("w.service", vendor("w.service"), UnitState::Enabled);
    plain.required_config = vec![CONF.to_owned()];
    let plain = units_snapshot(vec![plain], vec![]);
    assert_eq!(
        rule_5_outcome(&plain, "w.service"),
        Ok(()),
        "not an instance"
    );

    // The texts.
    for (gap, text) in [
        (
            ConfigGap::Excluded,
            "its configuration /etc/w/a.conf is excluded from the export",
        ),
        (
            ConfigGap::NotCaptured,
            "its configuration /etc/w/a.conf was not captured",
        ),
        (
            ConfigGap::Redacted,
            "its configuration /etc/w/a.conf holds a redacted value; provision the secret on \
             the deployed host and enable the unit there",
        ),
        (
            ConfigGap::NotInSnapshot,
            "its configuration /etc/w/a.conf is not in the export",
        ),
        (
            ConfigGap::Unverifiable,
            "its configuration path /etc/w/a.conf holds a .. component, so inspectah cannot \
             tell which file it reads",
        ),
    ] {
        let reason = OmissionReason::InstanceConfigNotStaged {
            path: CONF.to_owned(),
            gap,
        };
        assert_eq!(reason.describe(), text, "{gap:?}");
        assert_eq!(reason.name(), "InstanceConfigNotStaged");
    }
}

/// T44, in the plan alone: each cause omits an `enable`, and a `disable` is carried.
/// § 7.5 and § 10.2 rule 6, on the plan: an instance row whose
/// `needs_unknown` is set has its `enable` omitted as
/// `InstanceNeedsUnknown`, in either form, even with a `required_config`
/// path staged; the `# Omitted:` text names the setting, the file and the
/// line, or the file that could not be read. Its `disable` is carried, as
/// rule 6 asks nothing of one. Control: the same row without it is carried.
#[test]
fn rule_6_omits_an_instance_whose_needs_are_unknown() {
    use inspectah_core::types::units::InstanceNeedsUnknown;
    const CONF: &str = "/etc/w/a.conf";
    let instance = |needs_unknown: Option<InstanceNeedsUnknown>, state| {
        let mut change = change_with("w@a.service", vendor("w@.service"), state);
        change.required_config = vec![CONF.to_owned()];
        change.needs_unknown = needs_unknown;
        change
    };
    let control = units_snapshot(vec![instance(None, UnitState::Enabled)], vec![(CONF, "x")]);
    assert_eq!(rule_5_outcome(&control, "w@a.service"), Ok(()));

    let name = UnitName::try_from("w@a.service".to_owned()).unwrap();
    for (cause, text) in [
        (
            InstanceNeedsUnknown::Uninterpretable {
                key: "ExecStart".to_owned(),
                path: "/usr/lib/systemd/system/w@.service".to_owned(),
                line: 4,
            },
            "its ExecStart= setting in /usr/lib/systemd/system/w@.service (line 4) names a path \
             for the instance that inspectah cannot read as systemd does, so what the instance \
             reads is unknown and its enable is not carried",
        ),
        (
            InstanceNeedsUnknown::Unreadable {
                path: "/etc/systemd/system/service.d".to_owned(),
            },
            "/etc/systemd/system/service.d, a drop-in directory or drop-in systemd reads for the \
             instance, could not be read on the source host, so what the instance reads is \
             unknown and its enable is not carried",
        ),
    ] {
        let reason = OmissionReason::InstanceNeedsUnknown {
            cause: cause.clone(),
        };
        let snap = units_snapshot(
            vec![instance(Some(cause.clone()), UnitState::Enabled)],
            vec![(CONF, "x")],
        );
        assert_eq!(
            rule_5_outcome(&snap, "w@a.service"),
            Err(reason.clone()),
            "{cause:?}"
        );
        assert_eq!(reason.describe(), text, "{cause:?}");
        assert_eq!(reason.name(), "InstanceNeedsUnknown");
        let line = declined_line(&name, UnitAction::Enable, &reason);
        assert_eq!(line.marker, DeclinedMarker::Omitted, "{cause:?}");
        assert_eq!(line.text, format!("w@a.service ({text})"), "{cause:?}");

        let disabled = units_snapshot(
            vec![instance(Some(cause.clone()), UnitState::Disabled)],
            vec![],
        );
        assert_eq!(
            rule_5_outcome(&disabled, "w@a.service"),
            Ok(()),
            "{cause:?} disable"
        );
    }
}

/// § 10.8 "Data under `/var`", on the plan: a carried instance enable
/// with `data_paths` gets one data advisory naming every path, written as
/// one `# Advisory:` line before the `RUN` lines. No advisory for an
/// instance whose enable is declined, for a `disable`, or for an enable
/// with no `data_paths`.
#[test]
fn a_carried_instance_with_data_paths_has_one_data_advisory() {
    use inspectah_pipeline::render::unit_plan::unit_state_lines;
    const CONF: &str = "/etc/w/a.conf";
    let instance = |data: &[&str], state| {
        let mut change = change_with("w@a.service", vendor("w@.service"), state);
        change.required_config = vec![CONF.to_owned()];
        change.data_paths = data.iter().map(|p| (*p).to_owned()).collect();
        change
    };
    let plan_of = |snap: &InspectionSnapshot| plan_units(snap, &staged_writes(snap));
    let sentence = "w@a.service keeps data under /var/lib/w/a and /var/lib/w/b. The image does \
                    not carry them, and the carried enable may start the unit on first boot. Put \
                    the data in place before that boot, or the unit starts without it.";

    let carried = units_snapshot(
        vec![instance(
            &["/var/lib/w/a", "/var/lib/w/b"],
            UnitState::Enabled,
        )],
        vec![(CONF, "x")],
    );
    let plan = plan_of(&carried);
    assert_eq!(
        plan.data_advisories,
        [(
            UnitName::try_from("w@a.service".to_owned()).unwrap(),
            sentence.to_owned()
        )]
    );
    let lines = unit_state_lines(&plan);
    let advisory = format!("# Advisory: {sentence}");
    let at = lines
        .iter()
        .position(|l| *l == advisory)
        .unwrap_or_else(|| panic!("no data advisory: {lines:#?}"));
    let run = lines
        .iter()
        .position(|l| l.starts_with("RUN systemctl enable"))
        .unwrap_or_else(|| panic!("no RUN line: {lines:#?}"));
    assert!(at < run, "{lines:#?}");

    let one = units_snapshot(
        vec![instance(&["/var/lib/w/a"], UnitState::Enabled)],
        vec![(CONF, "x")],
    );
    assert!(
        plan_of(&one).data_advisories[0]
            .1
            .starts_with("w@a.service keeps data under /var/lib/w/a. The image does not carry it,"),
        "{:?}",
        plan_of(&one).data_advisories
    );
    let name = UnitName::try_from("w@a.service".to_owned()).unwrap();
    assert_eq!(
        inspectah_pipeline::render::unit_plan::data_advisory(&name, &[]),
        None,
        "no paths, no sentence"
    );

    let declined = units_snapshot(
        vec![instance(&["/var/lib/w/a"], UnitState::Enabled)],
        vec![],
    );
    assert!(plan_of(&declined).data_advisories.is_empty(), "declined");
    let disabled = units_snapshot(
        vec![instance(&["/var/lib/w/a"], UnitState::Disabled)],
        vec![],
    );
    assert!(plan_of(&disabled).data_advisories.is_empty(), "disable");
    let no_data = units_snapshot(vec![instance(&[], UnitState::Enabled)], vec![(CONF, "x")]);
    assert!(plan_of(&no_data).data_advisories.is_empty(), "no data");
}

/// § 10.2 rule 6 on a fleet aggregate: an instance's configuration whose
/// path holds more than one content variant declines the enable,
/// whichever variant is staged, since no one host's file is the one every
/// instance reads (as rule 5 declines an install file with two variants).
/// Control: one variant carries the enable.
#[test]
fn rule_6_declines_a_fleet_configuration_with_two_variants() {
    use inspectah_pipeline::render::unit_plan::ConfigGap;
    const CONF: &str = "/etc/w/a.conf";
    let fleet = |variants: &[&str]| {
        let mut change = change_with("w@a.service", vendor("w@.service"), UnitState::Enabled);
        change.required_config = vec![CONF.to_owned()];
        let mut snap = units_snapshot(vec![change], vec![(CONF, variants[0])]);
        snap.aggregate_meta = Some(AggregateSnapshotMeta {
            label: "fleet".into(),
            host_count: 2,
            hostnames: vec!["alpha".into(), "beta".into()],
            merged_at: "2026-11-15T00:00:00Z".into(),
            baseline_provisional: false,
            section_host_counts: Default::default(),
        });
        for content in &variants[1..] {
            snap.config
                .as_mut()
                .expect("a config section")
                .files
                .push(ConfigFileEntry {
                    path: CONF.into(),
                    category: ConfigCategory::Other,
                    content: (*content).into(),
                    disposition: FindingKind::Actionable { include: false },
                    ..Default::default()
                });
        }
        snap
    };
    assert_eq!(rule_5_outcome(&fleet(&["x"]), "w@a.service"), Ok(()), "one");
    let reason = OmissionReason::InstanceConfigNotStaged {
        path: CONF.to_owned(),
        gap: ConfigGap::FleetVariants,
    };
    assert_eq!(
        rule_5_outcome(&fleet(&["x", "y"]), "w@a.service"),
        Err(reason.clone()),
        "two"
    );
    assert_eq!(
        reason.describe(),
        "its configuration /etc/w/a.conf differs between hosts in the fleet, so the staged \
         copy may not be the one this instance reads"
    );
}

/// § 10.2 rule 5: in a fleet aggregate, a recorded install drop-in whose
/// path holds more than one content variant fails the rule, whichever
/// variant is staged, since the row may have been recorded from the
/// other. Control: the same snapshot with one variant carries the enable.
#[test]
fn rule_5_declines_a_recorded_install_drop_in_with_two_variants() {
    let listed = etc_drop_in("app.service", "10-install.conf");
    let change = || {
        with_install_files(
            change_with("app.service", vendor("app.service"), UnitState::Enabled),
            &[&listed],
            &[],
        )
    };
    let aggregate = |snap: &mut InspectionSnapshot| {
        snap.aggregate_meta = Some(AggregateSnapshotMeta {
            label: "fleet".into(),
            host_count: 2,
            hostnames: vec!["alpha".into(), "beta".into()],
            merged_at: "2026-11-15T00:00:00Z".into(),
            baseline_provisional: false,
            section_host_counts: Default::default(),
        });
    };

    let mut one = units_snapshot(
        vec![change()],
        vec![(&listed, "[Install]\nWantedBy=multi-user.target\n")],
    );
    aggregate(&mut one);
    assert_eq!(rule_5_outcome(&one, "app.service"), Ok(()));

    let mut two = units_snapshot(
        vec![change()],
        vec![(&listed, "[Install]\nWantedBy=multi-user.target\n")],
    );
    aggregate(&mut two);
    two.config
        .as_mut()
        .expect("a config section")
        .files
        .push(ConfigFileEntry {
            path: listed.clone(),
            category: ConfigCategory::Other,
            content: "[Install]\nWantedBy=graphical.target\n".into(),
            disposition: FindingKind::Actionable { include: false },
            ..Default::default()
        });
    assert_eq!(
        rule_5_outcome(&two, "app.service"),
        Err(OmissionReason::InstallEvidenceChanged { path: listed })
    );
}

// ---------------------------------------------------------------------------
// § 10.2 rule 7, `Also=` members (T25 (h) and (i), and the line rules)
// ---------------------------------------------------------------------------

/// An `Also=` member recorded with `links`.
fn also_member(unit: &str, links: inspectah_core::types::units::MemberLinks) -> AlsoMember {
    AlsoMember {
        unit: unit.to_owned().try_into().expect("a unit name"),
        links,
        vendor_drop_ins: Vec::new(),
    }
}

/// A vendor `unit` in `state` whose `also` list is `also`.
fn owner(unit: &str, state: UnitState, also: Vec<AlsoMember>) -> UnitStateChange {
    let mut change = change_with(unit, vendor(unit), state);
    change.also = also;
    change
}

/// A drop-in row at `path` for `unit`, holding `content`, included or not.
fn drop_in_row(unit: &str, path: &str, content: &str, included: bool) -> SystemdDropIn {
    SystemdDropIn {
        unit: unit.into(),
        path: path.into(),
        content: content.into(),
        disposition: if included {
            FindingKind::included()
        } else {
            FindingKind::excluded()
        },
        ..Default::default()
    }
}

/// `snap` with `rows` added to its drop-in rows.
fn with_drop_in_rows(mut snap: InspectionSnapshot, rows: Vec<SystemdDropIn>) -> InspectionSnapshot {
    snap.units
        .as_mut()
        .expect("a units section")
        .drop_ins
        .extend(rows);
    snap
}

/// The `(member, why)` of `unit`'s rule 7 decline, if any.
fn also_why(snap: &InspectionSnapshot, unit: &str) -> Option<(String, AlsoWhy)> {
    declined_of(snap)
        .into_iter()
        .find_map(|(name, _, reason)| match reason {
            OmissionReason::AlsoMember { member, why, .. } if name == unit => {
                Some((member.as_str().to_owned(), why))
            }
            _ => None,
        })
}

/// The section's lines from the first that starts with `first`, through
/// the indented lines after it.
fn block_at(lines: &[String], first: &str) -> Vec<String> {
    let mut rest = lines.iter().skip_while(|l| !l.starts_with(first));
    let Some(head) = rest.next() else {
        return Vec::new();
    };
    std::iter::once(head)
        .chain(rest.take_while(|l| l.starts_with("#   ")))
        .cloned()
        .collect()
}

/// § 10.2's header and footer around the lines to add.
const ALSO_HEADER: &str = "#   To reproduce the source host, add after the configuration COPY:";
const ALSO_FOOTER: &str =
    "#   Each command follows Also= in its turn. Check the result with systemctl is-enabled.";

/// The header in their place when the lines cannot reproduce the source
/// host: a member is `Other`, or a line is withheld.
const ALSO_PARTIAL_HEADER: &str = "#   These lines do not reproduce the source host. Check each \
     unit, then add after the configuration COPY:";

/// T25 (i): `mask` does not follow `Also=`, so a mask on a unit with
/// members in any state is carried.
#[test]
fn t25_i_a_mask_on_a_unit_with_members_is_carried() {
    use inspectah_core::types::units::MemberLinks::{Inert, Installed, Other, Unlinked};
    let snap = units_snapshot(
        vec![owner(
            "bundle.service",
            UnitState::Masked,
            vec![
                also_member("a.service", Installed),
                also_member("b.service", Unlinked),
                also_member("c.service", Inert),
                also_member("d.service", Other),
            ],
        )],
        vec![],
    );
    let rendered = render(&snap);
    assert!(rendered.omissions.is_empty(), "{:?}", rendered.omissions);
    assert!(
        acted_on(&rendered.lines, "bundle.service"),
        "{:?}",
        rendered.lines
    );
}

/// § 10.2 rule 7's line rules, on hand-built rows. Under `enable`:
/// - several `Unlinked` members: the reason names the first, the first
///   line names every one, joined with commas and `and`, in the plural,
///   and each has a `disable` line;
/// - an `Other` member first and an `Unlinked` one after: the first line
///   names only the `Other` one, and the `Unlinked` one keeps its line,
///   under the header that says the lines do not reproduce the source
///   host, with no footer;
/// - two `Other` members: one line naming both, no lines to add.
///
/// Under `disable`, `Installed` members get the `enable` line, and every
/// member `Unlinked` or `Inert` is carried.
#[test]
fn rule_7_line_rules() {
    use inspectah_core::types::units::MemberLinks::{Inert, Installed, Other, Unlinked};
    let plan_of = |state, also| {
        let snap = units_snapshot(vec![owner("bundle.service", state, also)], vec![]);
        let lines = render(&snap).lines;
        (also_why(&snap, "bundle.service"), lines)
    };
    let head = |verb: &str, named: &str, clause: &str| {
        format!(
            "# Advisory: bundle.service is {verb}d on the source host. Not carried: systemctl \
             {verb} on it also {verb}s {named} (named by Also=), {clause}."
        )
    };
    let add = |verb: &str, unit: &str| format!("#     RUN systemctl {verb} -- '{unit}'");

    let (why, lines) = plan_of(
        UnitState::Enabled,
        vec![
            also_member("a.service", Installed),
            also_member("b.service", Unlinked),
            also_member("d.service", Unlinked),
            also_member("e.service", Unlinked),
        ],
    );
    assert_eq!(
        why,
        Some(("b.service".into(), AlsoWhy::LinksDiffer(Unlinked)))
    );
    assert_eq!(
        block_at(&lines, "# Advisory: bundle.service"),
        [
            head(
                "enable",
                "b.service, d.service and e.service",
                "which are not enabled there"
            ),
            ALSO_HEADER.to_owned(),
            add("enable", "bundle.service"),
            add("disable", "b.service"),
            add("disable", "d.service"),
            add("disable", "e.service"),
            ALSO_FOOTER.to_owned(),
        ]
    );
    assert!(!acted_on(&lines, "bundle.service"), "{lines:?}");

    let (why, lines) = plan_of(
        UnitState::Enabled,
        vec![
            also_member("c.service", Other),
            also_member("b.service", Unlinked),
        ],
    );
    assert_eq!(why, Some(("c.service".into(), AlsoWhy::LinksDiffer(Other))));
    assert_eq!(
        block_at(&lines, "# Advisory: bundle.service"),
        [
            head(
                "enable",
                "c.service",
                "whose links there neither command reproduces, and b.service, which is not \
                 enabled there"
            ),
            ALSO_PARTIAL_HEADER.to_owned(),
            add("enable", "bundle.service"),
            add("disable", "b.service"),
        ]
    );

    let (_, lines) = plan_of(
        UnitState::Enabled,
        vec![
            also_member("b.service", Unlinked),
            also_member("c.service", Other),
        ],
    );
    assert_eq!(
        block_at(&lines, "# Advisory: bundle.service"),
        [
            head(
                "enable",
                "b.service",
                "which is not enabled there, and c.service, whose links there neither command \
                 reproduces"
            ),
            ALSO_PARTIAL_HEADER.to_owned(),
            add("enable", "bundle.service"),
            add("disable", "b.service"),
        ],
        "an Other member after the Unlinked one"
    );

    let (_, lines) = plan_of(
        UnitState::Enabled,
        vec![
            also_member("c.service", Other),
            also_member("f.service", Other),
        ],
    );
    assert_eq!(
        block_at(&lines, "# Advisory: bundle.service"),
        [head(
            "enable",
            "c.service and f.service",
            "whose links there neither command reproduces"
        )]
    );

    let (why, lines) = plan_of(
        UnitState::Disabled,
        vec![
            also_member("a.service", Installed),
            also_member("b.service", Unlinked),
            also_member("g.service", Installed),
        ],
    );
    assert_eq!(
        why,
        Some(("a.service".into(), AlsoWhy::LinksDiffer(Installed)))
    );
    assert_eq!(
        block_at(&lines, "# Advisory: bundle.service"),
        [
            head(
                "disable",
                "a.service and g.service",
                "which are enabled there"
            ),
            ALSO_HEADER.to_owned(),
            add("disable", "bundle.service"),
            add("enable", "a.service"),
            add("enable", "g.service"),
            ALSO_FOOTER.to_owned(),
        ]
    );

    let (why, lines) = plan_of(
        UnitState::Disabled,
        vec![
            also_member("b.service", Unlinked),
            also_member("c.service", Inert),
        ],
    );
    assert_eq!(why, None);
    assert!(acted_on(&lines, "bundle.service"), "{lines:?}");
}

/// Rule 7's lines never suggest disabling a protected unit (§ 10.3's
/// list, `target_needs_unit`): an enabled owner with an `Unlinked`
/// `bootc-x.timer` names it and says the image needs it, with no `disable`
/// line, under the header that says the lines do not reproduce the source
/// host and with no footer. The ordinary `Unlinked` member beside it keeps
/// its `disable` line. Control: the ordinary member alone gets the full
/// recipe with its header and footer.
#[test]
fn rule_7_never_suggests_disabling_a_protected_member() {
    use inspectah_core::types::units::MemberLinks::Unlinked;
    let plan_of = |also| {
        let snap = units_snapshot(
            vec![owner("bundle.service", UnitState::Enabled, also)],
            vec![],
        );
        render(&snap).lines
    };
    let add = |verb: &str, unit: &str| format!("#     RUN systemctl {verb} -- '{unit}'");

    let control = plan_of(vec![also_member("b.service", Unlinked)]);
    assert_eq!(
        block_at(&control, "# Advisory: bundle.service")[1..],
        [
            ALSO_HEADER.to_owned(),
            add("enable", "bundle.service"),
            add("disable", "b.service"),
            ALSO_FOOTER.to_owned(),
        ]
    );

    let lines = plan_of(vec![
        also_member("bootc-x.timer", Unlinked),
        also_member("b.service", Unlinked),
    ]);
    assert_eq!(
        block_at(&lines, "# Advisory: bundle.service"),
        [
            "# Advisory: bundle.service is enabled on the source host. Not carried: systemctl \
             enable on it also enables bootc-x.timer and b.service (named by Also=), which are \
             not enabled there."
                .to_owned(),
            ALSO_PARTIAL_HEADER.to_owned(),
            add("enable", "bundle.service"),
            add("disable", "b.service"),
            "#   No disable line for bootc-x.timer: the image needs this unit.".to_owned(),
        ]
    );
    assert!(
        lines
            .iter()
            .all(|l| !(l.contains("disable") && l.contains("'bootc-x.timer'"))),
        "{lines:?}"
    );
}

/// The collector's drop-in name rule holds in the export's checks: a
/// staged `.conf` whose name starts with `.` (an editor's
/// `.#override.conf`) is no drop-in systemd reads, so it is not an install
/// file for rule 5's added-file half or rule 7's condition 3, whatever it
/// holds. Control: the same file without the leading `.` is one, and
/// declines the command each way.
#[test]
fn hidden_drop_in_names_are_never_install_files() {
    use inspectah_core::types::units::MemberLinks::Installed;
    let install = "[Install]\nWantedBy=multi-user.target\n";
    for (file, hidden) in [(".#override.conf", true), ("override.conf", false)] {
        let path = etc_drop_in("app.service", file);
        let rule_5 = units_snapshot(
            vec![change_with(
                "app.service",
                vendor("app.service"),
                UnitState::Enabled,
            )],
            vec![(&path, install)],
        );
        let expected = if hidden {
            Ok(())
        } else {
            Err(OmissionReason::InstallEvidenceChanged { path: path.clone() })
        };
        assert_eq!(rule_5_outcome(&rule_5, "app.service"), expected, "{file}");

        let member = etc_drop_in("m.service", file);
        let rule_7 = units_snapshot(
            vec![owner(
                "app.service",
                UnitState::Enabled,
                vec![also_member("m.service", Installed)],
            )],
            vec![(&member, install)],
        );
        let why = also_why(&rule_7, "app.service");
        if hidden {
            assert_eq!(why, None, "{file}: {:?}", declined_of(&rule_7));
        } else {
            assert_eq!(
                why,
                Some(("m.service".into(), AlsoWhy::InstallFile { path: member })),
                "{file}"
            );
        }
    }
}

/// § 10.2 rule 7, condition 2 in the plural: two members whose rows are
/// excluded are named together; two instance members with no row are
/// named together. Condition 1 decides before condition 2, and condition 2
/// before condition 3.
#[test]
fn rule_7_conditions_in_order_and_in_the_plural() {
    use inspectah_core::types::units::MemberLinks::{Installed, Unlinked};
    let head = |named: &str, clause: &str| {
        format!(
            "# Advisory: bundle.service is enabled on the source host. Not carried: systemctl \
             enable on it also enables {named} (named by Also=), {clause}."
        )
    };
    let mut h1 = change_with("h1.service", vendor("h1.service"), UnitState::Enabled);
    h1.disposition = FindingKind::excluded();
    let mut h2 = change_with("h2.service", vendor("h2.service"), UnitState::Enabled);
    h2.disposition = FindingKind::excluded();
    let snap = units_snapshot(
        vec![
            owner(
                "bundle.service",
                UnitState::Enabled,
                vec![
                    also_member("h1.service", Installed),
                    also_member("h2.service", Installed),
                    also_member("t@one.service", Installed),
                ],
            ),
            h1,
            h2,
        ],
        vec![],
    );
    assert_eq!(
        also_why(&snap, "bundle.service"),
        Some(("h1.service".into(), AlsoWhy::RowNotCarried))
    );
    assert_eq!(
        block_at(&render(&snap).lines, "# Advisory: bundle.service"),
        [head(
            "h1.service and h2.service",
            "whose own rows are not carried with this command, and t@one.service, an instance \
             with no row here to check its configuration"
        )]
    );

    let snap = units_snapshot(
        vec![owner(
            "bundle.service",
            UnitState::Enabled,
            vec![
                also_member("t@one.service", Installed),
                also_member("t@two.service", Installed),
            ],
        )],
        vec![],
    );
    assert_eq!(
        also_why(&snap, "bundle.service"),
        Some(("t@one.service".into(), AlsoWhy::NeedsOwnRow))
    );
    assert_eq!(
        block_at(&render(&snap).lines, "# Advisory: bundle.service"),
        [head(
            "t@one.service and t@two.service",
            "instances with no row here to check their configuration"
        )]
    );

    // Condition 2 failed two ways: one clause group per cause, in `also`
    // order within a group, the group of the first failing member first.
    let mut h3 = change_with("h3.service", vendor("h3.service"), UnitState::Enabled);
    h3.disposition = FindingKind::excluded();
    let snap = units_snapshot(
        vec![
            owner(
                "bundle.service",
                UnitState::Enabled,
                vec![
                    also_member("t@one.service", Installed),
                    also_member("h3.service", Installed),
                    also_member("t@two.service", Installed),
                ],
            ),
            h3,
        ],
        vec![],
    );
    assert_eq!(
        also_why(&snap, "bundle.service"),
        Some(("t@one.service".into(), AlsoWhy::NeedsOwnRow))
    );
    assert_eq!(
        block_at(&render(&snap).lines, "# Advisory: bundle.service"),
        [head(
            "t@one.service and t@two.service",
            "instances with no row here to check their configuration, and h3.service, whose \
             own row is not carried with this command"
        )]
    );

    // Condition 1 first: an `Unlinked` member after an instance with no
    // row, and an install file on the owner.
    let snap = with_drop_in_rows(
        units_snapshot(
            vec![owner(
                "bundle.service",
                UnitState::Enabled,
                vec![
                    also_member("t@one.service", Installed),
                    also_member("c.service", Unlinked),
                ],
            )],
            vec![],
        ),
        vec![drop_in_row(
            "c.service",
            "/etc/systemd/system/c.service.d/10-install.conf",
            "[Install]\nWantedBy=multi-user.target\n",
            true,
        )],
    );
    assert_eq!(
        also_why(&snap, "bundle.service"),
        Some(("c.service".into(), AlsoWhy::LinksDiffer(Unlinked)))
    );
    // Condition 2 before condition 3.
    let snap = with_drop_in_rows(
        units_snapshot(
            vec![owner(
                "bundle.service",
                UnitState::Enabled,
                vec![
                    also_member("c.service", Installed),
                    also_member("t@one.service", Installed),
                ],
            )],
            vec![],
        ),
        vec![drop_in_row(
            "c.service",
            "/etc/systemd/system/c.service.d/10-install.conf",
            "[Install]\nWantedBy=multi-user.target\n",
            true,
        )],
    );
    assert_eq!(
        also_why(&snap, "bundle.service"),
        Some(("t@one.service".into(), AlsoWhy::NeedsOwnRow))
    );
}

/// T25 (h): which staged or excluded files are install files for rule 7.
/// A drop-in with `[Install]` in the type-wide `service.d` or in a
/// dash-prefix directory is not one, and the `enable` is carried; the same
/// file in the unit's own `.d`, recorded on the row so rule 5 passes, is
/// `InstallFile`. In a member's `.d`, a drop-in whose `[Install]` holds
/// `Also = x`, an assignment continued over two lines, or an empty
/// `WantedBy=` is an install file each time, included or excluded.
#[test]
fn t25_h_install_files_for_rule_7() {
    use inspectah_core::types::units::MemberLinks::Installed;
    const INSTALL: &str = "[Install]\nWantedBy=multi-user.target\n";
    let base = |extra: Vec<String>| {
        let mut row = owner(
            "a-b.service",
            UnitState::Enabled,
            vec![also_member("helper.service", Installed)],
        );
        row.install_drop_ins = extra;
        units_snapshot(vec![row], vec![])
    };
    for path in [
        "/etc/systemd/system/service.d/10-site.conf",
        "/etc/systemd/system/a-.service.d/10-site.conf",
    ] {
        let snap = with_drop_in_rows(
            base(Vec::new()),
            vec![drop_in_row("a-b.service", path, INSTALL, true)],
        );
        assert_eq!(also_why(&snap, "a-b.service"), None, "{path}");
        assert!(
            acted_on(&render(&snap).lines, "a-b.service"),
            "{path}: {:?}",
            render(&snap).lines
        );
    }
    let own = "/etc/systemd/system/a-b.service.d/10-site.conf";
    let snap = with_drop_in_rows(
        base(vec![own.to_owned()]),
        vec![drop_in_row("a-b.service", own, INSTALL, true)],
    );
    assert_eq!(
        also_why(&snap, "a-b.service"),
        Some((
            "helper.service".into(),
            AlsoWhy::InstallFile { path: own.into() }
        ))
    );
    assert_eq!(
        block_at(&render(&snap).lines, "# Advisory: a-b.service"),
        [
            format!(
                "# Advisory: a-b.service is enabled on the source host. Not carried: {own} sets \
                 [Install] for a-b.service, so the units systemctl enable reaches through Also= \
                 in the image may not be the ones recorded."
            ),
            "#   After reading that file, add after the configuration COPY:".to_owned(),
            "#     RUN systemctl enable -- 'a-b.service'".to_owned(),
        ]
    );

    let member_file = "/etc/systemd/system/helper.service.d/10-x.conf";
    for content in [
        "[Install]\nAlso = x.service\n",
        "[Install]\nWantedBy=multi-user.target \\\n  graphical.target\n",
        "[Install]\nWantedBy=\n",
    ] {
        for included in [true, false] {
            let snap = with_drop_in_rows(
                base(Vec::new()),
                vec![drop_in_row(
                    "helper.service",
                    member_file,
                    content,
                    included,
                )],
            );
            assert_eq!(
                also_why(&snap, "a-b.service"),
                Some((
                    "helper.service".into(),
                    AlsoWhy::InstallFile {
                        path: member_file.into()
                    }
                )),
                "{content:?} included {included}"
            );
        }
    }
    // The control: a `[Service]` drop-in under a name of its own.
    let snap = with_drop_in_rows(
        base(Vec::new()),
        vec![drop_in_row(
            "helper.service",
            member_file,
            "[Service]\nNice=5\n",
            true,
        )],
    );
    assert_eq!(also_why(&snap, "a-b.service"), None);
}
