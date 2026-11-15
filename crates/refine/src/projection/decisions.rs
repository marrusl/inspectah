use std::collections::{BTreeMap, BTreeSet};

use inspectah_core::types::repo::RepoId;
use inspectah_core::types::rpm::VersionChange;
use inspectah_core::types::users::UserGroupDecision;

use crate::classify::{classify_containers, classify_services, classify_tuned};
use crate::repo_index::RepoIndex;
use crate::session::RefineSession;
use crate::types::ItemId;

use inspectah_pipeline::render::not_carried::{
    NotCarriedPackage, not_carried_notice_for, not_carried_packages,
};

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::units::{
    AdminStorageFragment, CarriedUnit, UnitAction, UnitFragment, UnitName,
};
use inspectah_pipeline::render::configtree::{
    Destination, PathError, StagedFileKind, StagedWrites, WithheldWhy, staged_writes,
};
use inspectah_pipeline::render::unit_plan::{
    DeclinedMarker, GeneratedSource, OmissionReason, UnitPlan, declined_line, etc_unit_path,
    plan_units,
};

use super::types::{
    ConfigWithheldView, DecisionProjection, DeclinedGeneratedView, DeclinedRenders, DeclinedView,
    DropInView, FragmentView, GeneratedRowView, GeneratedTimerView, MountFindingView,
    NotCarriedRow, RepoGroup, UnitStateView, UnmanagedWithheldView, WithheldView,
};
use crate::types::{RefinedDropIn, RefinedServiceState};

/// Build a [`DecisionProjection`] from the current session state.
///
/// Calls classify_* functions on the projected snapshot and reads
/// repo groups, version changes, users/groups, sensitivity, and
/// baseline summary from the session's public API.
pub fn project_decisions(session: &RefineSession) -> DecisionProjection {
    let snap = session.snapshot_projected();

    // Classify units → tuple of (states, dropins), then attach what the
    // export does with each: the plan and the stager's decision, computed
    // once each over the projected snapshot, as the export computes them.
    let (states, dropins) = classify_services(&snap);
    let staged = staged_writes(&snap);
    let plan = plan_units(&snap, &staged);
    let withheld = WithheldIndex::new(&staged);
    let unit_states = unit_state_views(states, &plan);
    let unit_dropins = drop_in_views(dropins, &withheld);
    let mount_findings = mount_finding_views(&snap, &plan);
    let config_withheld = config_withheld_views(&snap, &withheld);
    let declined_generated = declined_generated_views(&plan);
    let generated_timers = generated_timer_views(&snap);
    let unmanaged_withheld = unmanaged_withheld_views(&snap, &withheld);

    // Classify containers → tuple of (quadlets, flatpaks)
    let (quadlets, flatpaks) = classify_containers(&snap);

    // Classify kernel/boot
    let tuned = classify_tuned(&snap);

    // Version changes from RPM section
    let version_changes: Vec<VersionChange> = snap
        .rpm
        .as_ref()
        .map(|r| r.version_changes.clone())
        .unwrap_or_default();

    // The export's `# Not carried` list and notice, from the same
    // projected snapshot it renders from and from one list, so the rows
    // that own an enabled unit are the packages the notice names.
    let not_carried_list = not_carried_packages(&snap);
    let not_carried_notice = not_carried_notice_for(&not_carried_list);
    let not_carried = not_carried_list.iter().map(not_carried_row).collect();

    // Users/groups from projected snapshot
    let users_groups: Vec<UserGroupDecision> = snap
        .users_groups
        .map(|ug| ug.users)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    // Repo groups — ported from handlers.rs build_repo_groups
    let repo_groups = build_repo_groups(session);

    // Sensitivity and baseline summary
    let is_sensitive = session.is_sensitive();
    let baseline_summary = session.baseline_summary();

    DecisionProjection {
        unit_states,
        unit_dropins,
        mount_findings,
        config_withheld,
        quadlets,
        flatpaks,
        tuned,
        repo_groups,
        version_changes,
        users_groups,
        is_sensitive,
        baseline_summary,
        not_carried,
        not_carried_notice,
        declined_generated,
        generated_timers,
        unmanaged_withheld,
    }
}

/// One view per included unmanaged-file row whose file the export
/// withholds, by the row's own path.
fn unmanaged_withheld_views(
    snap: &InspectionSnapshot,
    withheld: &WithheldIndex<'_>,
) -> Vec<UnmanagedWithheldView> {
    snap.unmanaged_files
        .iter()
        .flat_map(|section| &section.items)
        .filter(|file| file.disposition.is_included())
        .filter_map(|file| {
            withheld
                .view(StagedFileKind::UnmanagedFile, &file.path)
                .map(|withheld| UnmanagedWithheldView {
                    path: file.path.clone(),
                    withheld,
                })
        })
        .collect()
}

/// One view per generated-timer name, in the scheduled section's order,
/// with the include flag of the projected snapshot.
fn generated_timer_views(snap: &InspectionSnapshot) -> Vec<GeneratedTimerView> {
    let mut views: Vec<GeneratedTimerView> = Vec::new();
    for unit in snap
        .scheduled_tasks
        .iter()
        .flat_map(|st| &st.generated_timer_units)
    {
        if views.iter().any(|v| v.name == unit.name) {
            continue;
        }
        views.push(GeneratedTimerView {
            name: unit.name.clone(),
            include: unit.disposition.is_included(),
            locked: unit.locked,
        });
    }
    views
}

/// One view per generated unit's request the plan declined, with the row
/// it came from (§ 11.1).
fn declined_generated_views(plan: &UnitPlan) -> Vec<DeclinedGeneratedView> {
    plan.generated_declined
        .iter()
        .map(|decline| DeclinedGeneratedView {
            row: match &decline.source {
                GeneratedSource::Cron { name } => {
                    GeneratedRowView::Scheduled { name: name.clone() }
                }
                GeneratedSource::FlatpakProvisioning => GeneratedRowView::Flatpak,
            },
            unit: decline.unit.clone(),
            declined: declined_view(&decline.unit, UnitAction::Enable, &decline.reason),
        })
        .collect()
}

/// The row's view of a declined command: [`declined_line`] decides the
/// marker and the text, as it does for the Containerfile.
fn declined_view(name: &UnitName, action: UnitAction, reason: &OmissionReason) -> DeclinedView {
    let line = declined_line(name, action, reason);
    DeclinedView {
        renders: match line.marker {
            DeclinedMarker::Omitted => DeclinedRenders::Omitted,
            DeclinedMarker::Fixme => DeclinedRenders::Fixme,
        },
        reason: reason.name().to_owned(),
        text: line.text,
    }
}

/// One view per state change row, each with the plan's decline, if any.
/// A row the plan does not decline (planned, or excluded, as a
/// runtime updater is) has none.
fn unit_state_views(states: Vec<RefinedServiceState>, plan: &UnitPlan) -> Vec<UnitStateView> {
    let declined: BTreeMap<&UnitName, (UnitAction, &OmissionReason)> = plan
        .declined
        .iter()
        .map(|(name, action, reason)| (name, (*action, reason)))
        .collect();
    states
        .into_iter()
        .map(|RefinedServiceState { entry, triage }| {
            let name = entry.unit().name();
            let (fragment, unit_file_path) = match entry.unit() {
                CarriedUnit::Ordinary(unit) => match unit.fragment() {
                    UnitFragment::Vendor { .. } => (FragmentView::Vendor, None),
                    UnitFragment::Local { file } => {
                        (FragmentView::Local, Some(etc_unit_path(file)))
                    }
                    UnitFragment::Shadowed { file, .. } => {
                        (FragmentView::Shadowed, Some(etc_unit_path(file)))
                    }
                    UnitFragment::Absent => (FragmentView::Absent, None),
                },
                CarriedUnit::PackagedStorage(_) => (FragmentView::Vendor, None),
            };
            UnitStateView {
                unit: name.clone(),
                kind: name.kind().suffix().to_owned(),
                instance_of: name.instance().map(|instance| instance.template()),
                current_state: entry.current_state(),
                default_state: entry.default_state,
                fragment,
                unit_file_path,
                owning_package: entry.unit().owning_package().map(str::to_owned),
                include: entry.disposition.is_included(),
                locked: entry.locked,
                triage,
                attention_reason: entry.attention_reason.clone(),
                declined: declined
                    .get(name)
                    .map(|(action, reason)| declined_view(name, *action, reason)),
            }
        })
        .collect()
}

/// The stager's withheld and refused offers, keyed as a row finds its own:
/// a withheld file by its destination and the kind of row that offered
/// it, so a path spelled two ways finds its row; a refused offer by the
/// path as offered, which is the row's own.
struct WithheldIndex<'a> {
    withheld: BTreeMap<(&'a Destination, StagedFileKind), &'a WithheldWhy>,
    refused: BTreeMap<(StagedFileKind, &'a str), PathError>,
}

impl<'a> WithheldIndex<'a> {
    fn new(staged: &'a StagedWrites<'_>) -> Self {
        let mut withheld = BTreeMap::new();
        for (dest, kind, why) in &staged.withheld {
            withheld.entry((dest, *kind)).or_insert(why);
        }
        let refused = staged
            .refused
            .iter()
            .map(|(kind, path, error)| ((*kind, path.as_str()), *error))
            .collect();
        Self { withheld, refused }
    }

    /// Why the export does not write the file a row of `kind` offers at
    /// `path`, or `None` when it writes it.
    fn view(&self, kind: StagedFileKind, path: &str) -> Option<WithheldView> {
        match Destination::new(path) {
            Ok(dest) => self
                .withheld
                .get(&(&dest, kind))
                .map(|why| WithheldView::Withheld {
                    why: why.name().to_owned(),
                    text: why.describe(&dest),
                }),
            Err(_) => self
                .refused
                .get(&(kind, path))
                .map(|error| WithheldView::Refused {
                    rule: error.name().to_owned(),
                    text: error.to_string(),
                }),
        }
    }
}

/// One view per drop-in row, with its parent's kind and, for an included
/// row, why the export does not write its file.
fn drop_in_views(dropins: Vec<RefinedDropIn>, withheld: &WithheldIndex<'_>) -> Vec<DropInView> {
    dropins
        .into_iter()
        .map(|RefinedDropIn { entry, triage }| {
            let kind = UnitName::try_from(entry.unit.clone())
                .ok()
                .map(|name| name.kind().suffix().to_owned());
            let withheld = entry
                .disposition
                .is_included()
                .then(|| withheld.view(StagedFileKind::DropIn, &entry.path))
                .flatten();
            DropInView {
                entry,
                triage,
                kind,
                withheld,
            }
        })
        .collect()
}

/// Every included config row whose file the export does not write.
fn config_withheld_views(
    snap: &InspectionSnapshot,
    withheld: &WithheldIndex<'_>,
) -> Vec<ConfigWithheldView> {
    snap.config
        .iter()
        .flat_map(|config| &config.files)
        .filter(|file| file.disposition.is_included())
        .filter_map(|file| {
            withheld
                .view(StagedFileKind::ConfigFile, &file.path)
                .map(|withheld| ConfigWithheldView {
                    path: file.path.clone(),
                    withheld,
                })
        })
        .collect()
}

/// One view per mount finding, with § 10.6's lines from the plan, which
/// holds one block per finding in the section's order.
fn mount_finding_views(snap: &InspectionSnapshot, plan: &UnitPlan) -> Vec<MountFindingView> {
    let Some(units) = &snap.units else {
        return Vec::new();
    };
    let config_rows: BTreeMap<Destination, &str> = snap
        .config
        .iter()
        .flat_map(|config| &config.files)
        .filter_map(|file| Some((Destination::new(&file.path).ok()?, file.path.as_str())))
        .collect();
    debug_assert_eq!(
        units.mount_findings.len(),
        plan.mount_advisories.len(),
        "plan_units builds one advisory block per mount finding, in order, so the two zip \
         positionally"
    );
    units
        .mount_findings
        .iter()
        .zip(&plan.mount_advisories)
        .map(|(finding, advisories)| {
            let name = finding.unit.name().as_unit_name();
            let fragment = match finding.unit.fragment() {
                AdminStorageFragment::Local => FragmentView::Local,
                AdminStorageFragment::Shadowed { .. } => FragmentView::Shadowed,
                AdminStorageFragment::UnownedVendor => FragmentView::Vendor,
                AdminStorageFragment::MaskOnly => FragmentView::Absent,
            };
            let unit_file_path = match fragment {
                FragmentView::Local | FragmentView::Shadowed => {
                    Destination::new(&etc_unit_path(name))
                        .ok()
                        .and_then(|dest| config_rows.get(&dest))
                        .map(|path| (*path).to_owned())
                }
                FragmentView::Vendor | FragmentView::Absent => None,
            };
            MountFindingView {
                unit: name.clone(),
                kind: name.kind().suffix().to_owned(),
                state: finding.state_change.map(|(state, _)| state),
                mount_point: finding.mount_point.clone(),
                fragment,
                unit_file_path,
                advisories: advisories.clone(),
            }
        })
        .collect()
}

/// One not-carried package as refine shows it.
fn not_carried_row(pkg: &NotCarriedPackage) -> NotCarriedRow {
    NotCarriedRow {
        item_id: ItemId::Package {
            name: pkg.name.clone(),
            arch: pkg.arch.clone(),
        },
        display_name: pkg.display_name(),
        source_repo: pkg.source_repo.clone(),
        enabled_units: pkg.enabled_units.clone(),
        owns_enabled_unit: pkg.owns_enabled_unit(),
    }
}

/// Build repo groups from the session's repo index and current view.
///
/// Ported from `inspectah-web/src/handlers.rs` `build_repo_groups`.
/// Produces `Vec<RepoGroup>` (projection type) instead of `Vec<RepoGroupInfo>`.
fn build_repo_groups(session: &RefineSession) -> Vec<RepoGroup> {
    let view = session.view();
    let repo_index = session.repo_index();
    let changes = session.pending_changes();
    let repos_excluded = changes.repos_excluded();
    let excluded: BTreeSet<&str> = repos_excluded.iter().map(|s| s.as_str()).collect();

    // Count visible packages per source_repo, keeping the package's recorded
    // spelling as-is -- RepoIndex and RepoId fold ASCII case internally, so
    // the lookups below match regardless of case.
    let mut repo_counts: BTreeMap<String, usize> = BTreeMap::new();
    for pkg in &view.packages {
        let section = pkg.entry.source_repo.clone();
        *repo_counts.entry(section).or_insert(0) += 1;
    }

    // Also include repos known to the index but not visible (0-count)
    for section_id in repo_index.packages_by_repo().keys() {
        repo_counts.entry(section_id.to_string()).or_insert(0);
    }

    let mut groups: Vec<RepoGroup> = repo_counts
        .into_iter()
        .map(|(section_id, package_count)| {
            let provenance = if section_id.is_empty() {
                crate::types::RepoProvenance::Unknown
            } else {
                repo_index.provenance(&RepoId::new(section_id.as_str()))
            };
            let is_distro = if section_id.is_empty() {
                false
            } else {
                RepoIndex::is_distro_repo(&section_id)
            };
            let tier = RepoIndex::repo_tier(&section_id);
            let enabled = !excluded.contains(section_id.as_str());
            RepoGroup {
                section_id,
                provenance,
                is_distro,
                tier,
                package_count,
                enabled,
            }
        })
        .collect();

    // Sort: distro repos first, then by section_id
    groups.sort_by(|a, b| {
        b.is_distro
            .cmp(&a.is_distro)
            .then_with(|| a.section_id.cmp(&b.section_id))
    });

    groups
}

#[cfg(test)]
mod tests {
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

    use super::*;
    use inspectah_core::snapshot::InspectionSnapshot;
    use inspectah_core::types::FindingKind;
    use inspectah_core::types::rpm::{RpmSection, VersionChangeDirection};
    use inspectah_core::types::units::{
        PresetDefault, UnitInventoryEntry, UnitSection, UnitState, UnitStateChange,
    };

    /// Snapshot with one service state change.
    fn snapshot_with_service() -> InspectionSnapshot {
        InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection::default()),
            units: Some(UnitSection {
                state_changes: vec![
                    UnitStateChange::new(
                        test_carried_unit(
                            "httpd.service".to_owned().try_into().unwrap(),
                            Some("httpd".into()),
                        ),
                        UnitState::Enabled,
                        Some(PresetDefault::Disable),
                    )
                    .unwrap(),
                ],
                inventory: vec![UnitInventoryEntry {
                    unit: "httpd.service"
                        .to_owned()
                        .try_into()
                        .expect("valid unit name"),
                    state: UnitState::Enabled,
                    preset: None,
                }],
                drop_ins: vec![],
                mount_findings: Vec::new(),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn classified_services_appear_in_projection() {
        let session = RefineSession::new(snapshot_with_service());
        let proj = project_decisions(&session);

        assert_eq!(proj.unit_states.len(), 1);
        assert_eq!(proj.unit_states[0].unit.as_str(), "httpd.service");
    }

    #[test]
    fn empty_snapshot_produces_empty_projection() {
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection::default()),
            ..Default::default()
        };
        let session = RefineSession::new(snap);
        let proj = project_decisions(&session);

        assert!(proj.unit_states.is_empty());
        assert!(proj.unit_dropins.is_empty());
        assert!(proj.quadlets.is_empty());
        assert!(proj.flatpaks.is_empty());
        assert!(proj.tuned.is_empty());
        assert!(proj.version_changes.is_empty());
        assert!(proj.users_groups.is_empty());
    }

    #[test]
    fn version_changes_pass_through_from_rpm() {
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                version_changes: vec![VersionChange {
                    name: "bash".into(),
                    arch: "x86_64".into(),
                    host_version: "5.2.15-3.el9".into(),
                    base_version: "5.1.8-6.el9".into(),
                    host_epoch: "0".into(),
                    base_epoch: "0".into(),
                    direction: VersionChangeDirection::Upgrade,
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let session = RefineSession::new(snap);
        let proj = project_decisions(&session);

        assert_eq!(proj.version_changes.len(), 1);
        assert_eq!(proj.version_changes[0].name, "bash");
        assert_eq!(
            proj.version_changes[0].direction,
            VersionChangeDirection::Upgrade
        );
    }

    #[test]
    fn repo_group_count_excludes_platform_plumbing() {
        use inspectah_core::types::config::ConfigSection;
        use inspectah_core::types::rpm::{FileOwnershipEntry, PackageEntry};

        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![
                    // Platform plumbing — filtered from view
                    PackageEntry {
                        name: "grub2-tools".into(),
                        arch: "x86_64".into(),
                        disposition: FindingKind::included(),
                        source_repo: "anaconda".into(),
                        ..Default::default()
                    },
                    // Normal anaconda package — visible
                    PackageEntry {
                        name: "cronie".into(),
                        arch: "x86_64".into(),
                        disposition: FindingKind::included(),
                        source_repo: "anaconda".into(),
                        ..Default::default()
                    },
                ],
                file_ownership: vec![FileOwnershipEntry {
                    package_name: "dummy".into(),
                    paths: vec!["/etc/dummy".into()],
                }],
                ..Default::default()
            }),
            units: Some(UnitSection {
                state_changes: vec![
                    UnitStateChange::new(
                        test_carried_unit(
                            "sshd.service".to_owned().try_into().unwrap(),
                            Some("openssh-server".into()),
                        ),
                        UnitState::Enabled,
                        Some(PresetDefault::Enable),
                    )
                    .unwrap(),
                ],
                ..Default::default()
            }),
            config: Some(ConfigSection { files: vec![] }),
            ..Default::default()
        };
        snap.schema_version = inspectah_core::snapshot::SCHEMA_VERSION;

        let session = RefineSession::new(snap);
        let proj = project_decisions(&session);

        // Find the anaconda repo group
        let anaconda_group = proj
            .repo_groups
            .iter()
            .find(|rg| rg.section_id == "anaconda");
        assert!(anaconda_group.is_some(), "anaconda repo group should exist");
        assert_eq!(
            anaconda_group.unwrap().package_count,
            1,
            "anaconda group count should be 1 (grub2-tools filtered)"
        );
    }

    #[test]
    fn repo_group_mixed_case_source_repo_collapses_to_one_group() {
        use inspectah_core::types::rpm::{PackageEntry, RepoFile};

        // dnf repoquery can record source_repo with the .repo section header's
        // own (often uppercase) spelling. Regression guard: build_repo_groups
        // used to lowercase the package's source_repo while RepoIndex's
        // backfill loop used the host's original spelling, producing two
        // entries (one real, one zero-count) for a single repo.
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "some-pkg".into(),
                    arch: "x86_64".into(),
                    disposition: FindingKind::included(),
                    source_repo: "EPEL_10_Everything_x86_64".into(),
                    ..Default::default()
                }],
                repo_files: vec![RepoFile {
                    path: "/etc/yum.repos.d/epel.repo".into(),
                    content: "[EPEL_10_Everything_x86_64]\nname=EPEL\nbaseurl=http://example.invalid/epel10\n".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let session = RefineSession::new(snap);
        let proj = project_decisions(&session);

        let matches: Vec<_> = proj
            .repo_groups
            .iter()
            .filter(|rg| {
                rg.section_id
                    .eq_ignore_ascii_case("EPEL_10_Everything_x86_64")
            })
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "mixed-case source_repo must collapse to a single repo group, got {:?}",
            matches
        );
        assert_eq!(matches[0].package_count, 1);
    }
}
