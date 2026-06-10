//! Bridges `RefineSession` to TUI section types.
//!
//! Provides the canonical sidebar section order and a function to
//! build `SectionEntry` counts from a live session.

use inspectah_refine::session::RefineSession;
use inspectah_refine::types::SectionKind;

use crate::types::{SectionEntry, SectionId};

/// Ordered list of sidebar sections. Decision/composite sections first
/// (above the separator), then reference-only sections below.
pub const SECTION_ORDER: &[SectionId] = &[
    // Decision / composite sections
    SectionId::Packages,
    SectionId::Configs,
    SectionId::Services,
    SectionId::Containers,
    SectionId::Sysctls,
    SectionId::Tuned,
    SectionId::Users,
    // Reference-only sections
    SectionId::VerChanges,
    SectionId::KernelBoot,
    SectionId::Network,
    SectionId::Storage,
    SectionId::ScheduledTasks,
    SectionId::NonRpmSoftware,
    SectionId::SELinux,
];

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
                        .filter(|s| s.entry.include)
                        .count()
                        + decisions
                            .service_dropins
                            .iter()
                            .filter(|d| d.entry.include)
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
                        .filter(|q| q.entry.include)
                        .count()
                        + decisions
                            .flatpaks
                            .iter()
                            .filter(|f| f.entry.include)
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
                    let included = decisions.sysctls.iter().filter(|s| s.entry.include).count();
                    (total, included, total - included)
                }
                SectionId::Tuned => {
                    let total = decisions.tuned.len();
                    let included = decisions.tuned.iter().filter(|t| t.include).count();
                    (total, included, total - included)
                }
                SectionId::Users => {
                    let total = decisions.users_groups.len();
                    let included = decisions.users_groups.iter().filter(|u| u.include).count();
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

    #[test]
    fn section_order_has_fourteen_entries() {
        assert_eq!(SECTION_ORDER.len(), 14);
    }

    #[test]
    fn section_order_starts_with_decisions_ends_with_reference() {
        // First 7 are decision/composite sections.
        for id in &SECTION_ORDER[..7] {
            assert!(id.is_decision(), "{:?} should be a decision section", id);
        }
        // Last 7 are reference-only.
        for id in &SECTION_ORDER[7..] {
            assert!(!id.is_decision(), "{:?} should be a reference section", id);
        }
    }
}
