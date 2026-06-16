use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::services::{ServiceAction, ServiceStateChange};

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

fn manual_follow_up_reason(pkg: &PackageEntry) -> Option<ManualFollowUpReason> {
    match pkg.state {
        PackageState::LocalInstall => Some(ManualFollowUpReason::LocalInstall),
        PackageState::NoRepo => Some(ManualFollowUpReason::NoRepo),
        _ if pkg.source_repo.is_empty() => Some(ManualFollowUpReason::MissingSourceRepo),
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

/// Compute the set of package *names* (not `name.arch`) that the target
/// image is expected to contain. This merges baseline package names with
/// user-included added packages.
pub fn effective_target_packages(rpm: &RpmSection) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    if let Some(baseline) = &rpm.baseline_package_names {
        names.extend(baseline.iter().cloned());
    }
    names.extend(
        rpm.packages_added
            .iter()
            .filter(|pkg| pkg.include)
            .map(|pkg| pkg.name.clone()),
    );
    names
}

// ---------------------------------------------------------------------------
// Service omission / advisory decision engine
// ---------------------------------------------------------------------------

/// A service that was silently dropped from the Containerfile because its
/// owning package is proven absent from the target image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceOmission {
    pub unit: String,
    pub owning_package: String,
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
    /// The owning package is in `packages_added` with `include: false`.
    PackageExcluded,
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
    /// Drop this service — owning package proven absent.
    Omit { owning_package: String },
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
            if u.include && !u.name.is_empty() {
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
/// 2. Package in `packages_added` with `include: false` → Emit with
///    `PackageExcluded` (+ `BaselineUnavailable` if applicable)
/// 3. Package in `packages_added` with `include: true` but not installable
///    → Emit with `PackageUnreachable` (+ `BaselineUnavailable`)
/// 4. Package in `packages_added` with `include: true` and installable → Emit clean
/// 5. Package in `target_packages` (baseline) → Emit clean
/// 6. Package not found anywhere + baseline unavailable → Emit with `BaselineUnavailable`
/// 7. Package not found anywhere + baseline available → Omit
fn classify_service_presence(
    sc: &ServiceStateChange,
    rpm: &RpmSection,
    target_packages: &std::collections::BTreeSet<String>,
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
            .any(|pkg| pkg.include && is_package_installable(pkg));
        if any_installable {
            return PresenceDecision::Emit {
                advisory_reasons: None,
            };
        }

        // Tier 3: if ANY entry is included but not installable → PackageUnreachable
        let any_included_not_installable = matching_packages
            .iter()
            .any(|pkg| pkg.include && !is_package_installable(pkg));
        if any_included_not_installable {
            let mut reasons = vec![AdvisoryReason::PackageUnreachable];
            if baseline_unavailable {
                reasons.push(AdvisoryReason::BaselineUnavailable);
            }
            return PresenceDecision::Emit {
                advisory_reasons: Some((pkg_name.clone(), reasons)),
            };
        }

        // Tier 2: all entries are excluded → PackageExcluded
        let mut reasons = vec![AdvisoryReason::PackageExcluded];
        if baseline_unavailable {
            reasons.push(AdvisoryReason::BaselineUnavailable);
        }
        return PresenceDecision::Emit {
            advisory_reasons: Some((pkg_name.clone(), reasons)),
        };
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
        }
    }
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

    let included_changes: Vec<_> = services
        .state_changes
        .iter()
        .filter(|sc| sc.include)
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
    // owning package is an auto (non-leaf) dependency. Skip classification
    // entirely for aggregate — force all services to Emit.
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
            PresenceDecision::Emit {
                advisory_reasons: None,
            }
        } else {
            classify_service_presence(sc, rpm, &target_packages, baseline_unavailable)
        };

        match presence {
            PresenceDecision::Omit { owning_package } => {
                omission_comments.push(format!(
                    "# Omitted: {} (package '{}' not in target image)",
                    u, owning_package
                ));
                omissions.push(ServiceOmission {
                    unit: u.clone(),
                    owning_package,
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
        .filter(|sc| sc.include)
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
