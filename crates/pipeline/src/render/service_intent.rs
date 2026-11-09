use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::services::{ServiceAction, ServiceStateChange};

use inspectah_core::types::verified_upload::VerifiedUploads;

use super::not_carried::{NOT_CARRIED_REASON, not_carried_packages};
use super::repo_enabling::{RepoEnablingExport, repo_enabling_export};
use super::repoless::{PackageRoute, route_for};
use super::safety::sanitize_shell_value;

// ---------------------------------------------------------------------------
// Package installability helpers (unchanged from prior tasks)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManualFollowUpReason {
    LocalInstall,
    NoRepo,
    MissingSourceRepo,
}

fn package_display_name(pkg: &PackageEntry) -> String {
    if pkg.arch.is_empty() {
        pkg.name.clone()
    } else {
        format!("{}.{}", pkg.name, pkg.arch)
    }
}

fn package_state_label(pkg: &PackageEntry) -> String {
    serde_json::to_string(&pkg.state)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

/// The recorded source repo of a package installed by local RPM path.
const COMMANDLINE_REPO: &str = "@commandline";

fn manual_follow_up_reason(pkg: &PackageEntry) -> Option<ManualFollowUpReason> {
    match pkg.state {
        PackageState::LocalInstall => Some(ManualFollowUpReason::LocalInstall),
        PackageState::NoRepo => Some(ManualFollowUpReason::NoRepo),
        _ if pkg.source_repo.is_empty() => Some(ManualFollowUpReason::MissingSourceRepo),
        // `@commandline` is not a repo: the package arrived by local RPM
        // path, so `dnf install <name>` has nothing to resolve against.
        // Carrying a non-empty `source_repo` used to be enough on its own,
        // which rendered a `*-release` package as an active install *and*
        // as a repo-less MANUAL block for the same package. The one thing
        // that lifts the refusal is proof that a repo serves it by name,
        // and `repo_enabling_from` is that proof.
        _ if pkg.source_repo.eq_ignore_ascii_case(COMMANDLINE_REPO)
            && pkg.repo_enabling_from.is_empty() =>
        {
            Some(ManualFollowUpReason::NoRepo)
        }
        _ => None,
    }
}

/// Generate the TODO comment line for a package that cannot be installed via
/// `dnf install`. Returns `None` for packages that are directly installable.
///
/// Output matches the original containerfile renderer exactly — changing
/// wording here changes the Containerfile output.
pub(crate) fn manual_follow_up_line(pkg: &PackageEntry) -> Option<String> {
    match manual_follow_up_reason(pkg) {
        Some(ManualFollowUpReason::LocalInstall) => Some(format!(
            "# TODO: '{}' was installed locally (state: {}) \u{2014} provide a .rpm or custom repo.",
            package_display_name(pkg),
            package_state_label(pkg)
        )),
        Some(ManualFollowUpReason::NoRepo) => Some(format!(
            "# TODO: '{}' has no repository source (state: {}) \u{2014} provide a .rpm or custom repo.",
            package_display_name(pkg),
            package_state_label(pkg)
        )),
        Some(ManualFollowUpReason::MissingSourceRepo) => Some(format!(
            "# TODO: '{}' has no recorded repository source \u{2014} verify how to reinstall it in the image.",
            package_display_name(pkg)
        )),
        None => None,
    }
}

/// Returns `true` when a package can be installed via `dnf install` — i.e.,
/// it has a known repository source and is not a local-install or no-repo
/// package.
pub fn is_package_installable(pkg: &PackageEntry) -> bool {
    manual_follow_up_reason(pkg).is_none()
}

/// Whether the export installs `pkg` itself, on the route the
/// Containerfile renders it by.
///
/// It must be included, and then: a package installed by name must be
/// installable ([`is_package_installable`]), one from a host-disabled repo
/// always is, and a repo-less one only when the export carries its RPM (an
/// upload, taken on `uploaded` since no uploads are proved here, or a
/// cached RPM). A repo-enabling package whose serving repo the export
/// dropped is demoted to a FIXME and never installed. An included package
/// that fails this renders as a manual step only: a `# TODO`, `# MANUAL`
/// or `# FIXME` comment.
pub(crate) fn installs_on_its_own_route(rpm: &RpmSection, pkg: &PackageEntry) -> bool {
    pkg.disposition.is_included()
        && match route_for(rpm, pkg, &VerifiedUploads::new()) {
            PackageRoute::ByName => is_package_installable(pkg),
            PackageRoute::DisabledRepo(_) => true,
            PackageRoute::Upload(_) | PackageRoute::Repoless => pkg.uploaded || pkg.repoless_cached,
        }
        && !matches!(repo_enabling_export(rpm, pkg), RepoEnablingExport::Lost(_))
}

/// The canonical `name.arch` ids of `packages_added` entries that a leaf
/// the export installs pulls in as a dependency, whatever their own
/// disposition.
///
/// A package something installed depends on is in the image: dnf installs
/// it with the package that needs it. That holds for refine's default
/// exclusion of a non-leaf package and for an operator's explicit
/// exclusion alike, so "excluded" never means "absent" for such a package.
/// This is the one place that answers the question; the service tiers and
/// the package selection both ask it.
///
/// A leaf contributes only when [`installs_on_its_own_route`] holds for
/// it. An included leaf the export renders as a manual step (no repo
/// serves it and no RPM is carried, or a repo-enabling package demoted for
/// a dropped repo) is never installed by the image, so dnf never resolves
/// its dependencies either.
///
/// The answer comes from `leaf_dep_tree`, which maps each leaf to the
/// transitive closure of its auto (non-leaf) dependencies. Two limits
/// follow from what the tree records, and both fall back to treating the
/// package by its disposition alone, which is what the renderer did before
/// this existed:
///
/// - With no dependency data (`leaf_packages` unavailable, the tree empty)
///   the set is empty, so an excluded dependency reads as absent and its
///   unit is omitted even when dnf installs it.
/// - The tree lists only auto dependencies of leaves. A leaf another leaf
///   depends on, or a dependency of an auto package an operator included
///   on its own, is not in it.
pub fn pulled_in_by_included(rpm: &RpmSection) -> std::collections::HashSet<String> {
    let Some(tree) = rpm.leaf_dep_tree.as_object() else {
        return std::collections::HashSet::new();
    };
    let installed: std::collections::HashSet<String> = rpm
        .packages_added
        .iter()
        .filter(|pkg| installs_on_its_own_route(rpm, pkg))
        .map(|pkg| format!("{}.{}", pkg.name, pkg.arch))
        .collect();
    tree.iter()
        .filter(|(leaf, _)| installed.contains(leaf.as_str()))
        .filter_map(|(_, deps)| deps.as_array())
        .flatten()
        .filter_map(|dep| dep.as_str())
        .map(str::to_string)
        .collect()
}

/// Compute the set of package *names* (not `name.arch`) that the target
/// image is expected to contain. This merges baseline package names with
/// user-included added packages and the added packages an included one
/// pulls in ([`pulled_in_by_included`]).
pub fn effective_target_packages(rpm: &RpmSection) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    if let Some(baseline) = &rpm.baseline_package_names {
        names.extend(baseline.iter().cloned());
    }
    let pulled_in = pulled_in_by_included(rpm);
    names.extend(
        rpm.packages_added
            .iter()
            .filter(|pkg| {
                pkg.disposition.is_included()
                    || pulled_in.contains(&format!("{}.{}", pkg.name, pkg.arch))
            })
            .map(|pkg| pkg.name.clone()),
    );
    names
}

// ---------------------------------------------------------------------------
// Service omission / advisory decision engine
// ---------------------------------------------------------------------------

/// A service that was dropped from the Containerfile because its owning
/// package is absent from the target image. The omission is recorded as a
/// `# Omitted:` comment and surfaced through the refine projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceOmission {
    pub unit: String,
    pub owning_package: String,
    pub reason: OmissionReason,
}

/// Why a service's owning package is absent from the target image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmissionReason {
    /// Every `packages_added` entry for the owning package is excluded,
    /// and no included package pulls it in as a dependency.
    PackageExcluded,
    /// A baseline is available and the owning package is in neither the
    /// baseline nor `packages_added`.
    NotInTargetImage,
    /// The owning package is absent, as for [`Self::PackageExcluded`], and
    /// every `packages_added` entry of its name is in the not-carried list
    /// (`not_carried::not_carried_packages`). Nothing included needs it,
    /// which is why it is absent, so this wins over `PackageExcluded` even
    /// when an operator's exclusion led there.
    NotCarried,
}

impl OmissionReason {
    /// Human-readable explanation, shared by the Containerfile comment and
    /// the refine UI so both surfaces say the same thing.
    pub fn describe(self, owning_package: &str) -> String {
        match self {
            OmissionReason::PackageExcluded => {
                format!("package '{owning_package}' was excluded")
            }
            OmissionReason::NotInTargetImage => {
                format!("package '{owning_package}' not in target image")
            }
            OmissionReason::NotCarried => {
                format!("package '{owning_package}' is not carried: {NOT_CARRIED_REASON}")
            }
        }
    }
}

/// A service that IS emitted but carries supplemental context about why
/// the renderer couldn't fully validate its presence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceAdvisory {
    pub unit: String,
    pub owning_package: String,
    pub reasons: Vec<AdvisoryReason>,
}

/// Why a service received an advisory annotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdvisoryReason {
    /// The owning package is in `packages_added` with `include: true` but
    /// is not installable (LocalInstall / NoRepo / missing source_repo).
    PackageUnreachable,
    /// No baseline was available, so we cannot prove the package is absent.
    BaselineUnavailable,
}

/// The output of the service decision engine — lines for the Containerfile
/// plus structured omission/advisory metadata for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRenderPlan {
    pub lines: Vec<String>,
    pub omissions: Vec<ServiceOmission>,
    pub advisories: Vec<ServiceAdvisory>,
}

/// Internal classification of whether a service should appear in output.
enum PresenceDecision {
    /// Drop this service — owning package absent from the target image.
    Omit {
        owning_package: String,
        reason: OmissionReason,
    },
    /// Keep this service; optionally attach advisory reasons.
    Emit {
        advisory_reasons: Option<(String, Vec<AdvisoryReason>)>,
    },
}

// ---------------------------------------------------------------------------
// systemctl_lines — format a RUN block (unchanged from containerfile.rs)
// ---------------------------------------------------------------------------

/// Format a `RUN systemctl enable/disable/mask` block using multi-line
/// backslash continuation for consistency with the `RUN dnf install` style.
fn systemctl_lines(verb: &str, units: &[String]) -> Vec<String> {
    let mut lines = vec![format!("RUN systemctl {} \\", verb)];
    for (i, u) in units.iter().enumerate() {
        if i < units.len() - 1 {
            lines.push(format!("    {} \\", u));
        } else {
            lines.push(format!("    {}", u));
        }
    }
    lines
}

// ---------------------------------------------------------------------------
// config_tree_units — collect timer-associated units from scheduled_tasks
// ---------------------------------------------------------------------------

fn config_tree_units(snap: &InspectionSnapshot) -> std::collections::HashSet<String> {
    let mut units = std::collections::HashSet::new();
    if let Some(st) = &snap.scheduled_tasks {
        for t in &st.systemd_timers {
            if t.source == "local" && !t.name.is_empty() {
                units.insert(format!("{}.timer", t.name));
                units.insert(format!("{}.service", t.name));
            }
        }
        for u in &st.generated_timer_units {
            if u.disposition.is_included() && !u.name.is_empty() {
                if !u.timer_content.is_empty() {
                    units.insert(format!("{}.timer", u.name));
                }
                if !u.service_content.is_empty() {
                    units.insert(format!("{}.service", u.name));
                }
            }
        }
    }
    units
}

// ---------------------------------------------------------------------------
// classify_service_presence — the tiered suppression model
// ---------------------------------------------------------------------------

/// Determine whether a service should be omitted or emitted, and if emitted,
/// whether it needs advisory annotations.
///
/// Tier logic (evaluated in order):
/// 1. `owning_package: None` → always Emit (conservative — unknown owner)
/// 2. Package in `packages_added` with every entry `include: false`, not
///    in the baseline, and not pulled in by an included package
///    ([`pulled_in_by_included`]) → Omit with `OmissionReason::NotCarried`
///    when every entry of the name is in `not_carried`, else
///    `OmissionReason::PackageExcluded`.
///    A pulled-in package is in the image, so it falls through to Tier 5.
///    An exclusion is the strongest absence signal the model has, and
///    `systemctl enable` on a unit whose package is not installed fails the
///    build. The one known edge is dnf pulling the excluded package back in
///    as a dependency of something still included (the renderer passes no
///    `--exclude`); the recorded omission is what tells the operator to
///    enable the unit by hand in that case.
/// 3. Package in `packages_added` with `include: true` but not installable
///    → Emit with `PackageUnreachable` (+ `BaselineUnavailable`)
/// 4. Package in `packages_added` with `include: true` and installable → Emit clean
/// 5. Package in `target_packages` (baseline) → Emit clean
/// 6. Package not found anywhere + baseline unavailable → Emit with `BaselineUnavailable`
/// 7. Package not found anywhere + baseline available → Omit with
///    `OmissionReason::NotInTargetImage`
fn classify_service_presence(
    sc: &ServiceStateChange,
    rpm: &RpmSection,
    target_packages: &std::collections::BTreeSet<String>,
    not_carried: &std::collections::HashSet<String>,
    baseline_unavailable: bool,
) -> PresenceDecision {
    let pkg_name = match &sc.owning_package {
        Some(name) => name,
        // Tier 1: unknown owner — always emit conservatively
        None => {
            return PresenceDecision::Emit {
                advisory_reasons: None,
            };
        }
    };

    // Check all entries with this package name — duplicates may differ in
    // include/installability state.  Best-case wins: any included+installable
    // entry means the package is present.
    let matching_packages: Vec<&PackageEntry> = rpm
        .packages_added
        .iter()
        .filter(|p| p.name == *pkg_name)
        .collect();

    if !matching_packages.is_empty() {
        // Tier 4: if ANY entry is included AND installable → proven present
        let any_installable = matching_packages
            .iter()
            .any(|pkg| pkg.disposition.is_included() && is_package_installable(pkg));
        if any_installable {
            return PresenceDecision::Emit {
                advisory_reasons: None,
            };
        }

        // Tier 3: if ANY entry is included but not installable → PackageUnreachable
        let any_included_not_installable = matching_packages
            .iter()
            .any(|pkg| pkg.disposition.is_included() && !is_package_installable(pkg));
        if any_included_not_installable {
            let mut reasons = vec![AdvisoryReason::PackageUnreachable];
            if baseline_unavailable {
                reasons.push(AdvisoryReason::BaselineUnavailable);
            }
            return PresenceDecision::Emit {
                advisory_reasons: Some((pkg_name.clone(), reasons)),
            };
        }

        // Tier 2: all entries are excluded. A baseline that still ships the
        // package (another arch excluded, say) keeps the unit present, so
        // fall through to Tier 5 in that case.
        if let Some(omit) = excluded_owner(sc, rpm, target_packages, not_carried) {
            return omit;
        }
    }

    // Check if the package is in the effective target set (baseline or included-added)
    if target_packages.contains(pkg_name) {
        // Tier 5: baseline package — emit clean
        return PresenceDecision::Emit {
            advisory_reasons: None,
        };
    }

    // Package not found anywhere
    if baseline_unavailable {
        // Tier 6: can't prove absence without baseline
        PresenceDecision::Emit {
            advisory_reasons: Some((pkg_name.clone(), vec![AdvisoryReason::BaselineUnavailable])),
        }
    } else {
        // Tier 7: baseline available and package not in it — proven absent
        PresenceDecision::Omit {
            owning_package: pkg_name.clone(),
            reason: OmissionReason::NotInTargetImage,
        }
    }
}

/// Tier 2 on its own: the owning package has `packages_added` entries, and
/// none of them is included, pulled in by an included package, or shipped
/// by the baseline.
///
/// `target_packages` holds every included or pulled-in entry's name, so
/// "not in it" already says no entry for the name is either.
///
/// The presence check alone decides the omission; `not_carried` (the
/// `name.arch` ids of the not-carried list) only names the reason:
/// [`OmissionReason::NotCarried`] when every entry of the name is listed,
/// [`OmissionReason::PackageExcluded`] otherwise.
fn excluded_owner(
    sc: &ServiceStateChange,
    rpm: &RpmSection,
    target_packages: &std::collections::BTreeSet<String>,
    not_carried: &std::collections::HashSet<String>,
) -> Option<PresenceDecision> {
    let pkg_name = sc.owning_package.as_ref()?;
    let mut entries = rpm
        .packages_added
        .iter()
        .filter(|p| p.name == *pkg_name)
        .peekable();
    if entries.peek().is_none() || target_packages.contains(pkg_name) {
        return None;
    }
    let reason = if entries.all(|p| not_carried.contains(&format!("{}.{}", p.name, p.arch))) {
        OmissionReason::NotCarried
    } else {
        OmissionReason::PackageExcluded
    };
    Some(PresenceDecision::Omit {
        owning_package: pkg_name.clone(),
        reason,
    })
}

/// The presence decision for an aggregate snapshot.
///
/// An aggregate's `packages_added` is leaf-only, so the tiers that read
/// absence from it (3, 6 and 7) would omit the unit of an auto dependency
/// that the image does install. Tier 2 reads an explicit exclusion on an
/// entry that is present, which leaf filtering cannot fake, so it applies
/// here as it does to a single host; every other unit emits.
fn aggregate_presence(
    sc: &ServiceStateChange,
    rpm: &RpmSection,
    target_packages: &std::collections::BTreeSet<String>,
    not_carried: &std::collections::HashSet<String>,
) -> PresenceDecision {
    excluded_owner(sc, rpm, target_packages, not_carried).unwrap_or(PresenceDecision::Emit {
        advisory_reasons: None,
    })
}

// ---------------------------------------------------------------------------
// render_service_intent — the main entry point
// ---------------------------------------------------------------------------

/// Produce a `ServiceRenderPlan` from the snapshot. This is the single
/// authority for service omissions and advisories — `containerfile.rs`
/// delegates its service section to this function.
pub fn render_service_intent(snap: &InspectionSnapshot) -> ServiceRenderPlan {
    let ct_units = config_tree_units(snap);

    let services = match &snap.services {
        Some(s) => s,
        None => {
            return ServiceRenderPlan {
                lines: Vec::new(),
                omissions: Vec::new(),
                advisories: Vec::new(),
            };
        }
    };

    // When no RPM section exists, fall back to emitting everything
    // without omission/advisory logic (no package data to reason about).
    let rpm = match &snap.rpm {
        Some(r) => r,
        None => return render_without_rpm(snap, services, &ct_units),
    };

    let target_packages = effective_target_packages(rpm);
    let baseline_unavailable = rpm.baseline_package_names.is_none();
    // Empty for an aggregate, which `not_carried` never classifies.
    let not_carried: std::collections::HashSet<String> = not_carried_packages(snap)
        .into_iter()
        .map(|pkg| format!("{}.{}", pkg.name, pkg.arch))
        .collect();

    let included_changes: Vec<_> = services
        .state_changes
        .iter()
        .filter(|sc| sc.disposition.is_included())
        .collect();

    if included_changes.is_empty() {
        return ServiceRenderPlan {
            lines: Vec::new(),
            omissions: Vec::new(),
            advisories: Vec::new(),
        };
    }

    // Aggregate snapshots use leaf-only packages_added, so
    // classify_service_presence would incorrectly omit services whose
    // owning package is an auto (non-leaf) dependency. Aggregate applies
    // Tier 2 alone; see `aggregate_presence`.
    let is_aggregate = snap.aggregate_meta.is_some();

    let mut omissions = Vec::new();
    let mut omission_comments = Vec::new();
    let mut advisories = Vec::new();
    let mut safe_enabled = Vec::new();
    let mut safe_disabled = Vec::new();
    let mut safe_masked = Vec::new();
    let mut deferred = Vec::new();

    for sc in &included_changes {
        let u = &sc.unit;
        if sanitize_shell_value(u).is_none() {
            continue;
        }

        // Evaluate omission BEFORE config-tree deferral — a proven-absent
        // service never becomes deferred fiction.
        let presence = if is_aggregate {
            aggregate_presence(sc, rpm, &target_packages, &not_carried)
        } else {
            classify_service_presence(
                sc,
                rpm,
                &target_packages,
                &not_carried,
                baseline_unavailable,
            )
        };

        match presence {
            PresenceDecision::Omit {
                owning_package,
                reason,
            } => {
                omission_comments.push(format!(
                    "# Omitted: {} ({})",
                    u,
                    reason.describe(&owning_package)
                ));
                omissions.push(ServiceOmission {
                    unit: u.clone(),
                    owning_package,
                    reason,
                });
                continue;
            }
            PresenceDecision::Emit { advisory_reasons } => {
                if let Some((pkg, reasons)) = advisory_reasons {
                    advisories.push(ServiceAdvisory {
                        unit: u.clone(),
                        owning_package: pkg,
                        reasons,
                    });
                }
            }
        }

        // Advisory services remain in the main action list — advisory is
        // supplemental context, not a reason to exclude.
        match sc.implied_action() {
            ServiceAction::Enable => {
                if ct_units.contains(u.as_str()) {
                    deferred.push(u.clone());
                } else {
                    safe_enabled.push(u.clone());
                }
            }
            ServiceAction::Disable => {
                safe_disabled.push(u.clone());
            }
            ServiceAction::Mask => {
                safe_masked.push(u.clone());
            }
        }
    }

    let mut lines = Vec::new();
    lines.extend(omission_comments);
    if !safe_enabled.is_empty() {
        lines.extend(systemctl_lines("enable", &safe_enabled));
    }
    if !safe_disabled.is_empty() {
        lines.extend(systemctl_lines("disable", &safe_disabled));
    }
    if !safe_masked.is_empty() {
        lines.extend(systemctl_lines("mask", &safe_masked));
    }
    if !deferred.is_empty() {
        lines.push(format!(
            "# {} unit(s) deferred to Scheduled Tasks section: {}",
            deferred.len(),
            deferred.join(", ")
        ));
    }

    ServiceRenderPlan {
        lines,
        omissions,
        advisories,
    }
}

/// Fallback renderer when no RPM section is available — emit all included
/// services without omission/advisory logic.
fn render_without_rpm(
    _snap: &InspectionSnapshot,
    services: &inspectah_core::types::services::ServiceSection,
    ct_units: &std::collections::HashSet<String>,
) -> ServiceRenderPlan {
    let included_changes: Vec<_> = services
        .state_changes
        .iter()
        .filter(|sc| sc.disposition.is_included())
        .collect();

    if included_changes.is_empty() {
        return ServiceRenderPlan {
            lines: Vec::new(),
            omissions: Vec::new(),
            advisories: Vec::new(),
        };
    }

    let mut safe_enabled = Vec::new();
    let mut safe_disabled = Vec::new();
    let mut safe_masked = Vec::new();
    let mut deferred = Vec::new();

    for sc in &included_changes {
        let u = &sc.unit;
        if sanitize_shell_value(u).is_none() {
            continue;
        }
        match sc.implied_action() {
            ServiceAction::Enable => {
                if ct_units.contains(u.as_str()) {
                    deferred.push(u.clone());
                } else {
                    safe_enabled.push(u.clone());
                }
            }
            ServiceAction::Disable => {
                safe_disabled.push(u.clone());
            }
            ServiceAction::Mask => {
                safe_masked.push(u.clone());
            }
        }
    }

    let mut lines = Vec::new();
    if !safe_enabled.is_empty() {
        lines.extend(systemctl_lines("enable", &safe_enabled));
    }
    if !safe_disabled.is_empty() {
        lines.extend(systemctl_lines("disable", &safe_disabled));
    }
    if !safe_masked.is_empty() {
        lines.extend(systemctl_lines("mask", &safe_masked));
    }
    if !deferred.is_empty() {
        lines.push(format!(
            "# {} unit(s) deferred to Scheduled Tasks section: {}",
            deferred.len(),
            deferred.join(", ")
        ));
    }

    ServiceRenderPlan {
        lines,
        omissions: Vec::new(),
        advisories: Vec::new(),
    }
}
