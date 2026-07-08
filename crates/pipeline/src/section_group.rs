//! Section groups for the HTML report -- a rendering concern, not a core concept.
//!
//! Each group collects related sections under a collapsible disclosure heading.
//! This enum lives in the pipeline crate because grouping is a presentation
//! decision; other renderers (TUI, web) may define their own groupings.

/// A single section within a group, with its canonical ID, display label,
/// and whether it carries triage decisions (include/exclude toggles).
#[derive(Debug, Clone, Copy)]
pub struct SectionMeta {
    pub id: &'static str,
    pub label: &'static str,
    pub is_triage: bool,
}

/// Section IDs that existed in prior sidebar versions but are no longer
/// live. They are not routed to any group in the web UI.
const RETIRED_SECTION_IDS: &[&str] = &["system_tuning", "version_changes"];

/// Logical group that collects related report sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionGroup {
    Packages,
    SystemConfig,
    Services,
    Identity,
    Network,
    Storage,
    Software,
    Secrets,
}

impl SectionGroup {
    /// Human-readable label for this group.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Packages => "Packages",
            Self::SystemConfig => "System Configuration",
            Self::Services => "Services & Scheduling",
            Self::Identity => "Users & Identity",
            Self::Network => "Network",
            Self::Storage => "Storage",
            Self::Software => "Software & Files",
            Self::Secrets => "Secrets & Subscription",
        }
    }

    /// Check if a section ID is retired (no longer live in the web UI).
    pub fn is_retired(section_id: &str) -> bool {
        RETIRED_SECTION_IDS.contains(&section_id)
    }

    /// Map a snapshot section name to its group.
    pub fn for_section(section_name: &str) -> Self {
        match section_name {
            "rpm" | "packages" => Self::Packages,
            "config" | "configs" | "kernel_boot" | "selinux" => Self::SystemConfig,
            "services" | "scheduled_tasks" | "containers" | "compose" => Self::Services,
            "users_groups" => Self::Identity,
            "network" => Self::Network,
            "storage" => Self::Storage,
            "non_rpm_software" | "unmanaged_files" | "language_packages" => Self::Software,
            "secrets" | "subscription" => Self::Secrets,
            _ => Self::SystemConfig, // truly unknown IDs (not retired — retired IDs are caught by is_retired() before reaching this)
        }
    }

    /// Check if this group contains sections that have actionable findings
    /// (triage sections). Reference-only groups return false.
    pub fn has_actionable_sections(&self) -> bool {
        match self {
            Self::Packages
            | Self::SystemConfig
            | Self::Services
            | Self::Identity
            | Self::Software => true,
            Self::Network | Self::Storage | Self::Secrets => false,
        }
    }

    /// All groups in display order.
    pub fn all_in_order() -> &'static [SectionGroup] {
        &[
            Self::Packages,
            Self::SystemConfig,
            Self::Services,
            Self::Identity,
            Self::Network,
            Self::Storage,
            Self::Software,
            Self::Secrets,
        ]
    }

    /// URL-safe slug for HTML `id` attributes.
    ///
    /// Slugs are chosen to avoid collision with existing section IDs
    /// (e.g. `packages`, `storage`, `services`).
    pub fn slug(&self) -> &'static str {
        match self {
            Self::Packages => "packages-group",
            Self::SystemConfig => "system-config",
            Self::Services => "services-scheduling",
            Self::Identity => "identity",
            Self::Network => "network-group",
            Self::Storage => "storage-group",
            Self::Software => "software",
            Self::Secrets => "secrets",
        }
    }

    /// Canonical member sections for this group, in display order.
    ///
    /// Each entry carries the section's web-facing ID, its human-readable
    /// label, and whether the section appears in the triage (decision) view.
    pub fn member_sections(&self) -> &'static [SectionMeta] {
        match self {
            Self::Packages => &[SectionMeta {
                id: "packages",
                label: "Packages",
                is_triage: true,
            }],
            Self::SystemConfig => &[
                SectionMeta {
                    id: "config",
                    label: "Configuration Files",
                    is_triage: true,
                },
                SectionMeta {
                    id: "kernel_boot",
                    label: "Kernel & Boot",
                    is_triage: true,
                },
                SectionMeta {
                    id: "selinux",
                    label: "Security & Access Control",
                    is_triage: true,
                },
            ],
            Self::Services => &[
                SectionMeta {
                    id: "services",
                    label: "Services",
                    is_triage: true,
                },
                SectionMeta {
                    id: "scheduled_tasks",
                    label: "Scheduled Tasks",
                    is_triage: true,
                },
                SectionMeta {
                    id: "containers",
                    label: "Containers",
                    is_triage: true,
                },
            ],
            Self::Identity => &[SectionMeta {
                id: "users_groups",
                label: "Users & Groups",
                is_triage: true,
            }],
            Self::Network => &[SectionMeta {
                id: "network",
                label: "Network",
                is_triage: false,
            }],
            Self::Storage => &[SectionMeta {
                id: "storage",
                label: "Storage",
                is_triage: false,
            }],
            Self::Software => &[
                SectionMeta {
                    id: "non_rpm_software",
                    label: "Non-RPM Software",
                    is_triage: true,
                },
                SectionMeta {
                    id: "language_packages",
                    label: "Language Packages",
                    is_triage: true,
                },
                SectionMeta {
                    id: "unmanaged_files",
                    label: "Unmanaged Files",
                    is_triage: true,
                },
            ],
            Self::Secrets => &[
                SectionMeta {
                    id: "secrets",
                    label: "Secrets",
                    is_triage: false,
                },
                SectionMeta {
                    id: "subscription",
                    label: "Subscription",
                    is_triage: false,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_groups_have_labels() {
        for group in SectionGroup::all_in_order() {
            assert!(!group.label().is_empty());
        }
    }

    #[test]
    fn section_mapping_covers_known_sections() {
        assert_eq!(SectionGroup::for_section("rpm"), SectionGroup::Packages);
        assert_eq!(
            SectionGroup::for_section("config"),
            SectionGroup::SystemConfig
        );
        assert_eq!(
            SectionGroup::for_section("kernel_boot"),
            SectionGroup::SystemConfig
        );
        assert_eq!(
            SectionGroup::for_section("selinux"),
            SectionGroup::SystemConfig
        );
        assert_eq!(
            SectionGroup::for_section("services"),
            SectionGroup::Services
        );
        assert_eq!(
            SectionGroup::for_section("scheduled_tasks"),
            SectionGroup::Services
        );
        assert_eq!(
            SectionGroup::for_section("users_groups"),
            SectionGroup::Identity
        );
        assert_eq!(SectionGroup::for_section("storage"), SectionGroup::Storage);
        assert_eq!(
            SectionGroup::for_section("non_rpm_software"),
            SectionGroup::Software
        );
        assert_eq!(SectionGroup::for_section("secrets"), SectionGroup::Secrets);
        assert_eq!(
            SectionGroup::for_section("subscription"),
            SectionGroup::Secrets
        );
    }

    #[test]
    fn unknown_section_defaults_to_system_config() {
        assert_eq!(
            SectionGroup::for_section("something_unknown"),
            SectionGroup::SystemConfig
        );
    }

    #[test]
    fn all_in_order_contains_every_variant() {
        let all = SectionGroup::all_in_order();
        assert_eq!(all.len(), 8);
        assert!(all.contains(&SectionGroup::Packages));
        assert!(all.contains(&SectionGroup::SystemConfig));
        assert!(all.contains(&SectionGroup::Services));
        assert!(all.contains(&SectionGroup::Identity));
        assert!(all.contains(&SectionGroup::Network));
        assert!(all.contains(&SectionGroup::Storage));
        assert!(all.contains(&SectionGroup::Software));
        assert!(all.contains(&SectionGroup::Secrets));
    }

    #[test]
    fn slugs_are_unique() {
        let slugs: Vec<&str> = SectionGroup::all_in_order()
            .iter()
            .map(|g| g.slug())
            .collect();
        let mut deduped = slugs.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(slugs.len(), deduped.len(), "group slugs must be unique");
    }

    #[test]
    fn web_section_ids_all_resolve() {
        let web_ids = [
            "packages",
            "configs",
            "kernel_boot",
            "selinux",
            "services",
            "containers",
            "scheduled_tasks",
            "compose",
            "users_groups",
            "network",
            "storage",
            "non_rpm_software",
            "unmanaged_files",
            "language_packages",
            "secrets",
            "subscription",
        ];
        for id in web_ids {
            let _ = SectionGroup::for_section(id);
        }
    }

    #[test]
    fn retired_ids_are_explicitly_retired() {
        assert!(SectionGroup::is_retired("system_tuning"));
        assert!(SectionGroup::is_retired("version_changes"));
    }

    #[test]
    fn live_section_ids_are_not_retired() {
        let live_ids = [
            "packages",
            "configs",
            "kernel_boot",
            "selinux",
            "services",
            "containers",
            "scheduled_tasks",
            "compose",
            "users_groups",
            "network",
            "storage",
            "non_rpm_software",
            "unmanaged_files",
            "language_packages",
            "secrets",
            "subscription",
        ];
        for id in live_ids {
            assert!(!SectionGroup::is_retired(id), "{id} should not be retired");
        }
    }

    #[test]
    fn reference_only_groups_are_not_actionable() {
        assert!(!SectionGroup::Network.has_actionable_sections());
        assert!(!SectionGroup::Storage.has_actionable_sections());
        assert!(!SectionGroup::Secrets.has_actionable_sections());
    }

    #[test]
    fn triage_groups_are_actionable() {
        assert!(SectionGroup::Packages.has_actionable_sections());
        assert!(SectionGroup::SystemConfig.has_actionable_sections());
        assert!(SectionGroup::Services.has_actionable_sections());
        assert!(SectionGroup::Identity.has_actionable_sections());
        assert!(SectionGroup::Software.has_actionable_sections());
    }

    #[test]
    fn slugs_are_unique_and_stable() {
        let slugs: Vec<&str> = SectionGroup::all_in_order()
            .iter()
            .map(|g| g.slug())
            .collect();
        let mut deduped = slugs.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(slugs.len(), deduped.len(), "slugs must be unique");
    }

    #[test]
    fn member_sections_non_empty_for_all_groups() {
        for group in SectionGroup::all_in_order() {
            assert!(
                !group.member_sections().is_empty(),
                "{:?} must have at least one member section",
                group
            );
        }
    }

    #[test]
    fn member_section_ids_map_back_to_parent_group() {
        for group in SectionGroup::all_in_order() {
            for section in group.member_sections() {
                assert_eq!(
                    SectionGroup::for_section(section.id),
                    *group,
                    "section '{}' should map back to {:?}",
                    section.id,
                    group
                );
            }
        }
    }

    #[test]
    fn member_section_ids_are_unique_across_all_groups() {
        let all_ids: Vec<&str> = SectionGroup::all_in_order()
            .iter()
            .flat_map(|g| g.member_sections().iter().map(|s| s.id))
            .collect();
        let mut deduped = all_ids.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(
            all_ids.len(),
            deduped.len(),
            "section IDs must be unique across groups"
        );
    }
}
