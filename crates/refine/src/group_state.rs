//! Group state derivation logic.
//!
//! Determines which [`GroupRenderState`] variant applies to an installed
//! DNF group based on the effective projected packages, group-level
//! exclude state, per-group divergent overrides, and ungrouped status.

use std::collections::HashSet;

use inspectah_core::types::FindingKind;
use inspectah_core::types::group_render::{DegradationReason, GroupRenderState};
use inspectah_core::types::rpm::{InstalledGroup, PackageEntry};

/// Context needed to evaluate a single group's rendering state.
pub struct GroupEvalContext<'a> {
    /// The installed group definition (name + members).
    pub group: &'a InstalledGroup,
    /// Effective projected packages for member lookup.
    pub effective_packages: &'a [PackageEntry],
    /// Whether this group has been dissolved via an UngroupGroup directive.
    pub ungrouped: bool,
    /// Whether a group-level SetInclude(false) is active.
    pub group_excluded: bool,
    /// Package names with individual ops AFTER the most recent group-level
    /// op, whose result DIVERGES from the group op's intent. Built
    /// per-group during timeline scanning.
    pub divergent_overrides: &'a HashSet<String>,
}

/// The effective member set for a group op: declared members present in
/// `packages_added`, actionable (not advisory/inventory), and not locked.
/// One definition, used three times (§ 3.7): the no-op refusal
/// (`RefineSession::check_group_noop`), `GroupInfo`'s `effective_member_count`
/// and `effective_included`, and `derive_group_state`'s own evaluation below,
/// via the caller that builds its `effective_packages`.
pub fn effective_members<'a>(
    packages_added: &'a [PackageEntry],
    members: &[String],
) -> Vec<&'a PackageEntry> {
    packages_added
        .iter()
        .filter(|p| members.contains(&p.name))
        .filter(|p| matches!(p.disposition, FindingKind::Actionable { .. }))
        .filter(|p| !p.locked)
        .collect()
}

/// Determine the rendering state for a single installed group.
///
/// Priority order:
/// 1. Ungrouped — group dissolved, members render individually
/// 2. Multilib conflict — member appears in multiple architectures
/// 3. Divergent override — member op contradicts group-level op
/// 4. Inert — no effective member at all (all locked, all base, or no
///    member present); nothing to render and nothing to decide
/// 5. Excluded — group-level exclude with all non-locked members off
/// 6. Member excluded — non-locked member excluded without group op
/// 7. Renderable — all non-locked members included
pub fn derive_group_state(ctx: &GroupEvalContext) -> GroupRenderState {
    // Priority 1: ungrouped dissolves the group entirely.
    if ctx.ungrouped {
        return GroupRenderState::Ungrouped;
    }

    let mut any_non_locked_excluded = false;
    let mut all_non_locked_excluded = true;
    let mut has_non_locked = false;

    for member_name in &ctx.group.members {
        let matching: Vec<_> = ctx
            .effective_packages
            .iter()
            .filter(|p| p.name == *member_name)
            .collect();

        if matching.is_empty() {
            continue;
        }

        // Priority 2: multi-arch conflict.
        if matching.len() > 1 {
            return GroupRenderState::Degraded {
                reason: DegradationReason::MultilibConflict,
            };
        }

        let pkg = &matching[0];

        // Locked packages are invisible to group state logic.
        if pkg.locked {
            continue;
        }

        has_non_locked = true;

        // Priority 3: divergent override.
        if ctx.divergent_overrides.contains(&pkg.name) {
            return GroupRenderState::Degraded {
                reason: DegradationReason::MemberOverridden,
            };
        }

        if pkg.disposition.is_included() {
            all_non_locked_excluded = false;
        } else {
            any_non_locked_excluded = true;
        }
    }

    // Priority 4: no effective member at all. Without this, the fallthrough
    // below would call a group with nothing to evaluate `Renderable`, which
    // is item 1.9's bug: an empty group still emits `dnf group install`.
    if !has_non_locked {
        return GroupRenderState::Inert;
    }

    // Priority 5: explicit group exclude with all non-locked confirmed off.
    if ctx.group_excluded && all_non_locked_excluded {
        return GroupRenderState::Excluded;
    }

    // Priority 6: non-locked member excluded without group op.
    if any_non_locked_excluded {
        return GroupRenderState::Degraded {
            reason: DegradationReason::MemberExcluded,
        };
    }

    // Priority 7: everything checks out.
    GroupRenderState::Renderable
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(name: &str, members: &[&str]) -> InstalledGroup {
        InstalledGroup {
            name: name.into(),
            members: members.iter().map(|s| s.to_string()).collect(),
            optional_installed: vec![],
        }
    }

    fn pkg(name: &str, arch: &str, include: bool, locked: bool) -> PackageEntry {
        PackageEntry {
            name: name.into(),
            arch: arch.into(),
            disposition: FindingKind::from_bool(include),
            locked,
            ..PackageEntry::default()
        }
    }

    fn packages_all_included(names: &[&str]) -> Vec<PackageEntry> {
        names
            .iter()
            .map(|n| pkg(n, "x86_64", true, false))
            .collect()
    }

    fn packages_all_excluded(names: &[&str]) -> Vec<PackageEntry> {
        names
            .iter()
            .map(|n| pkg(n, "x86_64", false, false))
            .collect()
    }

    fn packages_with_overrides(specs: &[(&str, bool)]) -> Vec<PackageEntry> {
        specs
            .iter()
            .map(|(n, inc)| pkg(n, "x86_64", *inc, false))
            .collect()
    }

    #[test]
    fn all_members_included_no_overrides_is_renderable() {
        let ctx = GroupEvalContext {
            group: &group("Dev Tools", &["gcc", "make"]),
            effective_packages: &packages_all_included(&["gcc", "make"]),
            ungrouped: false,
            group_excluded: false,
            divergent_overrides: &HashSet::new(),
        };
        assert_eq!(derive_group_state(&ctx), GroupRenderState::Renderable);
    }

    #[test]
    fn group_level_exclude_with_all_members_off_is_excluded() {
        let ctx = GroupEvalContext {
            group: &group("Dev Tools", &["gcc", "make"]),
            effective_packages: &packages_all_excluded(&["gcc", "make"]),
            ungrouped: false,
            group_excluded: true,
            divergent_overrides: &HashSet::new(),
        };
        assert_eq!(derive_group_state(&ctx), GroupRenderState::Excluded);
    }

    #[test]
    fn group_excluded_but_member_reincluded_individually_is_degraded_overridden() {
        let mut divergent = HashSet::new();
        divergent.insert("gcc".into());
        let ctx = GroupEvalContext {
            group: &group("Dev Tools", &["gcc", "make"]),
            effective_packages: &packages_with_overrides(&[("gcc", true), ("make", false)]),
            ungrouped: false,
            group_excluded: true,
            divergent_overrides: &divergent,
        };
        assert!(matches!(
            derive_group_state(&ctx),
            GroupRenderState::Degraded {
                reason: DegradationReason::MemberOverridden
            }
        ));
    }

    #[test]
    fn reaffirming_member_op_does_not_degrade() {
        let ctx = GroupEvalContext {
            group: &group("Dev Tools", &["gcc", "make"]),
            effective_packages: &packages_all_excluded(&["gcc", "make"]),
            ungrouped: false,
            group_excluded: true,
            divergent_overrides: &HashSet::new(),
        };
        assert_eq!(derive_group_state(&ctx), GroupRenderState::Excluded);
    }

    #[test]
    fn member_excluded_without_group_op_is_degraded_member_excluded() {
        let ctx = GroupEvalContext {
            group: &group("Dev Tools", &["gcc", "make"]),
            effective_packages: &packages_with_overrides(&[("gcc", true), ("make", false)]),
            ungrouped: false,
            group_excluded: false,
            divergent_overrides: &HashSet::new(),
        };
        assert!(matches!(
            derive_group_state(&ctx),
            GroupRenderState::Degraded {
                reason: DegradationReason::MemberExcluded
            }
        ));
    }

    #[test]
    fn multilib_member_is_degraded() {
        let pkgs = vec![
            pkg("glibc", "x86_64", true, false),
            pkg("glibc", "i686", true, false),
            pkg("gcc", "x86_64", true, false),
        ];
        let ctx = GroupEvalContext {
            group: &group("Dev Tools", &["glibc", "gcc"]),
            effective_packages: &pkgs,
            ungrouped: false,
            group_excluded: false,
            divergent_overrides: &HashSet::new(),
        };
        assert!(matches!(
            derive_group_state(&ctx),
            GroupRenderState::Degraded {
                reason: DegradationReason::MultilibConflict
            }
        ));
    }

    #[test]
    fn locked_members_do_not_trigger_degradation() {
        let pkgs = vec![
            pkg("gcc", "x86_64", true, false),
            pkg("binutils", "x86_64", false, true), // locked, excluded
        ];
        let ctx = GroupEvalContext {
            group: &group("Dev Tools", &["gcc", "binutils"]),
            effective_packages: &pkgs,
            ungrouped: false,
            group_excluded: false,
            divergent_overrides: &HashSet::new(),
        };
        assert_eq!(derive_group_state(&ctx), GroupRenderState::Renderable);
    }

    #[test]
    fn ungrouped_group_is_ungrouped() {
        let ctx = GroupEvalContext {
            group: &group("Dev Tools", &["gcc", "make"]),
            effective_packages: &packages_all_included(&["gcc", "make"]),
            ungrouped: true,
            group_excluded: false,
            divergent_overrides: &HashSet::new(),
        };
        assert_eq!(derive_group_state(&ctx), GroupRenderState::Ungrouped);
    }

    #[test]
    fn shared_member_op_for_other_group_does_not_degrade_this_group() {
        let ctx = GroupEvalContext {
            group: &group("Group A", &["x", "y"]),
            effective_packages: &packages_all_included(&["x", "y"]),
            ungrouped: false,
            group_excluded: false,
            divergent_overrides: &HashSet::new(),
        };
        assert_eq!(derive_group_state(&ctx), GroupRenderState::Renderable);
    }
}
