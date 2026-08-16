//! Bridges `RefineSession` to TUI section types.
//!
//! Provides the canonical sidebar section order and a function to
//! build `SectionEntry` counts from a live session.

use inspectah_refine::session::RefineSession;
use inspectah_refine::types::SectionKind;

use crate::types::{NavGroup, SectionEntry, SectionId};

/// Ordered list of sidebar sections, grouped by `NavGroup`.
///
/// Sections are ordered by group (matching `NAV_GROUPS`), with decision
/// sections before reference-only sections within each group.
pub const SECTION_ORDER: &[SectionId] = &[
    // Packages group
    SectionId::Packages,
    SectionId::VerChanges,
    // System Configuration group
    SectionId::Configs,
    SectionId::Sysctls,
    SectionId::Tuned,
    SectionId::KernelBoot,
    SectionId::SELinux,
    // Services & Scheduling group
    SectionId::Services,
    SectionId::Containers,
    SectionId::ScheduledTasks,
    // Identity group
    SectionId::Users,
    // Network group
    SectionId::Network,
    // Storage group
    SectionId::Storage,
    // Software group
    SectionId::NonRpmSoftware,
];

/// Nav groups with their member sections in display order.
pub const NAV_GROUPS: &[(NavGroup, &[SectionId])] = &[
    (
        NavGroup::Packages,
        &[SectionId::Packages, SectionId::VerChanges],
    ),
    (
        NavGroup::SystemConfig,
        &[
            SectionId::Configs,
            SectionId::Sysctls,
            SectionId::Tuned,
            SectionId::KernelBoot,
            SectionId::SELinux,
        ],
    ),
    (
        NavGroup::Services,
        &[
            SectionId::Services,
            SectionId::Containers,
            SectionId::ScheduledTasks,
        ],
    ),
    (NavGroup::Identity, &[SectionId::Users]),
    (NavGroup::Network, &[SectionId::Network]),
    (NavGroup::Storage, &[SectionId::Storage]),
    (NavGroup::Software, &[SectionId::NonRpmSoftware]),
];

/// A row in the sidebar nav tree: either a group header or a section.
#[derive(Debug, Clone)]
pub enum NavRow {
    /// Collapsible group header.
    Group {
        group: NavGroup,
        collapsed: bool,
        /// Sum of actionable items across sections in this group.
        actionable: usize,
        /// Sum of advisory/reference items across sections in this group.
        advisories: usize,
    },
    /// An individual section within a group.
    Section(SectionEntry),
}

/// Build the flat list of nav rows from section entries and collapse state.
///
/// Each group gets a header row, followed by its section rows (if expanded).
pub fn build_nav_rows(
    entries: &[SectionEntry],
    collapsed: &std::collections::HashSet<NavGroup>,
) -> Vec<NavRow> {
    let mut rows = Vec::new();

    for &(group, member_ids) in NAV_GROUPS {
        let is_collapsed = collapsed.contains(&group);

        // Collect section entries for this group.
        let group_entries: Vec<&SectionEntry> = member_ids
            .iter()
            .filter_map(|id| entries.iter().find(|e| e.id == *id))
            .collect();

        // Compute group summary counts. Advisories are the remainder, so
        // this is only correct while `included` and `excluded` count
        // actionable decisions alone -- reference-only sections report
        // both as zero, and `SectionStats` keeps advisories out of its
        // excluded bucket for the same reason.
        let actionable: usize = group_entries.iter().map(|e| e.included + e.excluded).sum();
        let total: usize = group_entries.iter().map(|e| e.count).sum();
        let advisories = total.saturating_sub(actionable);

        rows.push(NavRow::Group {
            group,
            collapsed: is_collapsed,
            actionable,
            advisories,
        });

        if !is_collapsed {
            for id in member_ids {
                if let Some(entry) = entries.iter().find(|e| e.id == *id) {
                    rows.push(NavRow::Section(entry.clone()));
                }
            }
        }
    }

    rows
}

/// Find the list of visible section indices (into SECTION_ORDER) from
/// the current nav state.
pub fn visible_sections(collapsed: &std::collections::HashSet<NavGroup>) -> Vec<usize> {
    let mut visible = Vec::new();
    for (idx, section_id) in SECTION_ORDER.iter().enumerate() {
        if !collapsed.contains(&section_id.group()) {
            visible.push(idx);
        }
    }
    visible
}

/// Build sidebar entries with item counts from a live session.
///
/// For sections tracked by `RefineStats` (Package, Config, Repo), uses
/// `view().stats.section()`. For decision sections, counts items from
/// `decisions()`. For reference-only sections, counts from `reference()`.
///
/// Repos are not a standalone sidebar entry -- they are embedded in the
/// Packages section. Their counts are not surfaced here.
pub fn build_section_entries(session: &RefineSession) -> Vec<SectionEntry> {
    let view = session.view();
    let stats = &view.stats;
    let decisions = session.decisions();
    let reference = session.reference();

    SECTION_ORDER
        .iter()
        .map(|&id| {
            let (count, included, excluded) = match id {
                // ── Stats-backed sections ────────────────────────
                SectionId::Packages => {
                    let s = stats.section(SectionKind::Package);
                    (s.total, s.included, s.excluded)
                }
                SectionId::Configs => {
                    let s = stats.section(SectionKind::Config);
                    (s.total, s.included, s.excluded)
                }

                // ── Decision sections ────────────────────────────
                SectionId::Services => {
                    // Composite: decision states + drop-ins + reference sub-collections.
                    // Decision items have include fields, reference items are read-only.
                    let dec_count =
                        decisions.service_states.len() + decisions.service_dropins.len();
                    let dec_included = decisions
                        .service_states
                        .iter()
                        .filter(|s| s.entry.disposition.is_included())
                        .count()
                        + decisions
                            .service_dropins
                            .iter()
                            .filter(|d| d.entry.disposition.is_included())
                            .count();
                    let ref_count = reference.services.divergent.len()
                        + reference.services.preset_matched_with_dropins.len()
                        + reference.services.preset_unknown_enabled.len()
                        + reference.services.preset_unknown_disabled.len()
                        + reference.services.standalone_dropins.len()
                        + reference.services.omitted.len()
                        + reference.services.advisories.len()
                        + reference.services.warnings.len();
                    let total = dec_count + ref_count;
                    let excluded = dec_count - dec_included;
                    (total, dec_included, excluded)
                }
                SectionId::Containers => {
                    // Composite: decision quadlets/flatpaks + reference running/compose.
                    let dec_count = decisions.quadlets.len() + decisions.flatpaks.len();
                    let dec_included = decisions
                        .quadlets
                        .iter()
                        .filter(|q| q.entry.disposition.is_included())
                        .count()
                        + decisions
                            .flatpaks
                            .iter()
                            .filter(|f| f.entry.disposition.is_included())
                            .count();
                    let ref_count = reference.containers.running_containers.len()
                        + reference.containers.compose_files.len()
                        + reference.containers.quadlets.len()
                        + reference.containers.flatpaks.len();
                    let total = dec_count + ref_count;
                    let excluded = dec_count - dec_included;
                    (total, dec_included, excluded)
                }
                SectionId::Sysctls => {
                    let total = decisions.sysctls.len();
                    let included = decisions
                        .sysctls
                        .iter()
                        .filter(|s| s.entry.disposition.is_included())
                        .count();
                    (total, included, total - included)
                }
                SectionId::Tuned => {
                    let total = decisions.tuned.len();
                    let included = decisions.tuned.iter().filter(|t| t.include).count();
                    (total, included, total - included)
                }
                SectionId::Users => {
                    let total = decisions.users_groups.len();
                    let included = decisions
                        .users_groups
                        .iter()
                        .filter(|u| u.disposition.is_included())
                        .count();
                    (total, included, total - included)
                }

                // ── Reference-only sections ──────────────────────
                // These are read-only; included/excluded are always 0.
                SectionId::VerChanges => {
                    let total = reference.version_changes.downgrades.len()
                        + reference.version_changes.upgrades.len();
                    (total, 0, 0)
                }
                SectionId::KernelBoot => {
                    let kb = &reference.kernel_boot;
                    let total = kb.sysctl_overrides.len()
                        + kb.non_default_modules.len()
                        + kb.modules_load_d.len()
                        + kb.modprobe_d.len()
                        + kb.dracut_conf.len()
                        + kb.custom_tuned_profiles.len()
                        + kb.alternatives.len();
                    (total, 0, 0)
                }
                SectionId::Network => {
                    let net = &reference.network;
                    let total = net.connections.len()
                        + net.firewall_zones.len()
                        + net.firewall_direct_rules.len()
                        + net.static_routes.len()
                        + net.ip_routes.len()
                        + net.ip_rules.len()
                        + net.hosts_additions.len()
                        + net.proxy_env.len();
                    (total, 0, 0)
                }
                SectionId::Storage => {
                    let stor = &reference.storage;
                    let total = stor.fstab_entries.len()
                        + stor.mount_points.len()
                        + stor.lvm_volumes.len()
                        + stor.var_directories.len()
                        + stor.credential_refs.len();
                    (total, 0, 0)
                }
                SectionId::ScheduledTasks => (reference.scheduled_tasks.len(), 0, 0),
                SectionId::NonRpmSoftware => (reference.non_rpm_software.len(), 0, 0),
                SectionId::SELinux => (reference.selinux.len(), 0, 0),
            };

            SectionEntry {
                id,
                count,
                included,
                excluded,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn section_order_has_fourteen_entries() {
        assert_eq!(SECTION_ORDER.len(), 14);
    }

    #[test]
    fn nav_groups_cover_all_sections() {
        // Every section in SECTION_ORDER must appear in exactly one NAV_GROUP.
        let flat: Vec<SectionId> = NAV_GROUPS
            .iter()
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect();
        assert_eq!(
            flat.len(),
            SECTION_ORDER.len(),
            "NAV_GROUPS must cover all sections"
        );
        for (i, id) in flat.iter().enumerate() {
            assert_eq!(
                *id, SECTION_ORDER[i],
                "NAV_GROUPS order must match SECTION_ORDER at index {i}"
            );
        }
    }

    #[test]
    fn nav_rows_all_expanded() {
        let entries: Vec<SectionEntry> = SECTION_ORDER
            .iter()
            .map(|&id| SectionEntry {
                id,
                count: 1,
                included: 0,
                excluded: 0,
            })
            .collect();
        let collapsed = HashSet::new();
        let rows = build_nav_rows(&entries, &collapsed);
        // 7 group headers + 14 sections = 21 rows.
        assert_eq!(rows.len(), 21);
    }

    #[test]
    fn nav_rows_collapsed_group_hides_sections() {
        let entries: Vec<SectionEntry> = SECTION_ORDER
            .iter()
            .map(|&id| SectionEntry {
                id,
                count: 10,
                included: 3,
                excluded: 2,
            })
            .collect();
        let mut collapsed = HashSet::new();
        collapsed.insert(NavGroup::SystemConfig);
        let rows = build_nav_rows(&entries, &collapsed);
        // SystemConfig has 5 sections, now hidden.
        // 7 headers + 14 sections - 5 hidden = 16 rows.
        assert_eq!(rows.len(), 16);
        // Verify the SystemConfig header is collapsed.
        let sc_header = rows.iter().find(|r| {
            matches!(
                r,
                NavRow::Group {
                    group: NavGroup::SystemConfig,
                    ..
                }
            )
        });
        assert!(sc_header.is_some());
        if let Some(NavRow::Group { collapsed, .. }) = sc_header {
            assert!(*collapsed);
        }
    }

    /// The group badge reports advisories as whatever is left after the
    /// actionable decisions, so it is only ever right when `included` and
    /// `excluded` are actionable-only. While config advisories were
    /// counted as excluded, `included + excluded` equalled the total and a
    /// group holding two advisories rendered `[3, 0 adv]` -- the release's
    /// headline feature reporting itself absent.
    ///
    /// The same `SectionEntry` feeds the single-host status bar's
    /// included/excluded pair, so the counts asserted here are what that
    /// bar reads too.
    #[test]
    fn config_advisories_count_toward_the_group_advisory_badge() {
        use inspectah_core::snapshot::InspectionSnapshot;
        use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
        use inspectah_core::types::{AdvisoryType, FindingKind};
        use inspectah_refine::session::RefineSession;

        let advisory = |path: &str, advisory_type: AdvisoryType| ConfigFileEntry {
            path: path.into(),
            disposition: FindingKind::advisory(advisory_type, "rationale"),
            ..Default::default()
        };

        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            files: vec![
                advisory("/etc/init.d/legacy-app", AdvisoryType::Modernization),
                advisory("/etc/alternatives/java", AdvisoryType::CrossTreeSymlink),
                ConfigFileEntry {
                    path: "/etc/myapp.conf".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
            ],
        });

        let entries = build_section_entries(&RefineSession::new(snap));
        let configs = entries
            .iter()
            .find(|e| e.id == SectionId::Configs)
            .expect("Configs section entry");
        assert_eq!(configs.count, 3, "advisories stay visible in the total");
        assert_eq!(configs.included, 1);
        assert_eq!(
            configs.excluded, 0,
            "an advisory is not a file the user excluded"
        );

        let rows = build_nav_rows(&entries, &HashSet::new());
        let badge = rows
            .iter()
            .find_map(|r| match r {
                NavRow::Group {
                    group: NavGroup::SystemConfig,
                    actionable,
                    advisories,
                    ..
                } => Some((*actionable, *advisories)),
                _ => None,
            })
            .expect("System Config group header");
        assert_eq!(
            badge,
            (1, 2),
            "the group badge must read [1, 2 adv], not [3, 0 adv]"
        );
    }

    #[test]
    fn visible_sections_excludes_collapsed() {
        let mut collapsed = HashSet::new();
        collapsed.insert(NavGroup::Services);
        let vis = visible_sections(&collapsed);
        // 14 total - 3 services sections = 11.
        assert_eq!(vis.len(), 11);
        // None of the visible sections should be in the Services group.
        for idx in &vis {
            assert_ne!(
                SECTION_ORDER[*idx].group(),
                NavGroup::Services,
                "collapsed group sections should not be visible"
            );
        }
    }
}
