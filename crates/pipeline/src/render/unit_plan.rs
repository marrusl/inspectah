//! The unit plan (§ 10.1 and § 10.2 of the units spec): which unit-state
//! `systemctl` commands the export carries, checked against the files the
//! stager writes, and the lines that write them. The package presence tiers
//! it uses, and the package installability helpers the package sections
//! share, live here too.

use inspectah_core::baseline::{
    TARGET_NEEDS_UNIT, TMP_MOUNT, is_runtime_updater, target_needs_unit, target_needs_unit_text,
};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::units::{
    AdminStorageFragment, CarriedUnit, MountUnitFinding, StorageKind, SystemdDropIn, UnitAction,
    UnitFragment, UnitName, UnitState, UnitStateChange,
};

use inspectah_core::types::verified_upload::VerifiedUploads;

use super::comment::escape_comment_text;
use super::configtree::{Destination, ETC_UNIT_DIR, PathError, StagedWrites, is_staged};
use super::not_carried::{NOT_CARRIED_REASON, not_carried_packages};
use super::repo_enabling::{RepoEnablingExport, repo_enabling_export};
use super::repoless::{PackageRoute, route_for};
use super::safety::single_quote;
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
    /// A mask and a staged unit file claim one path: `systemctl mask`
    /// exits 1 on a regular file there (§ 10.2 rule 4).
    MaskPathOccupied { path: String },
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
            Self::MaskPathOccupied { path } => {
                format!("the export also carries a unit file at {path}; exclude one of the two")
            }
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
            Self::MaskPathOccupied { .. } => "MaskPathOccupied",
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
            | Self::MaskPathOccupied { .. } => None,
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
/// let _ = systemctl_lines(&[name]);
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

/// What [`plan_units`] decided for every included state change.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct UnitPlan {
    /// The commands the Containerfile writes.
    pub planned: Vec<PlannedUnit>,
    /// Every command the plan did not write, and why.
    pub declined: Vec<(UnitName, UnitAction, OmissionReason)>,
    /// Planned units whose package's presence could not be proved.
    pub advisories: Vec<ServiceAdvisory>,
    /// Enables left to the Scheduled Tasks section, which copies and
    /// enables the timers it carries (§ 8).
    pub deferred: Vec<UnitName>,
    /// Excluded rows of runtime package updaters (§ 10.4). Not planned, as
    /// no excluded row is, and not declined: the row's exclusion decided
    /// it. The section names each in [`excluded_updater_line`].
    pub excluded_updaters: Vec<UnitName>,
    /// § 10.6's comment lines, one block per mount finding
    /// ([`mount_advisory_lines`]), unescaped. Not commands: the section
    /// writes them after its `systemctl` lines (§ 10.1).
    pub mount_advisories: Vec<Vec<String>>,
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

/// One `RUN systemctl <verb> --` per verb, enable then disable then mask,
/// each unit single-quoted on its own continuation line, ordered by kind
/// and then by name (§ 10.1). Takes only [`PlannedUnit`]s, which only
/// [`plan_units`] builds.
///
/// [`single_quote`] is total on a [`UnitName`]: its character set holds no
/// quote and no line break. `--` ends the options, so a name that starts
/// with `-` (`-.slice`) is an operand.
pub fn systemctl_lines(units: &[PlannedUnit]) -> Vec<String> {
    let mut lines = Vec::new();
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
}

impl DeclinedMarker {
    /// The comment prefix, with its trailing space.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Omitted => "# Omitted: ",
            Self::Fixme => "# FIXME: ",
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
/// ([`target_needs_unit_text`]); every other reason is an `# Omitted:` line
/// naming the unit, then [`OmissionReason::describe`] in parentheses.
pub fn declined_line(name: &UnitName, action: UnitAction, reason: &OmissionReason) -> DeclinedLine {
    match reason {
        OmissionReason::TargetNeedsUnit => DeclinedLine {
            marker: DeclinedMarker::Fixme,
            text: target_needs_unit_text(name.as_str(), action),
        },
        _ => DeclinedLine {
            marker: DeclinedMarker::Omitted,
            text: format!("{} ({})", name.as_str(), reason.describe()),
        },
    }
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
/// declined command ([`declined_line`]), a line per excluded runtime
/// updater ([`excluded_updater_line`]), then [`systemctl_lines`], then the
/// note naming the enables left to the Scheduled Tasks section, then the
/// comment-only mount advisory blocks ([`UnitPlan::mount_advisories`]),
/// one after another. Each block starts with its `# Advisory:` line, so
/// no line separates them.
pub fn unit_state_lines(plan: &UnitPlan) -> Vec<String> {
    // The reason may name a package from the snapshot, so the finished
    // line is escaped here rather than inside `declined_line`, which refine
    // also calls and escapes for its own surface.
    let mut lines: Vec<String> = plan
        .declined
        .iter()
        .map(|(name, action, reason)| {
            escape_comment_text(&declined_line(name, *action, reason).comment())
        })
        .collect();
    lines.extend(
        plan.excluded_updaters
            .iter()
            .map(|name| escape_comment_text(&excluded_updater_line(name).comment())),
    );
    lines.extend(systemctl_lines(&plan.planned));
    if !plan.deferred.is_empty() {
        let names: Vec<&str> = plan.deferred.iter().map(UnitName::as_str).collect();
        lines.push(format!(
            "# {} unit(s) deferred to Scheduled Tasks section: {}",
            names.len(),
            names.join(", ")
        ));
    }
    // A mount point comes from a host file: escaped here, as above.
    lines.extend(
        plan.mount_advisories
            .iter()
            .flatten()
            .map(|line| escape_comment_text(line)),
    );
    lines
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

/// A mount or swap with `nofail` absent.
const MOUNT_NO_NOFAIL: &str = "#   No nofail option. If the unit is started at boot and its \
     device is missing, units that require it fail, and boot can stop in emergency mode.";

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
/// - The file line, when `staged` writes the unit's file at
///   `/etc/systemd/system/<unit>` ([`is_staged`]), so it follows the
///   config row's toggle.
/// - From `mount_point`, when there is one and the unit is not masked:
///   under `/usr`; one of [`BOOTC_OWNED_MOUNT_POINTS`]; under one of
///   [`BASE_VAR_SYMLINKS`] (naming that top-level directory); under one of
///   [`BASE_IMAGE_DIRECTORIES`]. Each is matched against the mount point in
///   normal form ([`normal_mount_point`]), and a line that shows the path
///   shows that form. A masked unit does not mount, so the layout of its
///   mount point does not bear on it.
/// - The `tmp.mount` line, when that unit's file is staged.
/// - The `nofail` line, for a mount or swap whose `nofail` is false, when
///   the unit is not masked, its file was read, and `drop_ins` holds no row
///   for the unit. A mask with no unit file has no `Options=` to read. A
///   mount unit with no `mount_point` was not read (or has no absolute
///   `Where=` in its `[Mount]` section; the finding cannot tell the two
///   apart), so it gets no line. A swap has no `mount_point` either way, so
///   an unreadable swap file still gets the line: telling it apart would
///   need a field the finding does not have. The collector reads
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
            lines.push(format!(
                "#   {top} is a symlink into /var in the RHEL bootc base image. The unit must \
                 be renamed for the resolved path (for /srv: var-srv.mount with \
                 Where=/var/srv)."
            ));
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
    let nofail_read = match name.kind() {
        StorageKind::Mount => finding.mount_point.is_some(),
        StorageKind::Swap => *finding.unit.fragment() != AdminStorageFragment::MaskOnly,
        StorageKind::Automount => false,
    };
    // The collector reads `Options=` from the unit file alone. With a
    // drop-in row for the unit, a drop-in may set `nofail`, so the finding's
    // settings are not known to be the effective ones.
    let has_drop_ins = drop_ins.iter().any(|d| d.unit == unit.as_str());
    if nofail_read && !masked && !finding.nofail && !has_drop_ins {
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
///   else `InstallEvidenceChanged` with that path.
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
            one_variant(snap, path)
        }
        UnitFragment::Shadowed { file, .. } => {
            let path = etc_unit_path(file);
            if !is_staged(staged, &path) {
                return Err(OmissionReason::InstallEvidenceChanged { path });
            }
            one_variant(snap, path)
        }
        UnitFragment::Vendor { .. } | UnitFragment::Absent => {
            no_file_at(etc_unit_path(unit.name()), staged)
        }
    }
}

/// Whether rule 5 asks the presence tiers about the unit's package
/// (§ 10.2): only for a unit whose image file comes from that package, a
/// `Vendor` unit or a package-owned storage unit. A `Shadowed` unit's one
/// condition is its staged shadow, which the image loads with or without
/// the package; a `Local` unit has no owner; `Absent` is reached only by a
/// mask, which rule 5 does not apply to.
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
/// selected.
fn one_variant(snap: &InspectionSnapshot, path: String) -> Result<(), OmissionReason> {
    if snap.aggregate_meta.is_none() {
        return Ok(());
    }
    let Ok(dest) = Destination::new(&path) else {
        return Ok(());
    };
    let at_dest = |p: &str| Destination::new(p).is_ok_and(|d| d == dest);
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
    if contents.len() > 1 {
        Err(OmissionReason::InstallEvidenceChanged { path })
    } else {
        Ok(())
    }
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
    sc: &UnitStateChange,
    rpm: &RpmSection,
    target_packages: &std::collections::BTreeSet<String>,
    not_carried: &std::collections::HashSet<String>,
    baseline_unavailable: bool,
) -> PresenceDecision {
    let pkg_name = match sc.unit().owning_package() {
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
    sc: &UnitStateChange,
    rpm: &RpmSection,
    target_packages: &std::collections::BTreeSet<String>,
    not_carried: &std::collections::HashSet<String>,
) -> Option<PresenceDecision> {
    let pkg_name = sc.unit().owning_package()?;
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
    sc: &UnitStateChange,
    rpm: &RpmSection,
    target_packages: &std::collections::BTreeSet<String>,
    not_carried: &std::collections::HashSet<String>,
) -> PresenceDecision {
    excluded_owner(sc, rpm, target_packages, not_carried).unwrap_or(PresenceDecision::Emit {
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

    fn decide(&self, sc: &UnitStateChange) -> PresenceDecision {
        if self.is_aggregate {
            aggregate_presence(sc, self.rpm, &self.target_packages, &self.not_carried)
        } else {
            classify_service_presence(
                sc,
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
///    free of a staged unit file. A `mask` that passes is planned: it reads
///    no `[Install]` and succeeds on an absent unit, so rule 5 does not
///    apply to it.
/// 4. Rule 5, the row's own file ([`rule_5_file`]), `enable` and
///    `disable`: the export stages the unit file the row recorded and no
///    other.
/// 5. Rule 5, the package, for a `Vendor` or package-owned storage unit
///    only ([`needs_its_package`]): the presence tiers
///    ([`classify_service_presence`], unchanged; an aggregate applies Tier 2
///    alone). With no package section there is nothing to read and the unit
///    is emitted: there is no separate path without one.
/// 6. An `enable` of a unit the Scheduled Tasks section carries is left to
///    that section and recorded in `deferred`.
///
/// Every other command is planned. Each mount finding gets its § 10.6
/// advisory lines against `staged`, in `mount_advisories`.
pub fn plan_units(snap: &InspectionSnapshot, staged: &StagedWrites<'_>) -> UnitPlan {
    let mut plan = UnitPlan::default();
    let Some(units) = &snap.units else {
        return plan;
    };
    let ct_units = config_tree_units(snap);
    let presence = snap.rpm.as_ref().map(|rpm| Presence::new(snap, rpm));

    plan.excluded_updaters = units
        .state_changes
        .iter()
        .filter(|sc| !sc.disposition.is_included())
        .map(|sc| sc.unit().name())
        .filter(|name| is_runtime_updater(name.as_str()))
        .cloned()
        .collect();
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
        if action != UnitAction::Mask {
            if let Err(reason) = rule_5_file(snap, sc.unit(), staged) {
                plan.declined.push((name, action, reason));
                continue;
            }
            let decision = presence
                .as_ref()
                .filter(|_| needs_its_package(sc.unit()))
                .map(|p| p.decide(sc));
            match decision {
                Some(PresenceDecision::Omit(reason)) => {
                    plan.declined.push((name, action, reason));
                    continue;
                }
                Some(PresenceDecision::Emit {
                    advisory_reasons: Some((owning_package, reasons)),
                }) => plan.advisories.push(ServiceAdvisory {
                    unit: name.clone(),
                    owning_package,
                    reasons,
                }),
                Some(PresenceDecision::Emit {
                    advisory_reasons: None,
                })
                | None => {}
            }
            if action == UnitAction::Enable && ct_units.contains(name.as_str()) {
                plan.deferred.push(name);
                continue;
            }
        }
        plan.planned.push(PlannedUnit { name, action });
    }
    plan.mount_advisories = units
        .mount_findings
        .iter()
        .map(|finding| mount_advisory_lines(finding, staged, &units.drop_ins))
        .collect();
    plan
}
