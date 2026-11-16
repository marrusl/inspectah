//! The unit plan (§ 10.1 and § 10.2 of the units spec): which unit-state
//! `systemctl` commands the export carries, checked against the files the
//! stager writes, and the lines that write them. The package presence tiers
//! it uses, and the package installability helpers the package sections
//! share, live here too.

use inspectah_core::baseline::{
    TARGET_NEEDS_UNIT, TMP_MOUNT, is_runtime_updater, target_needs_unit, target_needs_unit_text,
};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::redaction::{RedactionKind, RedactionState};
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::scheduled::TimerTrigger;
use inspectah_core::types::units::{
    AlsoMember, CarriedUnit, InstallUnknown, LinkRelation, ManualLink, ManualLinkCause,
    MemberLinks, MountUnitFinding, OrdinaryKind, StorageKind, SystemdDropIn, TargetUnitName,
    UndeterminedInstall, UnitAction, UnitFragment, UnitKind, UnitName, UnitState, UnitStateChange,
    holds_install_section, systemd_reads_drop_in,
};

use inspectah_core::types::verified_upload::VerifiedUploads;
use std::collections::BTreeMap;

use super::comment::escape_comment_text;
pub use super::configtree::Reservation;
use super::configtree::{
    Destination, ETC_UNIT_DIR, FLATPAK_SERVICE, PathError, StagedWrites, WithheldWhy, is_staged,
    staged_unit_file,
};
use super::not_carried::{NOT_CARRIED_REASON, not_carried_packages};
use super::repo_enabling::{RepoEnablingExport, repo_enabling_export};
use super::repoless::{PackageRoute, route_for};
use super::safety::{operand_refusal, single_quote};
use super::selinux::{
    BASE_IMAGE_DIRECTORIES, BASE_VAR_SYMLINKS, BOOTC_OWNED_MOUNT_POINTS, IMAGE_USR,
};

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
/// dropped is demoted to a FIXME and not installed, unless it was
/// uploaded: the Containerfile then installs it from the upload in the
/// repo-less block, as it does any uploaded repo-less package. An
/// included package that fails this renders as a manual step only: a
/// `# TODO`, `# MANUAL` or `# FIXME` comment.
pub(crate) fn installs_on_its_own_route(rpm: &RpmSection, pkg: &PackageEntry) -> bool {
    pkg.disposition.is_included()
        && match route_for(rpm, pkg, &VerifiedUploads::new()) {
            PackageRoute::ByName => is_package_installable(pkg),
            PackageRoute::DisabledRepo(_) => true,
            PackageRoute::Upload(_) | PackageRoute::Repoless => pkg.uploaded || pkg.repoless_cached,
        }
        && (pkg.uploaded || !matches!(repo_enabling_export(rpm, pkg), RepoEnablingExport::Lost(_)))
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
/// a dropped repo with no upload) is never installed by the image, so dnf
/// never resolves its dependencies either.
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
// The unit plan: every `systemctl` line for a collected unit
// ---------------------------------------------------------------------------

/// Why the plan writes no command for a unit it was asked to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OmissionReason {
    /// Every `packages_added` entry for the owning package is excluded,
    /// and no included package pulls it in as a dependency.
    PackageExcluded { package: String },
    /// A baseline is available and the owning package is in neither the
    /// baseline nor `packages_added`.
    NotInTargetImage { package: String },
    /// The owning package is absent, as for [`Self::PackageExcluded`], and
    /// every `packages_added` entry of its name is in the not-carried list
    /// (`not_carried::not_carried_packages`). Nothing included needs it,
    /// which is why it is absent, so this wins over `PackageExcluded` even
    /// when an operator's exclusion led there.
    NotCarried { package: String },
    /// A local unit's own file is not in the export (§ 10.2 rule 5).
    UnitFileNotStaged,
    /// The file that supplies `[Install]` in the image is not the one the
    /// row recorded (§ 10.2 rule 5). `path` is that file's image path.
    InstallEvidenceChanged { path: String },
    /// The export carries an administrator's unit file at a package-owned
    /// storage unit's name, which the image would load in place of the
    /// package's file (§ 10.2 rule 2). `path` is that file's image path.
    StorageFileStaged { path: String },
    /// The target needs the unit, or needs it unmasked (§ 10.2 rule 3,
    /// § 10.3). Rendered as a FIXME, not an `# Omitted:` line.
    TargetNeedsUnit,
    /// A drop-in that sets a timer's `Unit=` is not in the export, so the
    /// unit it would start is not the recorded trigger (§ 10.2 rule 5).
    /// `path` is that drop-in's recorded path.
    TimerTriggerEvidenceChanged { path: String },
    /// A timer's `Unit=` could not be resolved to a unit name (§ 10.2
    /// rule 5).
    TimerTriggerUnresolved,
    /// A timer's trigger is a local unit whose file is not in the export,
    /// and nothing else provides it (§ 10.2 rule 5).
    TimerTriggerNotStaged { trigger: UnitName },
    /// A mask and a staged unit file claim one path: `systemctl mask`
    /// exits 1 on a regular file there (§ 10.2 rule 4).
    MaskPathOccupied { path: String },
    /// A unit inspectah generates has a collected unit's name, so the
    /// stager withholds its files (§ 10.2, "What the plan takes" item 3).
    /// `name` is the collected unit's, the name of whichever generated file
    /// collides (the paired service of a generated timer, say), and
    /// `reserved_by` where that reservation came from, which decides the
    /// advice. Rendered as a FIXME, not an `# Omitted:` line.
    GeneratedNameCollision {
        name: UnitName,
        reserved_by: Reservation,
    },
    /// A template instance's configuration is not in the export as the
    /// source host had it (§ 10.2 rule 6, § 10.8). `path` is the
    /// configuration path, or the file under it that falls short.
    InstanceConfigNotStaged { path: String, gap: ConfigGap },
    /// The command would also act on `member` (§ 10.2 rule 7). An
    /// advisory ([`also_advisory_lines`]). `member` is the first member in
    /// `also` order that fails the first failing condition, or for
    /// [`AlsoWhy::InstallFile`] on the unit's own file and for
    /// [`AlsoWhy::FleetAggregate`], its first member. The other fields are
    /// what the advisory's text needs: the command's `verb`; `named`, the
    /// units the first line names first (the members that fail as `member`
    /// does, the unit whose install file it is, or every member in a fleet
    /// aggregate); `further`, the members failing the same condition the
    /// other way it can fail (each condition fails at most two ways: under
    /// `enable`, `Unlinked` or `Other`; under `disable`, `Installed` or
    /// `Other`; `RowNotCarried` or `NeedsOwnRow`), with that reason; and
    /// `add`, the members given a line of the opposite verb.
    AlsoMember {
        member: UnitName,
        why: AlsoWhy,
        verb: UnitAction,
        named: Vec<UnitName>,
        further: Option<Box<(AlsoWhy, Vec<UnitName>)>>,
        add: Vec<UnitName>,
    },
}

/// What falls short about an instance's configuration (§ 10.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigGap {
    /// The entry is excluded, or the stager leaves it out or withholds it.
    Excluded,
    /// The entry's content was not captured.
    NotCaptured,
    /// The redaction pass replaced or removed a value in it.
    Redacted,
    /// The snapshot holds nothing at the path.
    NotInSnapshot,
    /// A fleet aggregate's config entries at the path hold more than one
    /// content variant, so the staged copy may not be the one an instance
    /// reads ([`holds_variants`]).
    FleetVariants,
}

/// Why rule 7 of § 10.2 makes a command an advisory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlsoWhy {
    /// The member's links on the source are not what the verb produces.
    LinksDiffer(MemberLinks),
    /// The member has a row the plan does not carry with this verb.
    RowNotCarried,
    /// The member has no row and is an instance or a protected unit.
    NeedsOwnRow,
    /// The unit or the member has an install file in the snapshot.
    InstallFile { path: String },
    /// The export is a fleet aggregate.
    FleetAggregate,
}

impl OmissionReason {
    /// The text after the unit name in the `# Omitted:` line, and refine's
    /// subtitle for the row: the one place it is written. Package names
    /// come from the snapshot unchecked, so the Containerfile escapes the
    /// finished line, not this text.
    pub fn describe(&self) -> String {
        match self {
            Self::PackageExcluded { package } => format!("package '{package}' was excluded"),
            Self::NotInTargetImage { package } => {
                format!("package '{package}' not in target image")
            }
            Self::NotCarried { package } => {
                format!("package '{package}' is not carried: {NOT_CARRIED_REASON}")
            }
            Self::UnitFileNotStaged => "its unit file is not in the export".to_owned(),
            Self::InstallEvidenceChanged { path } => format!(
                "the [Install] section in the image would not be the one recorded on the \
                 source host ({path})"
            ),
            Self::StorageFileStaged { path } => {
                format!("the export carries an administrator's unit file for it ({path})")
            }
            Self::TargetNeedsUnit => TARGET_NEEDS_UNIT.to_owned(),
            Self::TimerTriggerEvidenceChanged { path } => format!(
                "the drop-in {path} that sets its Unit= is not in the export, so the unit it \
                 would start is not the one recorded"
            ),
            Self::TimerTriggerUnresolved => "its Unit= could not be resolved to a unit name, so \
                 inspectah cannot tell whether that unit is in the image"
                .to_owned(),
            Self::TimerTriggerNotStaged { trigger } => format!(
                "its unit {} is not in the export, and a timer whose unit is missing does not \
                 start",
                trigger.as_str()
            ),
            Self::MaskPathOccupied { path } => {
                format!("the export also carries a unit file at {path}; exclude one of the two")
            }
            Self::GeneratedNameCollision {
                name,
                reserved_by: Reservation::Row,
            } => format!(
                "inspectah generates this unit, and a unit collected from the host, {name}, has \
                 the name of one of its files, so the generated files are not written and the \
                 unit is not enabled here; to carry the generated unit, exclude the collected \
                 unit and its file",
                name = name.as_str()
            ),
            Self::InstanceConfigNotStaged { path, gap } => match gap {
                ConfigGap::Excluded => {
                    format!("its configuration {path} is excluded from the export")
                }
                ConfigGap::NotCaptured => format!("its configuration {path} was not captured"),
                ConfigGap::Redacted => format!(
                    "its configuration {path} holds a redacted value; provision the secret on the \
                     deployed host and enable the unit there"
                ),
                ConfigGap::NotInSnapshot => {
                    format!("its configuration {path} is not in the export")
                }
                ConfigGap::FleetVariants => format!(
                    "its configuration {path} differs between hosts in the fleet, so the staged \
                     copy may not be the one this instance reads"
                ),
            },
            Self::GeneratedNameCollision {
                name,
                reserved_by: Reservation::Inventory,
            } => format!(
                "inspectah generates this unit, and {name}, a unit the host already has, has the \
                 name of one of its files, so the generated files are not written and the unit \
                 is not enabled here; {name} has no row to exclude, so to carry the job, write \
                 its timer and its service by hand under other names, keeping the timer's Unit= \
                 or their shared name consistent",
                name = name.as_str()
            ),
            Self::AlsoMember {
                why,
                verb,
                named,
                further,
                ..
            } => also_reason_text(why, *verb, named, further.as_deref()),
        }
    }

    /// The variant's name, as refine's view names the reason (§ 11.1).
    /// The one place it is spelled: the match is exhaustive, so a new
    /// variant cannot go unnamed.
    pub fn name(&self) -> &'static str {
        match self {
            Self::PackageExcluded { .. } => "PackageExcluded",
            Self::NotInTargetImage { .. } => "NotInTargetImage",
            Self::NotCarried { .. } => "NotCarried",
            Self::UnitFileNotStaged => "UnitFileNotStaged",
            Self::InstallEvidenceChanged { .. } => "InstallEvidenceChanged",
            Self::StorageFileStaged { .. } => "StorageFileStaged",
            Self::TargetNeedsUnit => "TargetNeedsUnit",
            Self::TimerTriggerEvidenceChanged { .. } => "TimerTriggerEvidenceChanged",
            Self::TimerTriggerUnresolved => "TimerTriggerUnresolved",
            Self::TimerTriggerNotStaged { .. } => "TimerTriggerNotStaged",
            Self::MaskPathOccupied { .. } => "MaskPathOccupied",
            Self::GeneratedNameCollision { .. } => "GeneratedNameCollision",
            Self::InstanceConfigNotStaged { .. } => "InstanceConfigNotStaged",
            Self::AlsoMember { .. } => "AlsoMember",
        }
    }

    /// The owning package a presence reason names, `None` for a reason
    /// about the export's files.
    pub fn owning_package(&self) -> Option<&str> {
        match self {
            Self::PackageExcluded { package }
            | Self::NotInTargetImage { package }
            | Self::NotCarried { package } => Some(package),
            Self::UnitFileNotStaged
            | Self::InstallEvidenceChanged { .. }
            | Self::StorageFileStaged { .. }
            | Self::TargetNeedsUnit
            | Self::TimerTriggerEvidenceChanged { .. }
            | Self::TimerTriggerUnresolved
            | Self::TimerTriggerNotStaged { .. }
            | Self::MaskPathOccupied { .. }
            | Self::GeneratedNameCollision { .. }
            | Self::InstanceConfigNotStaged { .. }
            | Self::AlsoMember { .. } => None,
        }
    }
}

/// A unit that IS acted on but carries supplemental context about why
/// the plan could not fully validate its package's presence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceAdvisory {
    pub unit: UnitName,
    pub owning_package: String,
    pub reasons: Vec<AdvisoryReason>,
}

/// Why a unit received an advisory annotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdvisoryReason {
    /// The owning package is in `packages_added` with `include: true` but
    /// is not installable (LocalInstall / NoRepo / missing source_repo).
    PackageUnreachable,
    /// No baseline was available, so we cannot prove the package is absent.
    BaselineUnavailable,
}

/// A unit and the one command the plan writes for it.
///
/// The fields are private and there is no constructor outside this module:
/// [`plan_units`] is the only way to get one, so [`systemctl_lines`], which
/// takes nothing else, cannot name a unit the plan did not pass.
///
/// The two `compile_fail` blocks below pass on any compile error, since
/// rustdoc on stable does not check the error code, and a struct literal
/// cannot see a public constructor added later; the source check
/// `planned_unit_has_no_public_field_or_constructor` in
/// `tests/unit_plan_test.rs` pins both.
///
/// ```compile_fail,E0451
/// use inspectah_core::types::units::{UnitAction, UnitName};
/// use inspectah_pipeline::render::unit_plan::PlannedUnit;
///
/// let name = UnitName::try_from("sshd.service".to_string()).unwrap();
/// let _ = PlannedUnit { name, action: UnitAction::Enable };
/// ```
///
/// ```compile_fail,E0308
/// use inspectah_core::types::units::UnitName;
/// use inspectah_pipeline::render::unit_plan::systemctl_lines;
///
/// let name = UnitName::try_from("sshd.service".to_string()).unwrap();
/// let _ = systemctl_lines(None, &[name]);
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct PlannedUnit {
    name: UnitName,
    action: UnitAction,
}

impl PlannedUnit {
    /// The unit the command acts on.
    pub fn name(&self) -> &UnitName {
        &self.name
    }

    /// The command.
    pub fn action(&self) -> UnitAction {
        self.action
    }
}

/// The default target the plan sets (§ 10.5). Built only by
/// [`plan_units`], after rule 5, so [`systemctl_lines`] writes
/// `set-default` only for a target that passed it.
#[derive(Debug, PartialEq, Eq)]
pub struct PlannedTarget {
    name: TargetUnitName,
}

impl PlannedTarget {
    /// The target `set-default` names.
    pub fn name(&self) -> &TargetUnitName {
        &self.name
    }
}

/// What [`plan_units`] decided for every included state change.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct UnitPlan {
    /// The commands the Containerfile writes.
    pub planned: Vec<PlannedUnit>,
    /// The included default target, when it passed rule 5 (§ 10.5).
    pub default_target: Option<PlannedTarget>,
    /// The included default target, when rule 5 declined it, and why.
    /// Written as an `# Omitted:` line ([`default_target_line`]).
    pub default_target_declined: Option<(TargetUnitName, OmissionReason)>,
    /// Every command the plan did not write, and why.
    pub declined: Vec<(UnitName, UnitAction, OmissionReason)>,
    /// Planned units whose package's presence could not be proved.
    pub advisories: Vec<ServiceAdvisory>,
    /// The attention notes of carried instance enables (§ 10.8): a
    /// configuration path staged from a scan with no redaction, or holding
    /// a value the redaction pass flagged and did not change. Not a
    /// Containerfile line: refine shows each on its row.
    pub instance_notes: Vec<(UnitName, String)>,
    /// § 10.8 "Data under `/var`": one sentence per carried instance
    /// enable whose `data_paths` is not empty ([`data_advisory`]). Written
    /// as an `# Advisory:` line before the `RUN` lines, and refine shows
    /// the same sentence on the row.
    pub data_advisories: Vec<(UnitName, String)>,
    /// The enables of the units the scheduled section generates from cron
    /// entries, which that section copies and enables (§ 10.2 item 3,
    /// § 10.7).
    pub scheduled: Vec<PlannedUnit>,
    /// The enable of `flatpak-provision.service`, which the container
    /// workloads section copies and enables.
    pub workloads: Vec<PlannedUnit>,
    /// Every generated unit's request the plan did not write, and why. Each
    /// section writes its own as a FIXME or an `# Omitted:` line.
    pub generated_declined: Vec<GeneratedDecline>,
    /// Excluded rows of runtime package updaters (§ 10.4). Not planned, as
    /// no excluded row is, and not declined: the row's exclusion decided
    /// it. The section names each in [`excluded_updater_line`].
    pub excluded_updaters: Vec<UnitName>,
    /// § 10.6's comment lines, one block per mount finding
    /// ([`mount_advisory_lines`]), unescaped. Not commands: the section
    /// writes them after its `systemctl` lines (§ 10.1).
    pub mount_advisories: Vec<Vec<String>>,
    /// § 10.9's comment lines, one per hand-made link
    /// ([`manual_link_lines`]), in the section's order.
    pub link_advisories: Vec<FindingAdvisory>,
    /// § 10.9's unit-level block, one per undetermined install record
    /// ([`undetermined_install_lines`]), in the section's order.
    pub undetermined_advisories: Vec<FindingAdvisory>,
}

/// What § 10.9 says for one hand-made link or undetermined install record,
/// unescaped.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FindingAdvisory {
    /// The record's block, or, while `not_applied` holds a path, its
    /// unit's `# Omitted:` line ([`install_omitted_line`]) in its place.
    pub lines: Vec<String>,
    /// The install files of the record's unit the export does not stage
    /// (§ 7.4). Empty while the advisory applies.
    pub not_applied: Vec<String>,
}

/// What asked for a generated unit's enable: the row it shows on (§ 11.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneratedSource {
    /// A unit the scheduled section generates from a cron entry; `name` is
    /// its `generated_timer_units` row's name.
    Cron { name: String },
    /// The flatpak provisioning service, for the included flatpak apps.
    FlatpakProvisioning,
}

/// A generated unit's request the plan did not write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedDecline {
    pub source: GeneratedSource,
    pub unit: UnitName,
    pub reason: OmissionReason,
}

/// The unit `name.suffix` a generated request enables, when that name is
/// one the stager writes ([`staged_unit_file`]), a [`UnitName`], and an
/// operand inspectah writes. `None` otherwise: the scheduled section names
/// the refusal in a FIXME.
pub(crate) fn generated_unit_name(name: &str, suffix: &str) -> Option<UnitName> {
    let file = staged_unit_file(name, suffix)?;
    UnitName::try_from(file)
        .ok()
        .filter(|unit| operand_refusal(unit.as_str()).is_none())
}

/// Whether a generated unit whose files are `files` may be enabled: every
/// file is staged. [`OmissionReason::GeneratedNameCollision`], naming the
/// collected unit the stager gave, when it withheld one for a collision,
/// and [`OmissionReason::UnitFileNotStaged`] when one is not staged for
/// any other reason.
fn generated_request(files: &[String], staged: &StagedWrites<'_>) -> Result<(), OmissionReason> {
    let collision = files
        .iter()
        .filter_map(|path| Destination::new(path).ok())
        .find_map(|dest| {
            staged.withheld.iter().find_map(|(at, _, why)| match why {
                WithheldWhy::GeneratedNameCollision { unit, reserved_by } if *at == dest => {
                    Some(OmissionReason::GeneratedNameCollision {
                        name: unit.clone(),
                        reserved_by: *reserved_by,
                    })
                }
                _ => None,
            })
        });
    if let Some(reason) = collision {
        return Err(reason);
    }
    if !files.iter().all(|path| is_staged(staged, path)) {
        return Err(OmissionReason::UnitFileNotStaged);
    }
    Ok(())
}

/// [`FLATPAK_SERVICE`] as a [`UnitName`]. The constant is a literal that
/// parses, which `the_flatpak_service_constant_is_a_unit_name` checks, so
/// the parse cannot fail at run time.
fn flatpak_service_unit() -> UnitName {
    UnitName::try_from(FLATPAK_SERVICE.to_owned()).expect("FLATPAK_SERVICE is a unit name literal")
}

/// § 10.2 "What the plan takes" item 3: the enables of the units
/// inspectah generates. A cron unit enables its timer, whose paired
/// service must be staged too, or for `@reboot` its service. Two cron
/// units of one name are one request.
fn plan_generated(snap: &InspectionSnapshot, staged: &StagedWrites<'_>, plan: &mut UnitPlan) {
    if let Some(st) = &snap.scheduled_tasks {
        let mut seen = std::collections::BTreeSet::new();
        for u in st
            .generated_timer_units
            .iter()
            .filter(|u| u.disposition.is_included() && !u.name.is_empty())
        {
            let reboot = u.cron_expr == "@reboot";
            let suffix = if reboot { "service" } else { "timer" };
            let Some(unit) = generated_unit_name(&u.name, suffix) else {
                continue;
            };
            if !seen.insert(unit.clone()) {
                continue;
            }
            let mut files = vec![etc_unit_path(&unit)];
            if !reboot && !u.service_content.is_empty() {
                files.push(format!("{ETC_UNIT_DIR}/{}.service", u.name));
            }
            match generated_request(&files, staged) {
                Ok(()) => plan.scheduled.push(PlannedUnit {
                    name: unit,
                    action: UnitAction::Enable,
                }),
                Err(reason) => plan.generated_declined.push(GeneratedDecline {
                    source: GeneratedSource::Cron {
                        name: u.name.clone(),
                    },
                    unit,
                    reason,
                }),
            }
        }
    }
    let flatpak_included = snap.containers.as_ref().is_some_and(|c| {
        c.flatpak_apps
            .iter()
            .any(|app| app.disposition.is_included())
    });
    if flatpak_included {
        let unit = flatpak_service_unit();
        let files = [etc_unit_path(&unit)];
        match generated_request(&files, staged) {
            Ok(()) => plan.workloads.push(PlannedUnit {
                name: unit,
                action: UnitAction::Enable,
            }),
            Err(reason) => plan.generated_declined.push(GeneratedDecline {
                source: GeneratedSource::FlatpakProvisioning,
                unit,
                reason,
            }),
        }
    }
}

/// Internal classification of whether a unit's package is in the image.
enum PresenceDecision {
    /// Decline: the owning package is absent from the target image.
    Omit(OmissionReason),
    /// Keep the unit; optionally attach advisory reasons.
    Emit {
        advisory_reasons: Option<(String, Vec<AdvisoryReason>)>,
    },
}

/// `RUN systemctl set-default --` for `default_target` on one line, then
/// one `RUN systemctl <verb> --` per verb, enable then disable then mask,
/// each unit single-quoted on its own continuation line, ordered by kind
/// and then by name (§ 10.1). Takes only a [`PlannedTarget`] and
/// [`PlannedUnit`]s, which only [`plan_units`] builds.
///
/// [`single_quote`] is total on a [`UnitName`]: its character set holds no
/// quote and no line break. `--` ends the options, so a name that starts
/// with `-` (`-.slice`) is an operand.
pub fn systemctl_lines(
    default_target: Option<&PlannedTarget>,
    units: &[PlannedUnit],
) -> Vec<String> {
    let mut lines: Vec<String> = default_target
        .map(|target| {
            format!(
                "RUN systemctl set-default -- {}",
                single_quote(target.name.as_unit_name().as_str())
            )
        })
        .into_iter()
        .collect();
    for action in [UnitAction::Enable, UnitAction::Disable, UnitAction::Mask] {
        let mut names: Vec<&UnitName> = units
            .iter()
            .filter(|u| u.action == action)
            .map(|u| &u.name)
            .collect();
        if names.is_empty() {
            continue;
        }
        names.sort_by(|a, b| (a.kind(), a).cmp(&(b.kind(), b)));
        lines.push(format!("RUN systemctl {action} -- \\"));
        let last = names.len() - 1;
        for (i, name) in names.into_iter().enumerate() {
            let operand = single_quote(name.as_str());
            lines.push(if i < last {
                format!("    {operand} \\")
            } else {
                format!("    {operand}")
            });
        }
    }
    lines
}

/// How the Containerfile marks a declined command's line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclinedMarker {
    /// `# Omitted:`, for a command the export cannot carry as it stands.
    Omitted,
    /// `# FIXME:`, for a command the target refuses (§ 10.3).
    Fixme,
    /// `# Advisory:`, the first line of rule 7's block (§ 10.2), for a
    /// command whose `Also=` members the evidence does not match.
    Advisory,
}

impl DeclinedMarker {
    /// The comment prefix, with its trailing space.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Omitted => "# Omitted: ",
            Self::Fixme => "# FIXME: ",
            Self::Advisory => "# Advisory: ",
        }
    }
}

/// A declined command's line: its marker and the text after the marker.
/// The text is not escaped: package names in it come from the snapshot, so
/// each surface escapes the finished line for itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclinedLine {
    pub marker: DeclinedMarker,
    pub text: String,
}

impl DeclinedLine {
    /// The Containerfile comment, before escaping.
    pub fn comment(&self) -> String {
        format!("{}{}", self.marker.prefix(), self.text)
    }
}

/// The line for one entry of [`UnitPlan::declined`]: the one place the
/// marker and the text are chosen, for the Containerfile and refine alike.
/// [`OmissionReason::TargetNeedsUnit`] is a FIXME with § 10.3's sentence
/// ([`target_needs_unit_text`]); [`OmissionReason::GeneratedNameCollision`]
/// is a FIXME naming the unit, then its text in parentheses;
/// [`OmissionReason::AlsoMember`] is the first line of rule 7's block
/// ([`also_advisory_lines`]); every other reason is an `# Omitted:` line in
/// that form.
pub fn declined_line(name: &UnitName, action: UnitAction, reason: &OmissionReason) -> DeclinedLine {
    match reason {
        OmissionReason::AlsoMember { why, .. } => DeclinedLine {
            marker: DeclinedMarker::Advisory,
            text: format!(
                "{} is {} {} {}",
                name.as_str(),
                state_word(action),
                match why {
                    AlsoWhy::FleetAggregate => ALSO_NOT_CARRIED_FLEET,
                    _ => ALSO_NOT_CARRIED,
                },
                reason.describe()
            ),
        },
        OmissionReason::TargetNeedsUnit => DeclinedLine {
            marker: DeclinedMarker::Fixme,
            text: target_needs_unit_text(name.as_str(), action),
        },
        OmissionReason::GeneratedNameCollision { .. } => DeclinedLine {
            marker: DeclinedMarker::Fixme,
            text: format!("{} ({})", name.as_str(), reason.describe()),
        },
        _ => omitted_line(name, reason),
    }
}

/// An `# Omitted:` line: the unit, then the reason's text in parentheses.
fn omitted_line(name: &UnitName, reason: &OmissionReason) -> DeclinedLine {
    DeclinedLine {
        marker: DeclinedMarker::Omitted,
        text: format!("{} ({})", name.as_str(), reason.describe()),
    }
}

/// The line for [`UnitPlan::default_target_declined`]. Rule 5 is the only
/// rule a default target meets, and each of its reasons is an
/// `# Omitted:` line.
pub fn default_target_line(target: &TargetUnitName, reason: &OmissionReason) -> DeclinedLine {
    omitted_line(target.as_unit_name(), reason)
}

/// § 10.4's reason for an excluded runtime package updater, after its name
/// in the `# Omitted:` line.
const RUNTIME_UPDATER_OMITTED: &str =
    "updates packages in place, which an image-based host does not support; excluded by default";

/// The `# Omitted:` line for an excluded runtime package updater (§ 10.4),
/// one entry of [`UnitPlan::excluded_updaters`].
pub fn excluded_updater_line(name: &UnitName) -> DeclinedLine {
    DeclinedLine {
        marker: DeclinedMarker::Omitted,
        text: format!("{} ({RUNTIME_UPDATER_OMITTED})", name.as_str()),
    }
}

/// The unit state section's lines for `plan`, in § 10.1's order: a line per
/// declined command ([`declined_line`]) other than rule 7's, a line per
/// excluded runtime updater ([`excluded_updater_line`]), a data advisory
/// per carried instance with data under `/var` ([`data_advisory`]), then
/// [`systemctl_lines`], then the comment-only blocks one after another:
/// rule 7's ([`also_advisory_lines`]), the mount advisories
/// ([`UnitPlan::mount_advisories`]), the hand-made link advisories
/// ([`UnitPlan::link_advisories`]) and the undetermined install blocks
/// ([`UnitPlan::undetermined_advisories`]). Each block starts with its `# Advisory:` line, so
/// no line separates them. A unit's `# Omitted:` line, which stands in for
/// every block of the unit whose install files are not all staged, is
/// written once, where its first block would be (§ 10.9).
pub fn unit_state_lines(plan: &UnitPlan) -> Vec<String> {
    // The reason may name a package from the snapshot, so the finished
    // line is escaped here rather than inside `declined_line`, which refine
    // also calls and escapes for its own surface.
    let mut lines: Vec<String> = plan
        .declined
        .iter()
        .filter(|(_, _, reason)| !matches!(reason, OmissionReason::AlsoMember { .. }))
        .map(|(name, action, reason)| {
            escape_comment_text(&declined_line(name, *action, reason).comment())
        })
        .collect();
    lines.extend(plan.default_target_declined.iter().map(|(target, reason)| {
        escape_comment_text(&default_target_line(target, reason).comment())
    }));
    lines.extend(
        plan.excluded_updaters
            .iter()
            .map(|name| escape_comment_text(&excluded_updater_line(name).comment())),
    );
    // § 10.8: a carried instance's data advisory precedes the `RUN` lines.
    // Its paths come from the snapshot, so each line is escaped.
    lines.extend(plan.data_advisories.iter().map(|(_, sentence)| {
        escape_comment_text(&format!("{}{sentence}", DeclinedMarker::Advisory.prefix()))
    }));
    lines.extend(systemctl_lines(plan.default_target.as_ref(), &plan.planned));
    // Rule 7's blocks (§ 10.1's order). An install file's path comes from
    // the snapshot, so each line is escaped.
    lines.extend(
        plan.declined
            .iter()
            .flat_map(|(name, action, reason)| also_advisory_lines(name, *action, reason))
            .map(|line| escape_comment_text(&line)),
    );
    // A mount point comes from a host file, and an install file's path
    // from a directory listing: escaped here, as above.
    lines.extend(
        plan.mount_advisories
            .iter()
            .flatten()
            .map(|line| escape_comment_text(line)),
    );
    let mut omitted = std::collections::HashSet::new();
    for advisory in plan
        .link_advisories
        .iter()
        .chain(&plan.undetermined_advisories)
    {
        if !advisory.not_applied.is_empty() && !omitted.insert(&advisory.lines) {
            continue;
        }
        lines.extend(advisory.lines.iter().map(|line| escape_comment_text(line)));
    }
    lines
}

// ---------------------------------------------------------------------------
// § 10.2 rule 7, the `Also=` advisory
// ---------------------------------------------------------------------------

/// Rule 7's first line, between the unit's state and the reason.
const ALSO_NOT_CARRIED: &str = "on the source host. Not carried:";

/// [`ALSO_NOT_CARRIED`] for a fleet aggregate, whose rows come from
/// several hosts.
const ALSO_NOT_CARRIED_FLEET: &str = "on the source hosts. Not carried:";

/// After the members a rule 7 first line names.
const ALSO_NAMED_BY: &str = "(named by Also=)";

/// The clause for members not enabled under an `enable`: one, then more.
const ALSO_NOT_ENABLED: [&str; 2] = ["which is not enabled there", "which are not enabled there"];

/// The clause for members enabled under a `disable`: one, then more.
const ALSO_ENABLED: [&str; 2] = ["which is enabled there", "which are enabled there"];

/// The clause for members whose links neither verb reproduces.
const ALSO_NEITHER: &str = "whose links there neither command reproduces";

/// The clause for members whose own rows the plan does not carry with the
/// command: one, then more.
const ALSO_ROW_NOT_CARRIED: [&str; 2] = [
    "whose own row is not carried with this command",
    "whose own rows are not carried with this command",
];

/// The clause for instance members with no row under an `enable`: one,
/// then more.
const ALSO_INSTANCE_NEEDS_ROW: [&str; 2] = [
    "an instance with no row here to check its configuration",
    "instances with no row here to check their configuration",
];

/// The clause for protected members with no row under a `disable`.
const ALSO_IMAGE_NEEDS: &str = "which the image needs";

/// An `InstallFile` reason, between the file and the unit it belongs to.
const ALSO_INSTALL_FILE_SETS: &str = "sets [Install] for";

/// An `InstallFile` reason, after the unit, before the verb.
const ALSO_INSTALL_FILE_UNITS: &str = "so the units systemctl";

/// An `InstallFile` reason, after the verb.
const ALSO_INSTALL_FILE_REST: &str =
    "reaches through Also= in the image may not be the ones recorded.";

/// The header of the lines to add for a links mismatch.
const ALSO_TO_REPRODUCE: &str =
    "#   To reproduce the source host, add after the configuration COPY:";

/// The footer of the lines to add for a links mismatch.
const ALSO_EACH_FOLLOWS: &str =
    "#   Each command follows Also= in its turn. Check the result with systemctl is-enabled.";

/// The header of the lines to add for a links mismatch they cannot
/// reproduce: a member is `Other`, or a line is withheld. It takes the
/// place of [`ALSO_TO_REPRODUCE`], and no footer follows (Mark Russell,
/// 2026-11-15).
const ALSO_PARTIAL: &str = "#   These lines do not reproduce the source host. Check each unit, \
     then add after the configuration COPY:";

/// The header of an `InstallFile` advisory's one line to add.
const ALSO_AFTER_READING: &str = "#   After reading that file, add after the configuration COPY:";

/// The clause of a `FleetAggregate` advisory, after the members.
const ALSO_FLEET_EVIDENCE: &str = "and fleet evidence may omit install metadata";

/// The header of a `FleetAggregate` advisory's one line to add, before
/// the unit.
const ALSO_FLEET_CHECK: &str = "#   Before adding this after the configuration COPY, check the \
     effective [Install] and Also= of";

/// The header of a `FleetAggregate` advisory's one line to add, after the
/// unit.
const ALSO_FLEET_CHECK_REST: &str = "in the image, and the intended state of each member:";

/// The state a command reproduces: `enabled` for `enable`.
fn state_word(action: UnitAction) -> &'static str {
    match action {
        UnitAction::Enable => "enabled",
        UnitAction::Disable => "disabled",
        UnitAction::Mask => "masked",
    }
}

/// `names` as a list: `a`, `a and b`, `a, b and c`.
fn name_list(names: &[UnitName]) -> String {
    match names {
        [] => String::new(),
        [one] => one.as_str().to_owned(),
        [rest @ .., last] => format!(
            "{} and {}",
            rest.iter()
                .map(UnitName::as_str)
                .collect::<Vec<_>>()
                .join(", "),
            last.as_str()
        ),
    }
}

/// The singular or the plural of a clause, by how many members it names.
fn number(clause: [&'static str; 2], named: &[UnitName]) -> &'static str {
    if named.len() > 1 {
        clause[1]
    } else {
        clause[0]
    }
}

/// [`OmissionReason::AlsoMember`]'s text: rule 7's first line after
/// `Not carried: ` (§ 10.2). `further` adds its members and their clause
/// after the first group's.
fn also_reason_text(
    why: &AlsoWhy,
    verb: UnitAction,
    named: &[UnitName],
    further: Option<&(AlsoWhy, Vec<UnitName>)>,
) -> String {
    if let AlsoWhy::InstallFile { path } = why {
        return format!(
            "{path} {ALSO_INSTALL_FILE_SETS} {}, {ALSO_INSTALL_FILE_UNITS} {verb} \
             {ALSO_INSTALL_FILE_REST}",
            name_list(named)
        );
    }
    let mut text = format!(
        "systemctl {verb} on it also {verb}s {} {ALSO_NAMED_BY}, {}",
        name_list(named),
        also_clause(why, verb, named)
    );
    if let Some((why, names)) = further {
        text.push_str(&format!(
            ", and {}, {}",
            name_list(names),
            also_clause(why, verb, names)
        ));
    }
    text.push('.');
    text
}

/// The clause after the members a rule 7 first line names for `why`.
/// Conditions 1 and 2 give clause groups; an `InstallFile` reason is
/// written whole by [`also_reason_text`] and names no members, so its arm
/// gives the end of that sentence.
fn also_clause(why: &AlsoWhy, verb: UnitAction, named: &[UnitName]) -> &'static str {
    match why {
        AlsoWhy::InstallFile { .. } => ALSO_INSTALL_FILE_REST,
        AlsoWhy::LinksDiffer(MemberLinks::Unlinked) => number(ALSO_NOT_ENABLED, named),
        AlsoWhy::LinksDiffer(MemberLinks::Installed) => number(ALSO_ENABLED, named),
        AlsoWhy::LinksDiffer(MemberLinks::Other | MemberLinks::Inert) => ALSO_NEITHER,
        AlsoWhy::RowNotCarried => number(ALSO_ROW_NOT_CARRIED, named),
        AlsoWhy::NeedsOwnRow => match verb {
            UnitAction::Enable => number(ALSO_INSTANCE_NEEDS_ROW, named),
            UnitAction::Disable | UnitAction::Mask => ALSO_IMAGE_NEEDS,
        },
        AlsoWhy::FleetAggregate => ALSO_FLEET_EVIDENCE,
    }
}

/// One line to add: `RUN systemctl <verb> -- '<unit>'`, as a comment.
fn line_to_add(verb: UnitAction, unit: &UnitName) -> String {
    format!(
        "#     RUN systemctl {verb} -- {}",
        single_quote(unit.as_str())
    )
}

/// Rule 7's block for a declined command (§ 10.2), unescaped; empty for
/// any other reason. The first line is [`declined_line`]'s. A links
/// mismatch adds the unit's own command and the opposite verb for each
/// member in `add`, when there is any such member, except a verb § 10.3
/// refuses for that member ([`target_needs_unit`]): that member gets a line
/// saying the image needs it instead. The lines sit between a header and a
/// footer that say they reproduce the source host, unless a member is
/// `Other` or a line is withheld: then [`ALSO_PARTIAL`] heads them and no
/// footer follows. An `InstallFile` advisory adds the unit's own command,
/// and a `FleetAggregate` advisory adds it after what to check first; the
/// other reasons add no line.
pub fn also_advisory_lines(
    name: &UnitName,
    action: UnitAction,
    reason: &OmissionReason,
) -> Vec<String> {
    let OmissionReason::AlsoMember {
        why, further, add, ..
    } = reason
    else {
        return Vec::new();
    };
    let mut lines = vec![declined_line(name, action, reason).comment()];
    match why {
        AlsoWhy::LinksDiffer(_) if !add.is_empty() => {
            let opposite = match action {
                UnitAction::Enable => UnitAction::Disable,
                UnitAction::Disable | UnitAction::Mask => UnitAction::Enable,
            };
            let (withheld, suggested): (Vec<&UnitName>, Vec<&UnitName>) = add
                .iter()
                .partition(|member| target_needs_unit(member.as_str(), opposite));
            let other = AlsoWhy::LinksDiffer(MemberLinks::Other);
            let reproduces = withheld.is_empty()
                && *why != other
                && further.as_deref().is_none_or(|(why, _)| *why != other);
            lines.push(
                if reproduces {
                    ALSO_TO_REPRODUCE
                } else {
                    ALSO_PARTIAL
                }
                .to_owned(),
            );
            lines.push(line_to_add(action, name));
            lines.extend(suggested.iter().map(|member| line_to_add(opposite, member)));
            lines.extend(withheld.iter().map(|member| {
                format!(
                    "#   No {opposite} line for {}: {TARGET_NEEDS_UNIT}.",
                    member.as_str()
                )
            }));
            if reproduces {
                lines.push(ALSO_EACH_FOLLOWS.to_owned());
            }
        }
        AlsoWhy::InstallFile { .. } => {
            lines.push(ALSO_AFTER_READING.to_owned());
            lines.push(line_to_add(action, name));
        }
        AlsoWhy::FleetAggregate => {
            lines.push(format!(
                "{ALSO_FLEET_CHECK} {} {ALSO_FLEET_CHECK_REST}",
                name.as_str()
            ));
            lines.push(line_to_add(action, name));
        }
        AlsoWhy::LinksDiffer(_) | AlsoWhy::RowNotCarried | AlsoWhy::NeedsOwnRow => {}
    }
    lines
}

// ---------------------------------------------------------------------------
// § 10.9 hand-made link advisories
// ---------------------------------------------------------------------------

/// § 10.9's `StaticUnit` text after the link: it does not say who made
/// the link, which may be a hand or an `enable` before the section changed.
const LINK_STATIC_UNIT: &str = "[Install] section there does not make (it names no link: \
     none, only Also=, or a list a drop-in reset to empty). Not carried.";

/// § 10.9's `PartialInstall` text after the unit's name, before the link.
const LINK_PARTIAL_INSTALL: &str = "has some of the links its [Install] section names and \
     not others, so neither enable nor disable reproduces the source host. Not carried. Present:";

/// § 10.9's start of the command line, before the verb.
const LINK_COMMAND_PREFIX: &str =
    "#   To carry it, add after the configuration COPY: RUN systemctl";

/// § 10.9's unit-level block, the second line's text after the unit name
/// (`The image's links for <unit>`).
const UNDETERMINED_CHECK: &str = "can differ from the source host's, including links its \
     package install made before these files are copied. Check them with systemctl is-enabled \
     in the image.";

/// § 10.9's sentence for an undetermined record with no install file.
const UNDETERMINED_ASSIGNS_ALSO: &str = "It assigns Also=, so the units it names are enabled \
     or disabled with it in the image, and inspectah could not list them to check.";

/// The sentence for an undetermined record with no install file whose
/// unit file could not be read as text: whether it assigns `Also=` is not
/// known (Mark Russell, 2026-11-15).
const UNDETERMINED_MAY_ASSIGN_ALSO: &str = "It may assign Also=, so units it names may be \
     enabled or disabled with it in the image, and inspectah could not read it to check.";

/// Why a section cannot be relied on, in the words § 10.9 gives each
/// [`InstallUnknown`] reason.
pub fn install_unknown_words(reason: InstallUnknown) -> &'static str {
    match reason {
        InstallUnknown::Specifier => "a specifier inspectah does not expand",
        InstallUnknown::RequiredByReset => "an empty RequiredBy= whose effect was not measured",
        InstallUnknown::ForeignName => "a name this model does not admit",
        InstallUnknown::UpheldBy => "enabling the unit would also make an .upholds link",
        InstallUnknown::Unreadable => "a file of it inspectah could not read as text",
    }
}

/// How § 10.9 names a link: `<parent>.<relation>/<unit>` for a link in a
/// dependency directory, and for an alias the words `<parent> is a link to
/// <unit>`, which take the place of the path and of the `is a link` after
/// it. Relation `Also` is not a link and has no name here
/// ([`also_lines`]).
enum LinkName<'a> {
    Path { path: String, dir: DependencyDir },
    Alias { alias: &'a str, unit: &'a str },
}

/// The dependency directory a link sits in.
#[derive(Clone, Copy)]
enum DependencyDir {
    Wants,
    Requires,
    Upholds,
}

impl<'a> LinkName<'a> {
    /// The name of `link`, or `None` for relation `Also`.
    fn of(link: &'a ManualLink) -> Option<Self> {
        let (parent, unit) = (link.parent.as_str(), link.unit.as_str());
        let path = |suffix: &str, dir| {
            Some(Self::Path {
                path: format!("{parent}.{suffix}/{unit}"),
                dir,
            })
        };
        match link.relation {
            LinkRelation::Wants => path("wants", DependencyDir::Wants),
            LinkRelation::Requires => path("requires", DependencyDir::Requires),
            LinkRelation::Upholds => path("upholds", DependencyDir::Upholds),
            LinkRelation::Alias => Some(Self::Alias {
                alias: parent,
                unit,
            }),
            LinkRelation::Also => None,
        }
    }

    /// The words that open a sentence saying the link is on the host:
    /// `<path> is a link`, `<alias> is a link to <unit>`.
    fn is_a_link(&self) -> String {
        match self {
            Self::Path { path, .. } => format!("{path} is a link"),
            Self::Alias { alias, unit } => format!("{alias} is a link to {unit}"),
        }
    }

    /// The link as the subject of a sentence that does not say it is a
    /// link.
    fn subject(&self) -> String {
        match self {
            Self::Path { path, .. } => path.clone(),
            Self::Alias { alias, unit } => format!("{alias}, a link to {unit},"),
        }
    }

    /// Where the link sits in the image: under `/etc/systemd/system`, by
    /// its path or its alias name.
    fn etc_path(&self) -> String {
        match self {
            Self::Path { path, .. } => format!("{ETC_UNIT_DIR}/{path}"),
            Self::Alias { alias, .. } => format!("{ETC_UNIT_DIR}/{alias}"),
        }
    }
}

/// § 10.9's `HostInstallOnly` text after the link and before the unit.
const LINK_HOST_INSTALL_ONLY: &str = "is on the source host because";

/// § 10.9's `HostInstallOnly` text after `<unit>'s`.
const LINK_HOST_INSTALL_ONLY_REST: &str = "files under /etc/systemd/system name it, and the \
     package's unit does not. The image was linked from the package's unit, so it lacks this \
     link. Not carried.";

/// § 10.9's `VendorInstallOnly` text after the link and before the unit.
const LINK_VENDOR_INSTALL_ONLY: &str = "is not on the source host:";

/// § 10.9's `VendorInstallOnly` text after `<unit>'s`.
const LINK_VENDOR_INSTALL_ONLY_REST: &str = "files under /etc/systemd/system do not name it, \
     and the package's unit does. The image was linked from the package's unit, so it has this \
     link.";

/// § 10.9's start of the removal line, before the quoted path. The line
/// tests that the path is a link before it removes it, so it never deletes
/// a file.
const LINK_REMOVE_PREFIX: &str = "#   To remove it, add after the configuration COPY: RUN test -L";

/// The line in place of the removal line when the export writes a file at
/// the link's path, before the path: the copied file replaces the link, and
/// advice never deletes a file the export carries.
const LINK_STAGED_FILE_PREFIX: &str = "#   The export copies a file to";

/// [`LINK_STAGED_FILE_PREFIX`]'s line after the path.
const LINK_STAGED_FILE_REST: &str = "which replaces this link in the image, so no line removes it.";

/// The comment lines of § 10.9 for one hand-made link, the one source for
/// the Containerfile and the view response. The lines are the spec's text
/// with the names filled in, `#` included, and are not escaped: each
/// surface escapes the finished line for itself.
///
/// - The first line by cause. For an alias, the link is named
///   `<alias> is a link to <unit>` in place of its path ([`LinkName`]).
/// - For the first four causes and `HostInstallOnly`, one more line by
///   relation: the `add-wants` or `add-requires` command, quoted after
///   `--`, for a `.wants` or `.requires` link; for an `.upholds` link, what
///   the link does and how to add it, since no verb makes one; for an
///   alias, how to add it, since no verb but `enable` makes one. A
///   `HostDevice` link gets no command.
/// - For `VendorInstallOnly`, the line that removes the link the image
///   has, its path quoted, after a test that the path is a link. When
///   `staged` writes a file at that path, the copied file replaces the
///   link, and a line says so in place of the removal line.
/// - Relation `Also` is not a link: [`also_lines`].
pub fn manual_link_lines(link: &ManualLink, staged: &StagedWrites<'_>) -> Vec<String> {
    let unit = link.unit.as_str();
    let parent = link.parent.as_str();
    let Some(name) = LinkName::of(link) else {
        return also_lines(link);
    };
    let first = match link.cause {
        ManualLinkCause::StaticUnit => format!(
            "# Advisory: {} on the source host that {unit}'s {LINK_STATIC_UNIT}",
            name.is_a_link()
        ),
        ManualLinkCause::NotInInstall => format!(
            "# Advisory: {} made by hand on the source host (the unit's [Install] section \
             does not name {parent}). Not carried.",
            name.is_a_link()
        ),
        ManualLinkCause::PartialInstall => {
            let present = match &name {
                LinkName::Path { path, .. } => path.clone(),
                LinkName::Alias { .. } => name.subject().trim_end_matches(',').to_owned(),
            };
            format!("# Advisory: {unit} {LINK_PARTIAL_INSTALL} {present}.")
        }
        ManualLinkCause::Undetermined { reason } => format!(
            "# Advisory: {} is not carried: the unit's [Install] section cannot be relied on \
             ({}).",
            name.subject(),
            install_unknown_words(reason)
        ),
        ManualLinkCause::HostDevice => {
            return vec![format!(
                "# Advisory: {unit} is tied to a device of the source host. Not carried."
            )];
        }
        ManualLinkCause::HostInstallOnly => format!(
            "# Advisory: {} {LINK_HOST_INSTALL_ONLY} {unit}'s {LINK_HOST_INSTALL_ONLY_REST}",
            name.subject()
        ),
        ManualLinkCause::VendorInstallOnly => {
            return vec![
                format!(
                    "# Advisory: {} {LINK_VENDOR_INSTALL_ONLY} {unit}'s \
                     {LINK_VENDOR_INSTALL_ONLY_REST}",
                    name.subject()
                ),
                {
                    let path = name.etc_path();
                    if is_staged(staged, &path) {
                        format!("{LINK_STAGED_FILE_PREFIX} {path}, {LINK_STAGED_FILE_REST}")
                    } else {
                        let path = single_quote(&path);
                        format!("{LINK_REMOVE_PREFIX} {path} && rm -- {path}")
                    }
                },
            ];
        }
    };
    let command = |verb: &str| {
        format!(
            "{LINK_COMMAND_PREFIX} {verb} -- {} {}",
            single_quote(parent),
            single_quote(unit)
        )
    };
    let second = match name {
        LinkName::Path {
            dir: DependencyDir::Wants,
            ..
        } => command("add-wants"),
        LinkName::Path {
            dir: DependencyDir::Requires,
            ..
        } => command("add-requires"),
        LinkName::Path {
            dir: DependencyDir::Upholds,
            ..
        } => format!(
            "#   An .upholds link starts {unit} while {parent} is active and restarts it \
             whenever it stops, so adding it for a unit that becomes inactive after it exits, \
             such as a oneshot service without RemainAfterExit=yes, produces a restart loop. A \
             RHEL 9 target does not honor it. To add it, put Upholds={unit} in a drop-in for \
             {parent}."
        ),
        LinkName::Alias { .. } => format!(
            "#   No systemctl verb but enable makes an alias link. To carry it, make the link \
             in the Containerfile after the configuration COPY, or add Alias={parent} in a \
             drop-in for {unit} and run systemctl enable on {unit} after that COPY, which also \
             makes every other link its [Install] section and Also= name: check those first."
        ),
    };
    vec![first, second]
}

/// § 10.9's text for relation `Also`, which is not a link and has no
/// command: which side names `<unit>` in `Also=`, that the package install
/// in the image acted on the package's list, and to check `<unit>` with
/// `systemctl is-enabled` in the image. Only the two install-difference
/// causes name a relation `Also` (§ 6.6); any other has no text.
fn also_lines(link: &ManualLink) -> Vec<String> {
    let (owner, unit) = (link.parent.as_str(), link.unit.as_str());
    let side = match link.cause {
        ManualLinkCause::HostInstallOnly => format!(
            "{owner}'s files under /etc/systemd/system name {unit} in Also=, and the package's \
             unit does not."
        ),
        ManualLinkCause::VendorInstallOnly => format!(
            "{owner}'s package unit names {unit} in Also=, and its files under \
             /etc/systemd/system do not."
        ),
        ManualLinkCause::StaticUnit
        | ManualLinkCause::NotInInstall
        | ManualLinkCause::PartialInstall
        | ManualLinkCause::Undetermined { .. }
        | ManualLinkCause::HostDevice => return Vec::new(),
    };
    vec![
        format!("# Advisory: {side} {ALSO_PACKAGE_LIST}"),
        format!("#   Check {unit} with systemctl is-enabled in the image."),
    ]
}

/// § 10.9's `Also` text after which side names the unit.
const ALSO_PACKAGE_LIST: &str = "The package install in the image acted on the package's list.";

/// § 10.9's line for a unit whose install-difference advisories or
/// undetermined install block the export no longer supports: `excluded`
/// are its install files the export does not stage. Unescaped.
pub fn install_omitted_line(unit: &UnitName, excluded: &[String]) -> String {
    let unit = unit.as_str();
    format!(
        "# Omitted: {unit} links (excluded from the export: {}; the [Install] the image reads \
         is not the one recorded, so the source host's links for {unit} are not reproduced, and \
         links its package install made may remain in the image. Check them with systemctl \
         is-enabled in the image.)",
        excluded.join(", ")
    )
}

/// § 10.9's unit-level block for one undetermined install record, the one
/// source for the Containerfile and the view response, unescaped: its
/// install files come from a directory listing, so each surface escapes the
/// finished line for itself. A record with install files names them at
/// the end of the first line; one with none assigns `Also=`, which the
/// first line says instead, or, for a file that could not be read as text
/// ([`InstallUnknown::Unreadable`]), may assign it.
pub fn undetermined_install_lines(record: &UndeterminedInstall) -> Vec<String> {
    let unit = record.unit.as_str();
    let opening = format!(
        "# Advisory: {unit} is not carried: its [Install] section on the source host cannot be \
         relied on ({})",
        install_unknown_words(record.reason)
    );
    let first = if record.install_files.is_empty() {
        let also = match record.reason {
            InstallUnknown::Unreadable => UNDETERMINED_MAY_ASSIGN_ALSO,
            _ => UNDETERMINED_ASSIGNS_ALSO,
        };
        format!("{opening}. {also}")
    } else {
        format!(
            "{opening}, and these files under /etc/systemd/system change it: {}.",
            record.install_files.join(", ")
        )
    };
    vec![
        first,
        format!("#   The image's links for {unit} {UNDETERMINED_CHECK}"),
    ]
}

// ---------------------------------------------------------------------------
// § 10.6 mount, automount and swap advisories
// ---------------------------------------------------------------------------

/// The advisory's first line, after `<unit> is <state> on the source host.`
const MOUNT_ADVISORY_FACT: &str = "inspectah does not enable, disable or mask a mount, \
     automount or swap unit the administrator wrote.";

/// The state the first line names for a finding with no state change: an
/// `/etc` unit file that is neither enabled nor masked. It may be disabled
/// or static; the collector does not record which.
const MOUNT_STATE_NOT_RECORDED: &str = "not enabled";

/// The unit's file is in `staged_writes`.
const MOUNT_FILE_STAGED: &str = "#   Its unit file is copied into the image. No enable line is \
     written, but the unit still starts if another unit requires it or names its mount point \
     in RequiresMountsFor=, and it replaces a unit of the same name that the image ships.";

/// `mount_point` is one of [`BOOTC_OWNED_MOUNT_POINTS`].
const MOUNT_BOOTC_OWNED: &str = "#   bootc and ostree own this mount point on the target.";

/// The unit is `tmp.mount` and its file is in `staged_writes`.
const MOUNT_TMP_FILE_STAGED: &str = "#   The bootc base image starts tmp.mount from a link it \
     ships, so this file replaces the unit that mounts /tmp on the target.";

/// A finding more than one host recorded, merged into one host's finding.
const MOUNT_MERGED_FROM_HOSTS: &str =
    "#   This describes one host's copy of the unit; the copy staged may differ.";

/// A mount or swap with `nofail` absent.
const MOUNT_NO_NOFAIL: &str = "#   No nofail option in the unit file. If the unit is started \
     at boot and its device is missing, units that require it fail, and boot can stop in \
     emergency mode.";

/// Whether `path` is `top` or a path under it, by whole components.
fn is_or_is_under(path: &str, top: &str) -> bool {
    path.strip_prefix(top)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// `mount_point` in normal form, as [`Destination::new`] makes one: empty
/// and `.` components dropped, so `/var/` is `/var` and `/srv//data` is
/// `/srv/data`. An absolute path with no other component is `/`. A path
/// `Destination` refuses for another reason (a `..` component, a control
/// character) is returned as written.
fn normal_mount_point(mount_point: &str) -> String {
    match Destination::new(mount_point) {
        Ok(path) => path.as_str().to_owned(),
        Err(PathError::NoFileName) if mount_point.starts_with('/') => "/".to_owned(),
        Err(_) => mount_point.to_owned(),
    }
}

/// The comment lines of § 10.6 for one mount finding, in the table's
/// order: the one source for the Containerfile and the view response. The
/// lines are the spec's text with the unit, its state, the path and the
/// verb filled in, `#` included, and are not escaped: `mount_point` comes
/// from a host file, so each surface escapes the finished line for itself.
///
/// - Always, the first line. A finding with no state change names the
///   unit `not enabled`.
/// - The merged line, when the finding's fleet counts show more than one
///   host recorded the unit: the finding is one host's, and the unit file
///   staged may be another's. Never on a single-host scan, which has no
///   fleet counts.
/// - The file line, when `staged` writes the unit's file at
///   `/etc/systemd/system/<unit>` ([`is_staged`]), so it follows the
///   config row's toggle.
/// - From `mount_point`, when there is one and the unit is not masked:
///   under `/usr`; one of [`BOOTC_OWNED_MOUNT_POINTS`]; under one of
///   [`BASE_VAR_SYMLINKS`] (naming that top-level directory, and for an
///   automount its matching mount unit too); under one of
///   [`BASE_IMAGE_DIRECTORIES`]. Each is matched against the mount point in
///   normal form ([`normal_mount_point`]), and a line that shows the path
///   shows that form. A masked unit does not mount, so the layout of its
///   mount point does not bear on it.
/// - The `tmp.mount` line, when that unit's file is staged.
/// - The `nofail` line, for a mount or swap whose `nofail` is
///   `Some(false)` (its unit file was read and `Options=` has no
///   `nofail`), when the unit is not masked and `drop_ins` holds no row
///   for the unit. `None` (a mask with no unit file, a file that is not
///   text or could not be read) gets no line. A mount unit with no
///   `mount_point` gets none either: its file was not read, or has no
///   absolute `Where=` in its `[Mount]` section. The collector reads
///   `Options=` from the unit file alone, by logical line: a drop-in may
///   set `nofail`, so a unit with any drop-in row whose `unit` is its
///   name gets no line.
/// - The `To carry it` line, for an enabled or masked unit, except a
///   masked `tmp.mount` (§ 10.3), and except where the advisory itself
///   rules the recipe out: an enabled unit given the `/var` rename line
///   (the two cannot both be followed), and a masked unit whose file is
///   staged at its name (`systemctl mask` fails on the occupied path,
///   § 10.2 rule 4). These two exceptions go beyond § 10.6's table, which
///   conditions the line on the state alone.
pub fn mount_advisory_lines(
    finding: &MountUnitFinding,
    staged: &StagedWrites<'_>,
    drop_ins: &[SystemdDropIn],
) -> Vec<String> {
    let name = finding.unit.name();
    let unit = name.as_unit_name();
    let state = finding.state_change.map(|(state, _)| state);
    let masked = state == Some(UnitState::Masked);
    let state_text = state.map_or_else(|| MOUNT_STATE_NOT_RECORDED.to_owned(), |s| s.to_string());
    let mut lines = vec![format!(
        "# Advisory: {} is {state_text} on the source host. {MOUNT_ADVISORY_FACT}",
        unit.as_str()
    )];
    // A merged finding is the whole finding of one host (`merge_items` in
    // `inspectah_core::aggregate::merge`); the unit file staged may be
    // another host's variant. A finding one host recorded, on a single
    // host or an aggregate, has no other copy to differ from.
    if finding.aggregate.as_ref().is_some_and(|a| a.count > 1) {
        lines.push(MOUNT_MERGED_FROM_HOSTS.to_owned());
    }
    let file_staged = is_staged(staged, &etc_unit_path(unit));
    if file_staged {
        lines.push(MOUNT_FILE_STAGED.to_owned());
    }
    let mount_point = finding
        .mount_point
        .as_deref()
        .filter(|_| !masked)
        .map(normal_mount_point);
    let mut renamed = false;
    if let Some(path) = mount_point.as_deref() {
        if is_or_is_under(path, IMAGE_USR) {
            lines.push(format!(
                "#   /usr is part of the read-only image on the target. The mount point must \
                 already exist in the image, and a mount there hides the image's own content \
                 under {path}."
            ));
        }
        if BOOTC_OWNED_MOUNT_POINTS.contains(&path) {
            lines.push(MOUNT_BOOTC_OWNED.to_owned());
        }
        if let Some(top) = BASE_VAR_SYMLINKS
            .iter()
            .find(|top| is_or_is_under(path, top))
        {
            // An automount keeps its suffix, and its matching mount unit
            // is renamed with it (systemd.automount(5): an automount unit
            // needs a mount unit of the same name).
            lines.push(if name.kind() == StorageKind::Automount {
                format!(
                    "#   {top} is a symlink into /var in the RHEL bootc base image. The unit \
                     must be renamed for the resolved path (for /srv: var-srv.automount with \
                     Where=/var/srv), and so must its matching mount unit (var-srv.mount)."
                )
            } else {
                format!(
                    "#   {top} is a symlink into /var in the RHEL bootc base image. The unit \
                     must be renamed for the resolved path (for /srv: var-srv.mount with \
                     Where=/var/srv)."
                )
            });
            renamed = true;
        }
        if let Some(top) = BASE_IMAGE_DIRECTORIES
            .iter()
            .find(|top| is_or_is_under(path, top))
        {
            lines.push(format!(
                "#   {top} is a directory of the read-only image in the RHEL bootc base image, \
                 not a link into /var. The unit keeps its name. The mount point must already \
                 exist in the image, and a mount there hides the image's own content under \
                 {path}."
            ));
        }
    }
    let is_tmp_mount = unit.as_str() == TMP_MOUNT;
    if is_tmp_mount && file_staged {
        lines.push(MOUNT_TMP_FILE_STAGED.to_owned());
    }
    // The line is a statement about the unit file's `Options=`, so it needs
    // them read (`Some(false)`). A mount unit with no mount point gets no
    // line either, and an automount has no `Options=`.
    let kind_allows_line = match name.kind() {
        StorageKind::Mount => finding.mount_point.is_some(),
        StorageKind::Swap => true,
        StorageKind::Automount => false,
    };
    // The collector reads `Options=` from the unit file alone. With a
    // drop-in row for the unit, a drop-in may set `nofail`, so the finding's
    // settings are not known to be the effective ones.
    let has_drop_ins = drop_ins.iter().any(|d| d.unit == unit.as_str());
    if kind_allows_line && finding.nofail == Some(false) && !masked && !has_drop_ins {
        lines.push(MOUNT_NO_NOFAIL.to_owned());
    }
    // A recipe that its own advisory rules out is not given: a unit told
    // to be renamed is not enabled under its old name, and a mask cannot
    // be made where the export stages a unit file of that name.
    let verb = match state {
        Some(UnitState::Enabled) if !renamed => Some(UnitAction::Enable),
        Some(UnitState::Masked) if !is_tmp_mount && !file_staged => Some(UnitAction::Mask),
        Some(UnitState::Enabled | UnitState::Masked | UnitState::Disabled) | None => None,
    };
    if let Some(verb) = verb {
        lines.push(format!(
            "#   To carry it, add after the configuration COPY: RUN systemctl {verb} -- {}",
            single_quote(unit.as_str())
        ));
    }
    lines
}

// ---------------------------------------------------------------------------
// Rules 2, 3 and 4
// ---------------------------------------------------------------------------

/// Rule 2 (§ 10.2), every verb: a package-owned storage unit acts only on
/// its package's file. A unit file staged at its name is what the image
/// would load, so the command is declined with that file's path. Any write
/// at the destination is a unit file there, whatever offered it, so the
/// stager's yes or no ([`is_staged`]) is the whole question.
fn rule_2_storage_file(
    unit: &CarriedUnit,
    staged: &StagedWrites<'_>,
) -> Result<(), OmissionReason> {
    match unit {
        CarriedUnit::PackagedStorage(_) => {
            let path = etc_unit_path(unit.name());
            if is_staged(staged, &path) {
                Err(OmissionReason::StorageFileStaged { path })
            } else {
                Ok(())
            }
        }
        CarriedUnit::Ordinary(_) => Ok(()),
    }
}

/// Rule 3 (§ 10.2, § 10.3): no `disable` or `mask` of a unit the target
/// needs, and no `mask` of `tmp.mount` ([`target_needs_unit`]). Rendered as
/// a FIXME.
fn rule_3_target_needs_unit(name: &UnitName, action: UnitAction) -> Result<(), OmissionReason> {
    if target_needs_unit(name.as_str(), action) {
        Err(OmissionReason::TargetNeedsUnit)
    } else {
        Ok(())
    }
}

/// Rule 4 (§ 10.2), `mask` only: `systemctl mask` makes a link at
/// `/etc/systemd/system/<name>` after every `COPY` and exits 1 on a regular
/// file there, so a mask needs that path free of any staged file.
fn rule_4_mask_path(
    name: &UnitName,
    action: UnitAction,
    staged: &StagedWrites<'_>,
) -> Result<(), OmissionReason> {
    if action != UnitAction::Mask {
        return Ok(());
    }
    let path = etc_unit_path(name);
    if is_staged(staged, &path) {
        Err(OmissionReason::MaskPathOccupied { path })
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Rule 5, the row's own file
// ---------------------------------------------------------------------------

/// Where the image loads a unit file named `file` from `/etc`: the path a
/// config entry or drop-in row for it records, and refine's
/// `unit_file_path`.
pub fn etc_unit_path(file: &UnitName) -> String {
    format!("{ETC_UNIT_DIR}/{}", file.as_str())
}

/// Rule 5's fragment half (§ 10.2): the export stages exactly the unit
/// file the row recorded, or the command is declined.
///
/// - `Local`: the unit file at `/etc/systemd/system/<file>` is staged,
///   else [`OmissionReason::UnitFileNotStaged`].
/// - `Shadowed`: the shadow at `/etc/systemd/system/<file>` is staged,
///   else [`OmissionReason::InstallEvidenceChanged`] with its path.
/// - `Vendor`, a package-owned storage unit, and (for a mask, which this
///   rule never sees) `Absent`: no unit file is staged at the unit's name,
///   nor, for an instance recorded from its vendor template, at the
///   template's, else `InstallEvidenceChanged` with that path.
///
/// "Staged" is [`is_staged`], the stager's own answer by [`Destination`],
/// so the plan and the stager cannot disagree about a spelling. In a fleet
/// aggregate a staged `Local` or `Shadowed` file whose path holds more than
/// one content variant fails too: the row may have been recorded from
/// another variant.
fn rule_5_file(
    snap: &InspectionSnapshot,
    unit: &CarriedUnit,
    staged: &StagedWrites<'_>,
) -> Result<(), OmissionReason> {
    let fragment = match unit {
        CarriedUnit::Ordinary(ordinary) => ordinary.fragment(),
        CarriedUnit::PackagedStorage(_) => {
            return no_file_at(etc_unit_path(unit.name()), staged);
        }
    };
    match fragment {
        UnitFragment::Local { file } => {
            let path = etc_unit_path(file);
            if !is_staged(staged, &path) {
                return Err(OmissionReason::UnitFileNotStaged);
            }
            one_variant(snap, path)?;
            own_name_free(unit.name(), file, staged)
        }
        UnitFragment::Shadowed { file, .. } => {
            let path = etc_unit_path(file);
            if !is_staged(staged, &path) {
                return Err(OmissionReason::InstallEvidenceChanged { path });
            }
            one_variant(snap, path)?;
            own_name_free(unit.name(), file, staged)
        }
        // An instance recorded from its vendor template: a template file
        // staged under `/etc` would load in the template's place, with an
        // `[Install]` nobody recorded.
        UnitFragment::Vendor { file, .. } => {
            no_file_at(etc_unit_path(unit.name()), staged)?;
            if file == unit.name() {
                return Ok(());
            }
            no_file_at(etc_unit_path(file), staged)
        }
        UnitFragment::Absent => no_file_at(etc_unit_path(unit.name()), staged),
    }
}

/// Rule 5 asked of a timer's trigger (§ 10.2, "A timer's trigger is held
/// to the same rule"), for an `enable` that passed the timer's own file
/// and package. `Ok` for a unit that is not a timer. The timer's recorded
/// descriptions are its local one when there is one (the `/etc` file is
/// what loads), else its vendor one; each must pass, in order:
///
/// 1. every path in `trigger_drop_ins` is staged, else
///    [`OmissionReason::TimerTriggerEvidenceChanged`];
/// 2. the trigger is resolved, else
///    [`OmissionReason::TimerTriggerUnresolved`]. A timer with no
///    description records no trigger, which is read the same way;
/// 3. the trigger is not a local unit whose file is unstaged with nothing
///    else providing it ([`local_trigger_missing`]), else
///    [`OmissionReason::TimerTriggerNotStaged`].
fn timer_trigger_check(
    snap: &InspectionSnapshot,
    timer: &UnitName,
    presence: Option<&Presence<'_>>,
    staged: &StagedWrites<'_>,
) -> Result<(), OmissionReason> {
    if timer.kind() != UnitKind::Ordinary(OrdinaryKind::Timer) {
        return Ok(());
    }
    let stem = timer
        .as_str()
        .strip_suffix(".timer")
        .expect("a timer's name ends in .timer");
    let described: Vec<_> = snap
        .scheduled_tasks
        .iter()
        .flat_map(|st| &st.systemd_timers)
        .filter(|t| t.name == stem)
        .collect();
    let local: Vec<_> = described
        .iter()
        .copied()
        .filter(|t| t.source == "local")
        .collect();
    let recorded = if local.is_empty() { described } else { local };
    if recorded.is_empty() {
        return Err(OmissionReason::TimerTriggerUnresolved);
    }
    for description in recorded {
        if let Some(path) = description
            .trigger_drop_ins
            .iter()
            .find(|path| !is_staged(staged, path))
        {
            return Err(OmissionReason::TimerTriggerEvidenceChanged { path: path.clone() });
        }
        let TimerTrigger::Unit(trigger) = &description.trigger else {
            return Err(OmissionReason::TimerTriggerUnresolved);
        };
        if local_trigger_missing(snap, trigger, presence, staged) {
            return Err(OmissionReason::TimerTriggerNotStaged {
                trigger: trigger.clone(),
            });
        }
    }
    Ok(())
}

/// Whether `trigger` is a local unit the image would lack: the snapshot
/// holds it as a unit file under `/etc/systemd/system` with no vendor file
/// of that name (a config entry at its path that shadows nothing, or a
/// state change recorded `Local`), that file is not staged, and nothing
/// else provides the unit. A staged generated unit of the name is a
/// staged file at the path. A package in the image that owns the config
/// entry provides it; one whose presence cannot be decided is taken to,
/// as the presence tiers take an unproved absence. A vendor file is known
/// from a state change recorded `Vendor` or `Shadowed`, or a drop-in row
/// at the path, which is a full shadow. The file of an instance trigger is
/// its template's unless the snapshot holds one at the instance's own name
/// ([`trigger_file`]).
fn local_trigger_missing(
    snap: &InspectionSnapshot,
    trigger: &UnitName,
    presence: Option<&Presence<'_>>,
    staged: &StagedWrites<'_>,
) -> bool {
    let path = etc_unit_path(&trigger_file(snap, trigger));
    if is_staged(staged, &path) {
        return false;
    }
    let Ok(dest) = Destination::new(&path) else {
        return false;
    };
    let at_dest = |p: &str| Destination::new(p).is_ok_and(|d| d == dest);
    let change = snap
        .units
        .iter()
        .flat_map(|u| &u.state_changes)
        .find(|sc| sc.unit().name() == trigger);
    let vendor_file = change.is_some_and(|sc| match sc.unit() {
        CarriedUnit::Ordinary(ordinary) => matches!(
            ordinary.fragment(),
            UnitFragment::Vendor { .. } | UnitFragment::Shadowed { .. }
        ),
        CarriedUnit::PackagedStorage(_) => true,
    }) || snap
        .units
        .iter()
        .flat_map(|u| &u.drop_ins)
        .any(|d| at_dest(&d.path));
    if vendor_file {
        return false;
    }
    let entries: Vec<_> = snap
        .config
        .iter()
        .flat_map(|c| &c.files)
        .filter(|f| at_dest(&f.path) && f.shadowed_by.is_none())
        .collect();
    let packaged = entries.iter().any(|f| {
        f.package.as_deref().is_some_and(|package| {
            presence.is_none_or(|p| p.baseline_unavailable || p.target_packages.contains(package))
        })
    });
    if packaged {
        return false;
    }
    let local_change = change.is_some_and(|sc| {
        matches!(
            sc.unit(),
            CarriedUnit::Ordinary(ordinary) if matches!(ordinary.fragment(), UnitFragment::Local { .. })
        )
    });
    local_change || !entries.is_empty()
}

/// The unit file under `/etc/systemd/system` that loads for `trigger`: its
/// own, or, for an instance the snapshot holds nothing at the own name of
/// (no config entry, no drop-in row, no state change recorded from that
/// file), its template's, which is what systemd loads for it.
///
/// Only `/etc` evidence at the own name counts. An instance trigger with
/// a vendor file of its own under `/usr/lib` beside an `/etc` template is
/// mapped to the template, although systemd loads the instance's own file
/// first; with the template excluded, the timer is then declined as
/// `TimerTriggerNotStaged` although its trigger is provided. That errs
/// toward omitting an enable, never toward carrying one.
fn trigger_file(snap: &InspectionSnapshot, trigger: &UnitName) -> UnitName {
    let Some(parts) = trigger.instance() else {
        return trigger.clone();
    };
    let Ok(own) = Destination::new(&etc_unit_path(trigger)) else {
        return trigger.clone();
    };
    let at_own = |p: &str| Destination::new(p).is_ok_and(|d| d == own);
    let held = snap
        .config
        .iter()
        .flat_map(|c| &c.files)
        .any(|f| at_own(&f.path))
        || snap
            .units
            .iter()
            .flat_map(|u| &u.drop_ins)
            .any(|d| at_own(&d.path))
        || snap
            .units
            .iter()
            .flat_map(|u| &u.state_changes)
            .filter(|sc| sc.unit().name() == trigger)
            .any(|sc| match sc.unit() {
                CarriedUnit::Ordinary(ordinary) => match ordinary.fragment() {
                    UnitFragment::Local { file } | UnitFragment::Shadowed { file, .. } => {
                        file == trigger
                    }
                    UnitFragment::Vendor { .. } | UnitFragment::Absent => false,
                },
                CarriedUnit::PackagedStorage(_) => false,
            });
    if held {
        trigger.clone()
    } else {
        parts.template()
    }
}

/// Whether rule 5 asks the presence tiers about the unit's package
/// (§ 10.2): only for a unit whose image file comes from that package, a
/// `Vendor` unit or a package-owned storage unit. A `Shadowed` unit's one
/// condition is its staged shadow, which the image loads with or without
/// the package; a `Local` unit has no owner; `Absent`, reached only by a
/// mask on a name with no unit file, has no owner either, so its mask is
/// carried.
fn needs_its_package(unit: &CarriedUnit) -> bool {
    match unit {
        CarriedUnit::PackagedStorage(_) => true,
        CarriedUnit::Ordinary(ordinary) => match ordinary.fragment() {
            UnitFragment::Vendor { .. } => true,
            UnitFragment::Shadowed { .. } | UnitFragment::Local { .. } | UnitFragment::Absent => {
                false
            }
        },
    }
}

/// For an instance recorded from its template's file under `/etc`, `Ok`
/// when the export stages no unit file at the instance's own name, which
/// would load in its place (§ 10.2, rule 5 for every fragment). `Ok` when
/// `file` is the unit's own.
fn own_name_free(
    name: &UnitName,
    file: &UnitName,
    staged: &StagedWrites<'_>,
) -> Result<(), OmissionReason> {
    if file == name {
        return Ok(());
    }
    no_file_at(etc_unit_path(name), staged)
}

/// The `/etc` drop-in directories `enable` reads `[Install]` from for
/// `name`: its own `.d` and, for an instance, its template's. The
/// type-wide and dash-prefix directories are never among them (§ 10.2,
/// "Install files").
fn install_drop_in_dirs(name: &UnitName) -> Vec<String> {
    let mut dirs = vec![format!("{ETC_UNIT_DIR}/{}.d", name.as_str())];
    if let Some(parts) = name.instance() {
        dirs.push(format!("{ETC_UNIT_DIR}/{}.d", parts.template().as_str()));
    }
    dirs
}

/// Rule 5 for every fragment, the drop-in half (§ 10.2): the install
/// drop-ins the export stages for the unit are exactly the row's
/// `install_drop_ins`, none missing and none added.
///
/// - Each recorded path is read through [`Destination`]; one that does not
///   parse, or is not staged, fails. In a fleet aggregate, one whose path
///   holds more than one content variant fails too ([`one_variant`]). The
///   recorded paths include a drop-in ranked above `/etc` that hides one
///   of the `/etc` ones, which the configuration export stages when it is
///   under `/etc/systemd/system.control` and included, and never when it
///   is under `/run`.
/// - A staged drop-in systemd reads ([`systemd_reads_drop_in`]: a `.conf`
///   name that is not hidden) directly in one of [`install_drop_in_dirs`]
///   that the row does not record fails when it is an install file: it
///   holds an `[Install]` section ([`holds_install_section`]), or it has
///   the file name of one of the row's `vendor_drop_ins`.
///
/// Every failure is [`OmissionReason::InstallEvidenceChanged`] naming the
/// path.
fn rule_5_drop_ins(
    snap: &InspectionSnapshot,
    change: &UnitStateChange,
    staged: &StagedWrites<'_>,
) -> Result<(), OmissionReason> {
    let changed = |path: &str| OmissionReason::InstallEvidenceChanged {
        path: path.to_owned(),
    };
    let mut recorded = Vec::new();
    for path in &change.install_drop_ins {
        let Ok(dest) = Destination::new(path) else {
            return Err(changed(path));
        };
        if !staged.writes.contains_key(&dest) {
            return Err(changed(path));
        }
        one_variant(snap, path.clone())?;
        recorded.push(dest);
    }
    for dir in install_drop_in_dirs(change.unit().name()) {
        let Ok(start) = Destination::new(&dir) else {
            continue;
        };
        let prefix = format!("{}/", start.as_str());
        // Every path under the directory sorts after it and before any
        // path that does not start with its name.
        for (dest, write) in staged
            .writes
            .range(start.clone()..)
            .take_while(|(dest, _)| dest.as_str().starts_with(start.as_str()))
        {
            let Some(file) = dest.as_str().strip_prefix(&prefix) else {
                continue;
            };
            if file.contains('/') || !systemd_reads_drop_in(file) || recorded.contains(dest) {
                continue;
            }
            if holds_install_section(&write.content)
                || change.vendor_drop_ins.iter().any(|name| name == file)
            {
                return Err(changed(dest.as_str()));
            }
        }
    }
    Ok(())
}

/// `Ok` when the export stages no file at `path`.
fn no_file_at(path: String, staged: &StagedWrites<'_>) -> Result<(), OmissionReason> {
    if is_staged(staged, &path) {
        Err(OmissionReason::InstallEvidenceChanged { path })
    } else {
        Ok(())
    }
}

/// `Ok` unless the snapshot is a fleet aggregate whose config entries and
/// drop-in rows at `path` hold more than one content, whichever of them is
/// selected ([`holds_variants`]).
fn one_variant(snap: &InspectionSnapshot, path: String) -> Result<(), OmissionReason> {
    match Destination::new(&path) {
        Ok(dest) if holds_variants(snap, &dest) => {
            Err(OmissionReason::InstallEvidenceChanged { path })
        }
        _ => Ok(()),
    }
}

/// Whether the snapshot is a fleet aggregate whose config entries and
/// drop-in rows at `dest` hold more than one content, whichever of them is
/// selected.
fn holds_variants(snap: &InspectionSnapshot, dest: &Destination) -> bool {
    if snap.aggregate_meta.is_none() {
        return false;
    }
    let at_dest = |p: &str| Destination::new(p).is_ok_and(|d| d == *dest);
    let config = snap
        .config
        .iter()
        .flat_map(|c| &c.files)
        .filter(|f| at_dest(&f.path))
        .map(|f| f.content.as_str());
    let drop_ins = snap
        .units
        .iter()
        .flat_map(|u| &u.drop_ins)
        .filter(|d| at_dest(&d.path))
        .map(|d| d.content.as_str());
    let contents: std::collections::BTreeSet<&str> = config.chain(drop_ins).collect();
    contents.len() > 1
}

// ---------------------------------------------------------------------------
// Rule 6, an instance's configuration
// ---------------------------------------------------------------------------

/// The attention note of a carried instance whose configuration file was
/// staged by a scan taken with no redaction (§ 10.8), before the path.
const CONFIG_UNREDACTED: &str = "Its configuration";

/// [`CONFIG_UNREDACTED`]'s text after the path.
const CONFIG_UNREDACTED_REST: &str = "is staged as the source host had it: the scan was taken \
     with --no-redaction, so no secret in it was redacted.";

/// The attention note of a carried instance whose configuration holds a
/// value the redaction pass flagged and did not change (§ 10.8), after the
/// path.
const CONFIG_FLAGGED_REST: &str = "holds a value the redaction pass flagged and did not \
     change. Check it before the image is used.";

/// § 10.8's data sentence, after the unit, its paths and the pronoun that
/// stands for them.
const DATA_ADVISORY_REST: &str = ", and the carried enable starts the unit on first boot. Put \
     the data in place before that boot, or the unit starts without it.";

/// § 10.8 "Data under `/var`": the sentence for a carried instance enable
/// whose data sits at `paths`, which the image does not carry. Several
/// paths are named in one sentence (`a, b and c`) and are "them" where
/// one path is "it". Written as an `# Advisory:` line before the `RUN`
/// lines ([`unit_state_lines`]) and, unchanged, on the refine row's
/// `attention_reason`. `None` when `paths` is empty: an instance with no
/// data has no sentence, so no caller can render one that names nothing.
pub fn data_advisory(unit: &UnitName, paths: &[String]) -> Option<String> {
    let (named, pronoun) = match paths {
        [] => return None,
        [one] => (one.clone(), "it"),
        [rest @ .., last] => (format!("{} and {last}", rest.join(", ")), "them"),
    };
    Some(format!(
        "{} keeps data under {named}. The image does not carry {pronoun}{DATA_ADVISORY_REST}",
        unit.as_str()
    ))
}

/// Rule 6 (§ 10.2, § 10.8), for an `enable` on a template instance: every
/// path in `required_config` is in the export as the source host had it.
/// For each path, in order, the config entries at it or under it decide,
/// in path order:
///
/// - none: [`ConfigGap::NotInSnapshot`], naming the path. Package
///   ownership is no evidence (§ 10.8, "Only a file the export carries
///   passes");
/// - an entry whose content was not captured: [`ConfigGap::NotCaptured`];
/// - an entry the stager does not write, excluded, left out or withheld:
///   [`ConfigGap::Excluded`];
/// - an entry `snapshot.redactions` holds an `Inline` or `Excluded`
///   finding for: [`ConfigGap::Redacted`];
/// - on a fleet aggregate, an entry whose path holds more than one content
///   variant, whichever is staged: [`ConfigGap::FleetVariants`], as rule 5
///   declines such an install file ([`one_variant`]).
///
/// Each but the first names the entry's path. `Ok` holds the attention
/// notes of a passing instance: one per entry with a `Flagged` finding,
/// and one per entry when the snapshot was taken with no redaction.
fn rule_6_instance_config(
    snap: &InspectionSnapshot,
    change: &UnitStateChange,
    staged: &StagedWrites<'_>,
) -> Result<Vec<String>, OmissionReason> {
    let mut notes = Vec::new();
    if change.unit().name().instance().is_none() {
        return Ok(notes);
    }
    let unredacted = matches!(snap.redaction_state, Some(RedactionState::Raw));
    let gap = |path: &str, gap| OmissionReason::InstanceConfigNotStaged {
        path: path.to_owned(),
        gap,
    };
    for required in &change.required_config {
        let Ok(at) = Destination::new(required) else {
            return Err(gap(required, ConfigGap::NotInSnapshot));
        };
        let under = format!("{}/", at.as_str());
        // Each entry at or under the path, by its destination, or by its
        // path as written when the stager refuses it (`None`): a refused
        // entry is never staged, so it declines the enable, not drops out.
        let mut entries: Vec<(String, Option<Destination>, _)> = snap
            .config
            .iter()
            .flat_map(|c| &c.files)
            .filter_map(|f| match Destination::new(&f.path) {
                Ok(d) if d == at || d.as_str().starts_with(&under) => {
                    Some((d.as_str().to_owned(), Some(d), f))
                }
                Ok(_) => None,
                Err(_) => lexically_under(&f.path, &at).then(|| (f.path.clone(), None, f)),
            })
            .collect();
        if entries.is_empty() {
            return Err(gap(required, ConfigGap::NotInSnapshot));
        }
        entries.sort_by(|(a, ..), (b, ..)| a.cmp(b));
        for (path, dest, entry) in entries {
            if !entry.content_captured() {
                return Err(gap(&path, ConfigGap::NotCaptured));
            }
            let Some(dest) = dest.filter(|d| staged.writes.contains_key(d)) else {
                return Err(gap(&path, ConfigGap::Excluded));
            };
            let path = dest.as_str();
            let findings: Vec<&RedactionKind> = snap
                .redactions
                .iter()
                .filter(|f| Destination::new(&f.path).is_ok_and(|d| d == dest))
                .map(|f| &f.kind)
                .collect();
            if findings
                .iter()
                .any(|kind| matches!(kind, RedactionKind::Inline | RedactionKind::Excluded))
            {
                return Err(gap(path, ConfigGap::Redacted));
            }
            if holds_variants(snap, &dest) {
                return Err(gap(path, ConfigGap::FleetVariants));
            }
            if unredacted {
                notes.push(format!(
                    "{CONFIG_UNREDACTED} {path} {CONFIG_UNREDACTED_REST}"
                ));
            } else if findings.contains(&&RedactionKind::Flagged) {
                notes.push(format!("{CONFIG_UNREDACTED} {path} {CONFIG_FLAGGED_REST}"));
            }
        }
    }
    Ok(notes)
}

/// Whether `path`, a path [`Destination`] refuses, sits at or under `at`
/// as written: its components, empty and `.` ones dropped, start with
/// `at`'s. Whatever follows (a `..`, a line break) is not interpreted, so a
/// refused entry whose written path starts inside `at` counts as under it.
fn lexically_under(path: &str, at: &Destination) -> bool {
    let mut components = path.split('/').filter(|c| !matches!(*c, "" | "."));
    at.as_str()
        .split('/')
        .skip(1)
        .all(|part| components.next() == Some(part))
}

// ---------------------------------------------------------------------------
// classify_service_presence — the tiered suppression model
// ---------------------------------------------------------------------------

/// Determine whether a unit should be omitted or emitted, and if emitted,
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
    unit: &CarriedUnit,
    rpm: &RpmSection,
    target_packages: &std::collections::BTreeSet<String>,
    not_carried: &std::collections::HashSet<String>,
    baseline_unavailable: bool,
) -> PresenceDecision {
    let pkg_name = match unit.owning_package() {
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
                advisory_reasons: Some((pkg_name.to_owned(), reasons)),
            };
        }

        // Tier 2: all entries are excluded. A baseline that still ships the
        // package (another arch excluded, say) keeps the unit present, so
        // fall through to Tier 5 in that case.
        if let Some(omit) = excluded_owner(unit, rpm, target_packages, not_carried) {
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
            advisory_reasons: Some((
                pkg_name.to_owned(),
                vec![AdvisoryReason::BaselineUnavailable],
            )),
        }
    } else {
        // Tier 7: baseline available and package not in it — proven absent
        PresenceDecision::Omit(OmissionReason::NotInTargetImage {
            package: pkg_name.to_owned(),
        })
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
    unit: &CarriedUnit,
    rpm: &RpmSection,
    target_packages: &std::collections::BTreeSet<String>,
    not_carried: &std::collections::HashSet<String>,
) -> Option<PresenceDecision> {
    let pkg_name = unit.owning_package()?;
    let mut entries = rpm
        .packages_added
        .iter()
        .filter(|p| p.name == *pkg_name)
        .peekable();
    if entries.peek().is_none() || target_packages.contains(pkg_name) {
        return None;
    }
    let package = pkg_name.to_owned();
    let reason = if entries.all(|p| not_carried.contains(&format!("{}.{}", p.name, p.arch))) {
        OmissionReason::NotCarried { package }
    } else {
        OmissionReason::PackageExcluded { package }
    };
    Some(PresenceDecision::Omit(reason))
}

/// The presence decision for an aggregate snapshot.
///
/// An aggregate's `packages_added` is leaf-only, so the tiers that read
/// absence from it (3, 6 and 7) would omit the unit of an auto dependency
/// that the image does install. Tier 2 reads an explicit exclusion on an
/// entry that is present, which leaf filtering cannot fake, so it applies
/// here as it does to a single host; every other unit emits.
fn aggregate_presence(
    unit: &CarriedUnit,
    rpm: &RpmSection,
    target_packages: &std::collections::BTreeSet<String>,
    not_carried: &std::collections::HashSet<String>,
) -> PresenceDecision {
    excluded_owner(unit, rpm, target_packages, not_carried).unwrap_or(PresenceDecision::Emit {
        advisory_reasons: None,
    })
}

/// What the presence tiers read, computed once per plan.
struct Presence<'a> {
    rpm: &'a RpmSection,
    target_packages: std::collections::BTreeSet<String>,
    not_carried: std::collections::HashSet<String>,
    baseline_unavailable: bool,
    is_aggregate: bool,
}

impl<'a> Presence<'a> {
    fn new(snap: &InspectionSnapshot, rpm: &'a RpmSection) -> Self {
        Self {
            rpm,
            target_packages: effective_target_packages(rpm),
            baseline_unavailable: rpm.baseline_package_names.is_none(),
            // Empty for an aggregate, which `not_carried` never classifies.
            not_carried: not_carried_packages(snap)
                .into_iter()
                .map(|pkg| format!("{}.{}", pkg.name, pkg.arch))
                .collect(),
            // Aggregate snapshots use leaf-only packages_added, so the
            // tiers that read absence from it would omit units whose owning
            // package is an auto (non-leaf) dependency. Aggregate applies
            // Tier 2 alone; see `aggregate_presence`.
            is_aggregate: snap.aggregate_meta.is_some(),
        }
    }

    fn decide(&self, unit: &CarriedUnit) -> PresenceDecision {
        if self.is_aggregate {
            aggregate_presence(unit, self.rpm, &self.target_packages, &self.not_carried)
        } else {
            classify_service_presence(
                unit,
                self.rpm,
                &self.target_packages,
                &self.not_carried,
                self.baseline_unavailable,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// plan_units: the main entry point
// ---------------------------------------------------------------------------

/// The one check of the assembled export (§ 10.2): every included state
/// change, planned against `staged`, the decision the stager acts on. The
/// single authority for which unit-state `systemctl` lines the
/// Containerfile carries; refine reads the same plan.
///
/// Per state change, the rules of the § 10.2 table in its order; the first
/// that fails decides:
///
/// 1. Rule 2 ([`rule_2_storage_file`]), every verb: a package-owned
///    storage unit with a unit file staged at its name is declined.
/// 2. Rule 3 ([`rule_3_target_needs_unit`]), `disable` and `mask`: a unit
///    the target needs is not switched off (§ 10.3).
/// 3. Rule 4 ([`rule_4_mask_path`]), `mask` only: the mask's path must be
///    free of a staged unit file.
/// 4. Rule 5, the row's own file ([`rule_5_file`]), `enable` and
///    `disable`: the export stages the unit file the row recorded and no
///    other. A `mask` reads no `[Install]`, so this half does not apply to
///    it.
/// 5. Rule 5, the package, every verb, `mask` included, for a `Vendor` or
///    package-owned storage unit only ([`needs_its_package`]): the presence
///    tiers
///    ([`classify_service_presence`], unchanged; an aggregate applies Tier 2
///    alone). With no package section there is nothing to read and the unit
///    is emitted: there is no separate path without one.
/// 6. Rule 6 ([`rule_6_instance_config`]), `enable` on a template
///    instance: its configuration is in the export as the host had it.
///    Its attention notes go to `instance_notes` once the enable is
///    planned.
/// 7. Rule 7 ([`rule_7_also`]), `enable` and `disable` with `Also=`
///    members, once every row's result under the rules above is known:
///    the command becomes an advisory unless its members agree.
///
/// Every other command is planned. The included default target is held
/// to rule 5 ([`plan_default_target`]). Each mount finding gets its § 10.6
/// advisory lines against `staged`, in `mount_advisories`. The enables of
/// the units inspectah generates are planned first ([`plan_generated`]),
/// in `scheduled` and `workloads`, or declined in `generated_declined`:
/// a generated unit whose name a collected unit shares was withheld by the
/// stager, so the collected unit is planned against the files that
/// remain.
pub fn plan_units(snap: &InspectionSnapshot, staged: &StagedWrites<'_>) -> UnitPlan {
    let mut plan = UnitPlan::default();
    plan_generated(snap, staged, &mut plan);
    let Some(units) = &snap.units else {
        return plan;
    };
    let presence = snap.rpm.as_ref().map(|rpm| Presence::new(snap, rpm));

    plan.excluded_updaters = units
        .state_changes
        .iter()
        .filter(|sc| !sc.disposition.is_included())
        .map(|sc| sc.unit().name())
        .filter(|name| is_runtime_updater(name.as_str()))
        .cloned()
        .collect();
    // The rows that pass rules 1 to 6, with their presence advisories and
    // instance notes. Rule 7 reads every row's result, so it runs once all
    // are known.
    type Passed<'s> = (
        &'s UnitStateChange,
        PlannedUnit,
        Option<ServiceAdvisory>,
        Vec<String>,
    );
    let mut passed: Vec<Passed<'_>> = Vec::new();
    for sc in units
        .state_changes
        .iter()
        .filter(|sc| sc.disposition.is_included())
    {
        let name = sc.unit().name().clone();
        let action = sc.implied_action();
        let rules_2_to_4 = rule_2_storage_file(sc.unit(), staged)
            .and_then(|()| rule_3_target_needs_unit(&name, action))
            .and_then(|()| rule_4_mask_path(&name, action, staged));
        if let Err(reason) = rules_2_to_4 {
            plan.declined.push((name, action, reason));
            continue;
        }
        // `mask` reads no `[Install]`, so the file half of rule 5 does not
        // apply to it; the package half does (§ 10.2 "A mask follows the
        // package check").
        if action != UnitAction::Mask
            && let Err(reason) = rule_5_file(snap, sc.unit(), staged)
                .and_then(|()| rule_5_drop_ins(snap, sc, staged))
        {
            plan.declined.push((name, action, reason));
            continue;
        }
        let decision = presence
            .as_ref()
            .filter(|_| needs_its_package(sc.unit()))
            .map(|p| p.decide(sc.unit()));
        let advisory = match decision {
            Some(PresenceDecision::Omit(reason)) => {
                plan.declined.push((name, action, reason));
                continue;
            }
            Some(PresenceDecision::Emit {
                advisory_reasons: Some((owning_package, reasons)),
            }) => Some(ServiceAdvisory {
                unit: name.clone(),
                owning_package,
                reasons,
            }),
            Some(PresenceDecision::Emit {
                advisory_reasons: None,
            })
            | None => None,
        };
        if action == UnitAction::Enable
            && let Err(reason) = timer_trigger_check(snap, &name, presence.as_ref(), staged)
        {
            plan.declined.push((name, action, reason));
            continue;
        }
        let notes = if action == UnitAction::Enable {
            match rule_6_instance_config(snap, sc, staged) {
                Ok(notes) => notes,
                Err(reason) => {
                    plan.declined.push((name, action, reason));
                    continue;
                }
            }
        } else {
            Vec::new()
        };
        passed.push((sc, PlannedUnit { name, action }, advisory, notes));
    }
    let rows: BTreeMap<&UnitName, &UnitStateChange> = units
        .state_changes
        .iter()
        .map(|sc| (sc.unit().name(), sc))
        .collect();
    let carried: BTreeMap<&UnitName, UnitAction> = passed
        .iter()
        .map(|(_, planned, _, _)| (&planned.name, planned.action))
        .collect();
    let mut also_declined = Vec::new();
    for (sc, planned, advisory, notes) in &passed {
        match rule_7_also(snap, staged, sc, planned.action, &rows, &carried) {
            Ok(()) => {
                plan.advisories.extend(advisory.clone());
                plan.instance_notes.extend(
                    notes
                        .iter()
                        .map(|note| (planned.name.clone(), note.clone())),
                );
                if planned.action == UnitAction::Enable
                    && planned.name.instance().is_some()
                    && let Some(sentence) = data_advisory(&planned.name, &sc.data_paths)
                {
                    plan.data_advisories.push((planned.name.clone(), sentence));
                }
                plan.planned.push(PlannedUnit {
                    name: planned.name.clone(),
                    action: planned.action,
                });
            }
            Err(reason) => also_declined.push((planned.name.clone(), planned.action, reason)),
        }
    }
    plan.declined.extend(also_declined);
    plan_default_target(snap, staged, presence.as_ref(), &mut plan);
    plan.mount_advisories = units
        .mount_findings
        .iter()
        .map(|finding| mount_advisory_lines(finding, staged, &units.drop_ins))
        .collect();
    let excluded = excluded_install_files(units, staged);
    let advise = |unit: &UnitName, files: &[String], lines: Vec<String>| match excluded
        .get(unit)
        .filter(|_| !files.is_empty())
    {
        Some(paths) => FindingAdvisory {
            lines: vec![install_omitted_line(unit, paths)],
            not_applied: paths.clone(),
        },
        None => FindingAdvisory {
            lines,
            not_applied: Vec::new(),
        },
    };
    plan.link_advisories = units
        .manual_links
        .iter()
        .map(|link| {
            advise(
                compared_unit(link),
                &link.install_files,
                manual_link_lines(link, staged),
            )
        })
        .collect();
    plan.undetermined_advisories = units
        .undetermined_installs
        .iter()
        .map(|record| {
            advise(
                &record.unit,
                &record.install_files,
                undetermined_install_lines(record),
            )
        })
        .collect();
    plan
}

/// The included default target (§ 10.5), held to rule 5 as any row is:
/// the file half ([`rule_5_file`]), then, for a `Vendor` target, the
/// package half (the presence tiers). `set-default` reads no `[Install]`,
/// so rule 5's drop-in half has nothing to compare, and no other rule
/// applies to it. A target that passes is `default_target`; one that fails
/// is `default_target_declined` with the reason.
fn plan_default_target(
    snap: &InspectionSnapshot,
    staged: &StagedWrites<'_>,
    presence: Option<&Presence<'_>>,
    plan: &mut UnitPlan,
) {
    let Some(default) = snap
        .units
        .as_ref()
        .and_then(|units| units.default_target.as_ref())
        .filter(|default| default.disposition.is_included())
    else {
        return;
    };
    let name = default.target.name().clone();
    let unit = CarriedUnit::from(default.target.clone());
    if let Err(reason) = rule_5_file(snap, &unit, staged) {
        plan.default_target_declined = Some((name, reason));
        return;
    }
    match presence
        .filter(|_| needs_its_package(&unit))
        .map(|p| p.decide(&unit))
    {
        Some(PresenceDecision::Omit(reason)) => {
            plan.default_target_declined = Some((name, reason));
            return;
        }
        Some(PresenceDecision::Emit {
            advisory_reasons: Some((owning_package, reasons)),
        }) => plan.advisories.push(ServiceAdvisory {
            unit: name.as_unit_name().clone(),
            owning_package,
            reasons,
        }),
        Some(PresenceDecision::Emit {
            advisory_reasons: None,
        })
        | None => {}
    }
    plan.default_target = Some(PlannedTarget { name });
}

// ---------------------------------------------------------------------------
// Rule 7, `Also=` members
// ---------------------------------------------------------------------------

/// Rule 7 (§ 10.2), for a row that passed rules 1 to 6: an `enable` or
/// `disable` whose `also` list is not empty is never carried in a fleet
/// aggregate ([`AlsoWhy::FleetAggregate`], naming every member), and on a
/// single host is carried only when three conditions hold, the first that
/// fails giving the [`AlsoWhy`]. `rows` is every state change by name, and
/// `carried` every row that passed rules 1 to 6, with its verb.
///
/// 1. Every member's links match the verb: `Installed` or `Inert` for
///    `enable`, `Unlinked` or `Inert` for `disable`.
/// 2. A member with a row is carried with the same verb; a member with no
///    row is not an instance under `enable` nor a protected unit under
///    `disable`.
/// 3. Neither the unit nor any member has an install file in the snapshot
///    ([`install_file_in_snapshot`]), staged or excluded.
///
/// The reason names the first member, in `also` order, that fails the
/// condition, and lists every member failing it: one group per link value
/// or cause, in `also` order within a group, the first member's group
/// first.
fn rule_7_also(
    snap: &InspectionSnapshot,
    staged: &StagedWrites<'_>,
    sc: &UnitStateChange,
    action: UnitAction,
    rows: &BTreeMap<&UnitName, &UnitStateChange>,
    carried: &BTreeMap<&UnitName, UnitAction>,
) -> Result<(), OmissionReason> {
    if action == UnitAction::Mask || sc.also.is_empty() {
        return Ok(());
    }
    let declined = |member: &UnitName,
                    why: AlsoWhy,
                    named: Vec<UnitName>,
                    further: Option<Box<(AlsoWhy, Vec<UnitName>)>>,
                    add: Vec<UnitName>| {
        OmissionReason::AlsoMember {
            member: member.clone(),
            why,
            verb: action,
            named,
            further,
            add,
        }
    };
    // The members failing a condition, grouped by `fails`' reason, the
    // group of the first failing member first; that member is the reason's
    // `member`. A condition fails at most two ways, so there are at most
    // two groups.
    let failing = |fails: &dyn Fn(&AlsoMember) -> Option<AlsoWhy>, add: Vec<UnitName>| {
        let mut groups: Vec<(AlsoWhy, Vec<UnitName>)> = Vec::new();
        for m in &sc.also {
            if let Some(why) = fails(m) {
                match groups.iter_mut().find(|(seen, _)| *seen == why) {
                    Some((_, names)) => names.push(m.unit.clone()),
                    None => groups.push((why, vec![m.unit.clone()])),
                }
            }
        }
        let mut groups = groups.into_iter();
        if let Some((why, named)) = groups.next()
            && let Some(member) = named.first().cloned()
        {
            return Err(declined(
                &member,
                why,
                named,
                groups.next().map(Box::new),
                add,
            ));
        }
        Ok(())
    };
    // A fleet aggregate is always an advisory (§ 12): its evidence may lack
    // another host's install files, so no condition can be established.
    if snap.aggregate_meta.is_some()
        && let Some(first) = sc.also.first()
    {
        let every = sc.also.iter().map(|m| m.unit.clone()).collect();
        return Err(declined(
            &first.unit,
            AlsoWhy::FleetAggregate,
            every,
            None,
            Vec::new(),
        ));
    }
    let (matching, opposite) = match action {
        UnitAction::Enable => (MemberLinks::Installed, MemberLinks::Unlinked),
        UnitAction::Disable | UnitAction::Mask => (MemberLinks::Unlinked, MemberLinks::Installed),
    };
    let members_where = |test: &dyn Fn(&AlsoMember) -> bool| -> Vec<UnitName> {
        sc.also
            .iter()
            .filter(|m| test(m))
            .map(|m| m.unit.clone())
            .collect()
    };

    // Condition 1.
    failing(
        &|m| {
            (m.links != matching && m.links != MemberLinks::Inert)
                .then_some(AlsoWhy::LinksDiffer(m.links))
        },
        members_where(&|m| m.links == opposite),
    )?;

    // Condition 2.
    let own_rule = |m: &AlsoMember| -> Option<AlsoWhy> {
        if rows.contains_key(&m.unit) {
            return (carried.get(&m.unit) != Some(&action)).then_some(AlsoWhy::RowNotCarried);
        }
        let needs_row = match action {
            UnitAction::Enable => m.unit.instance().is_some(),
            UnitAction::Disable | UnitAction::Mask => target_needs_unit(m.unit.as_str(), action),
        };
        needs_row.then_some(AlsoWhy::NeedsOwnRow)
    };
    failing(&own_rule, Vec::new())?;

    // Condition 3.
    let own = install_file_in_snapshot(snap, staged, sc.unit().name(), &sc.vendor_drop_ins)
        .map(|path| (sc.unit().name(), &sc.also[0].unit, path));
    let file = own.or_else(|| {
        sc.also.iter().find_map(|m| {
            let mut vendor = m.vendor_drop_ins.clone();
            if let Some(row) = rows.get(&m.unit) {
                vendor.extend(row.vendor_drop_ins.iter().cloned());
            }
            install_file_in_snapshot(snap, staged, &m.unit, &vendor)
                .map(|path| (&m.unit, &m.unit, path))
        })
    });
    if let Some((of, member, path)) = file {
        return Err(declined(
            member,
            AlsoWhy::InstallFile { path },
            vec![of.clone()],
            None,
            Vec::new(),
        ));
    }
    Ok(())
}

/// The first file the snapshot holds, staged or excluded, that is an
/// install file for `name` (§ 10.2 "Install files"): a unit file at
/// `/etc/systemd/system/<name>` or, for an instance, at its template's
/// name; or a drop-in systemd reads ([`systemd_reads_drop_in`]) directly
/// in one of [`install_drop_in_dirs`] that holds an `[Install]` section or
/// has one of `vendor_drop_ins`' file names. Config entries, then drop-in rows, then the files the export
/// writes, each by [`Destination`]; a path that does not parse is none.
fn install_file_in_snapshot(
    snap: &InspectionSnapshot,
    staged: &StagedWrites<'_>,
    name: &UnitName,
    vendor_drop_ins: &[String],
) -> Option<String> {
    let template = name.instance().map(|parts| parts.template());
    let unit_files: Vec<Destination> = std::iter::once(etc_unit_path(name))
        .chain(template.map(|t| format!("{ETC_UNIT_DIR}/{}", t.as_str())))
        .filter_map(|path| Destination::new(&path).ok())
        .collect();
    let dirs: Vec<Destination> = install_drop_in_dirs(name)
        .iter()
        .filter_map(|dir| Destination::new(dir).ok())
        .collect();
    let is_install_file = |path: &str, content: &str| {
        let Ok(dest) = Destination::new(path) else {
            return false;
        };
        unit_files.contains(&dest)
            || dirs.iter().any(|dir| {
                dest.as_str()
                    .strip_prefix(dir.as_str())
                    .and_then(|rest| rest.strip_prefix('/'))
                    .is_some_and(|file| {
                        !file.contains('/')
                            && systemd_reads_drop_in(file)
                            && (holds_install_section(content)
                                || vendor_drop_ins.iter().any(|vendor| vendor == file))
                    })
            })
    };
    let config = snap
        .config
        .iter()
        .flat_map(|c| &c.files)
        .map(|f| (f.path.as_str(), f.content.as_str()));
    let drop_ins = snap
        .units
        .iter()
        .flat_map(|u| &u.drop_ins)
        .map(|d| (d.path.as_str(), d.content.as_str()));
    let writes = staged
        .writes
        .iter()
        .map(|(dest, write)| (dest.as_str(), write.content.as_ref()));
    config
        .chain(drop_ins)
        .chain(writes)
        .find(|(path, content)| is_install_file(path, content))
        .map(|(path, _)| path.to_owned())
}

/// The unit whose `[Install]` a link's `install_files` belong to: `unit`
/// for a link, `parent` for relation `Also` (§ 6.6).
fn compared_unit(link: &ManualLink) -> &UnitName {
    match link.relation {
        LinkRelation::Also => &link.parent,
        LinkRelation::Wants
        | LinkRelation::Requires
        | LinkRelation::Upholds
        | LinkRelation::Alias => &link.unit,
    }
}

/// For each unit with an install-difference link or an undetermined
/// install record, the install files those name that `staged` does not
/// write, in first-named order: one that does not parse as a
/// [`Destination`] counts as not staged (§ 6.7). Units with none are left
/// out.
fn excluded_install_files(
    units: &inspectah_core::types::units::UnitSection,
    staged: &StagedWrites<'_>,
) -> BTreeMap<UnitName, Vec<String>> {
    let mut excluded: BTreeMap<UnitName, Vec<String>> = BTreeMap::new();
    let named = units
        .manual_links
        .iter()
        .map(|link| (compared_unit(link), &link.install_files))
        .chain(
            units
                .undetermined_installs
                .iter()
                .map(|record| (&record.unit, &record.install_files)),
        );
    for (unit, files) in named {
        for path in files.iter().filter(|path| !is_staged(staged, path)) {
            let paths = excluded.entry(unit.clone()).or_default();
            if !paths.contains(path) {
                paths.push(path.clone());
            }
        }
    }
    excluded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flatpak_service_constant_is_a_unit_name() {
        assert_eq!(flatpak_service_unit().as_str(), FLATPAK_SERVICE);
    }
}
