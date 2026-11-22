//! The not-carried class: auto packages that nothing the image installs
//! needs.
//!
//! An added package is in the class when it is in `auto_packages` (not
//! user-installed on the source host) and no leaf or carried unit owner
//! the export installs pulls it in, as `pulled_in_by_default_roots`
//! answers from the recorded dependency closures. An included package the
//! export renders only as a manual step pulls nothing in, since the image
//! never installs it. Identity is
//! `name.arch` throughout. [`classify_auto_carriage`] is the one decider:
//! no function outside this module decides membership, and every other
//! function here reaches the class through it.
//!
//! Some packages keep their existing handling and are never in the class:
//! a locked package, one with `source_repo == "anaconda"` (refine's
//! anaconda classifier decides those), one whose name is in
//! `baseline_package_names`, one with `pkg.uploaded` set, and one that
//! routes as [`PackageRoute::Repoless`].
//!
//! **Fallback.** [`classify_auto_carriage`] returns `None`, and so
//! nothing is excluded and no list is emitted, when the snapshot cannot
//! support the judgement: `leaf_packages` or `auto_packages` is `None`
//! (the collector's unavailable or degraded state), `leaf_dep_tree` is
//! not a JSON object, the tree is an empty object while `auto_packages`
//! is not empty (reading that as "no leaf needs anything" would put every
//! auto package in the class), or the snapshot is an aggregate, whose
//! `packages_added` is leaf-only.
//!
//! **Unit owners are carried by default.** An auto package that owns a
//! unit enabled on the source host is in [`default_carried_unit_owners`],
//! whether or not something pulls it in and whether or not the tree can
//! support the class: the scan and refine both keep it included, and the
//! Containerfile names it on the plain install line although it is not a
//! leaf. Its own recorded closure (`dep_closures`) then counts as pulled
//! in, as a leaf's does. In the class it is [`AutoCarriage::UnitOwner`].
//!
//! **Except one that needs a repo the image does not enable.** The plain
//! install line enables no disabled repo, and nothing routes an auto
//! package's dependencies into the transaction that does
//! (`unit_plan::leaf_tree_pulled_in`). So an owner that comes from a repo
//! the host has disabled, or that may pull in a package only such a repo
//! or no repo at all serves, is not carried by default ([`owner_hold`]):
//! it is [`AutoCarriage::HeldOwner`], excluded by default like an
//! [`AutoCarriage::Unneeded`] package, and listed with its reason. So is
//! one whose closure may need a package the host did not have, and one
//! that may pull in a package from a repo this export does not carry,
//! which follows refine's repo selection ([`hold_owners_losing_a_repo`]).
//! The operator can include it in refine; it then installs as any auto
//! package the operator includes does. "May pull in" is its whole recorded
//! closure, every provider and branch included, and a requirement the
//! collector could not read counts as pulling in every added package.
//! Each of the four kinds, once excluded (by the default or by an
//! operator), is listed as not carried.
//!
//! **A fleet keeps only leaves.** The aggregate merge drops every package
//! that is not a leaf on every host, so the fleet export cannot carry an
//! auto package at all. [`fleet_not_carried_packages`] names the ones it
//! drops that own a unit enabled on any host, so the fleet export gives
//! the same notice, block and table as a single host.
//!
//! **Defaults and presence count different roots.** The class, and so
//! the scan's default exclusion, counts the closures of the installed
//! leaves and carried unit owners only: the scan decides it while every
//! auto package is still included. What is listed as not carried counts
//! everything a package the export installs may pull in
//! ([`possibly_pulled_in_by_included`]), an auto package an operator
//! includes by hand among them, so a dependency dnf may install with it is
//! not listed. An auto package that only another not-carried package
//! needs is in the class too, which is correct: nothing the export
//! installs pulls either of them in.
//!
//! **Only certain absence is listed.** A package is listed only when the
//! export certainly leaves it out: excluded, and outside both maps of
//! every package the export installs or may install (`dep_closures` and
//! `dep_alternatives`). One dnf may install, such as an excluded provider
//! of a capability another package provides too, is not listed, and its
//! unit commands run only if the unit file is in the image
//! (`unit_plan::export_installs`). The class counts every provider a
//! default root may pull in as well, so the scan keeps such a package
//! included as the source host had it. A fleet's drops count them too: a
//! package dnf may install is never called dropped.

use std::collections::{BTreeSet, HashMap, HashSet};

use inspectah_core::baseline::INCOMPATIBLE_SERVICES;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::repo::{RepoTier, effective_definition, parse_repo_sections, repo_tier};
use inspectah_core::types::rpm::{PackageEntry, RpmSection};
use inspectah_core::types::units::{UnitName, UnitState, UnitStateChange};
use inspectah_core::types::verified_upload::VerifiedUploads;

use super::repoless::{InstallRepos, PackageRoute, route_for};
use super::unit_plan::{
    every_installed_package_has_a_closure, is_package_installable, possible_closure,
    possibly_pulled_in_by_included, pulled_in_by_default_roots,
};
use crate::export_warning::{ExportWarning, sanitize_bounded};

/// `source_repo` recorded for a package installed at system install time,
/// which refine's anaconda classifier decides.
const ANACONDA_SOURCE_REPO: &str = "anaconda";

/// Why a not-carried package is left out of the image, as every surface
/// states it: the Containerfile block, the audit table and the service
/// omission reason.
pub const NOT_CARRIED_REASON: &str =
    "not user-installed on the source host, and nothing this image installs needs it";

/// How an added package reaches the target image, for the auto class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoCarriage {
    /// Not this class: a leaf, not in `auto_packages`, or an auto package
    /// that keeps its existing handling (locked, anaconda-sourced, named in
    /// the baseline, uploaded, or routed repo-less).
    NotAuto,
    /// An auto package a leaf or carried unit owner the export installs
    /// pulls in (`pulled_in_by_default_roots`).
    PulledIn,
    /// An auto package nothing included pulls in that owns a unit enabled
    /// on the source host. Carried by default: a firewall or cron daemon
    /// missing from the image surfaces long after the migration.
    UnitOwner,
    /// A [`Self::UnitOwner`] that needs a repo the image does not enable
    /// ([`owner_hold`]). Excluded by default: naming it on the plain
    /// install line would fail the build.
    HeldOwner(OwnerHold),
    /// An auto package nothing included pulls in and that owns no enabled
    /// unit. Excluded by default.
    Unneeded,
}

/// Classify every `packages_added` entry, keyed by `name.arch`.
///
/// `None` when the snapshot cannot support the judgement (see the module
/// documentation's fallback). Takes the whole snapshot so the aggregate
/// guard lives here and nowhere else.
///
/// An upload is read from the package's `uploaded` flag, which refine sets
/// when it stages one, because the verified uploads only exist inside an
/// export. The route is taken with no verified uploads, the scan-time view,
/// so it can never be [`PackageRoute::Upload`] here and is checked for
/// [`PackageRoute::Repoless`] only: a repo-less package with no disabled
/// repo to install from.
pub fn classify_auto_carriage(snap: &InspectionSnapshot) -> Option<HashMap<String, AutoCarriage>> {
    if snap.aggregate_meta.is_some() {
        return None;
    }
    let rpm = snap.rpm.as_ref()?;
    let leaves: HashSet<&str> = rpm
        .leaf_packages
        .as_ref()?
        .iter()
        .map(String::as_str)
        .collect();
    let auto: HashSet<&str> = rpm
        .auto_packages
        .as_ref()?
        .iter()
        .map(String::as_str)
        .collect();
    let tree = rpm.leaf_dep_tree.as_object()?;
    if tree.is_empty() && !auto.is_empty() {
        return None;
    }

    let repos = InstallRepos::of(snap);
    let pulled_in = pulled_in_by_default_roots(snap, &repos);
    let existing = ExistingHandling::of(rpm, &repos);
    let unit_owners = unit_owner_holds(snap);

    let classes = rpm
        .packages_added
        .iter()
        .map(|pkg| {
            let id = format!("{}.{}", pkg.name, pkg.arch);
            let class = if leaves.contains(id.as_str()) || !auto.contains(id.as_str()) {
                AutoCarriage::NotAuto
            } else if pulled_in.contains(&id) {
                AutoCarriage::PulledIn
            } else if existing.keeps(pkg) {
                AutoCarriage::NotAuto
            } else if let Some(hold) = unit_owners.get(&id) {
                hold.map_or(AutoCarriage::UnitOwner, AutoCarriage::HeldOwner)
            } else {
                AutoCarriage::Unneeded
            };
            (id, class)
        })
        .collect();
    Some(classes)
}

/// The packages that keep their existing handling and are never in the
/// class (see the module documentation), whatever their dependencies.
struct ExistingHandling<'a> {
    rpm: &'a RpmSection,
    repos: &'a InstallRepos,
    baseline: HashSet<&'a str>,
    no_uploads: VerifiedUploads,
}

impl<'a> ExistingHandling<'a> {
    fn of(rpm: &'a RpmSection, repos: &'a InstallRepos) -> Self {
        Self {
            rpm,
            repos,
            baseline: rpm
                .baseline_package_names
                .iter()
                .flatten()
                .map(String::as_str)
                .collect(),
            no_uploads: VerifiedUploads::new(),
        }
    }

    /// Locked, anaconda-sourced, named in the baseline, uploaded, or routed
    /// repo-less.
    fn keeps(&self, pkg: &PackageEntry) -> bool {
        pkg.locked
            || pkg.source_repo == ANACONDA_SOURCE_REPO
            || self.baseline.contains(pkg.name.as_str())
            || pkg.uploaded
            || route_for(self.rpm, self.repos, pkg, &self.no_uploads) == PackageRoute::Repoless
    }
}

/// Exclude every [`AutoCarriage::Unneeded`] and
/// [`AutoCarriage::HeldOwner`] package from a scan snapshot.
///
/// This is the scan's default disposition for the class; every other
/// package, an [`AutoCarriage::UnitOwner`] included, is left as it is,
/// and under the fallback nothing changes.
/// Idempotent: only leaves and carried unit owners contribute to
/// `pulled_in_by_default_roots`, and an excluded package is neither, so
/// excluding one cannot change the class.
pub fn apply_scan_package_defaults(snap: &mut InspectionSnapshot) {
    let Some(classes) = classify_auto_carriage(snap) else {
        return;
    };
    let Some(rpm) = snap.rpm.as_mut() else {
        return;
    };
    for pkg in &mut rpm.packages_added {
        let id = format!("{}.{}", pkg.name, pkg.arch);
        if matches!(
            classes.get(&id),
            Some(AutoCarriage::Unneeded | AutoCarriage::HeldOwner(_))
        ) {
            pkg.disposition = FindingKind::excluded();
        }
    }
}

/// An excluded [`AutoCarriage::Unneeded`], [`AutoCarriage::UnitOwner`] or
/// [`AutoCarriage::HeldOwner`] package, or a package a fleet drops
/// ([`fleet_not_carried_packages`]), as every surface shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotCarriedPackage {
    /// Package name.
    pub name: String,
    /// Package architecture. Empty for a fleet entry: the merge that
    /// drops the package keeps no record of it but its name.
    pub arch: String,
    /// Repository the package was installed from. Empty for a fleet entry,
    /// as `arch` is.
    pub source_repo: String,
    /// Enabled units this package owns (`enabled_units_by_owner`).
    pub enabled_units: Vec<String>,
    /// Why this unit owner is not carried by default
    /// ([`AutoCarriage::HeldOwner`]); `None` for every other entry.
    pub hold: Option<OwnerHold>,
    /// Another arch of this name is in the same list.
    arch_qualified: bool,
}

impl NotCarriedPackage {
    /// The name both surfaces print: `name`, or `name.arch` when the list
    /// holds more than one arch of this name.
    pub fn display_name(&self) -> String {
        if self.arch_qualified {
            format!("{}.{}", self.name, self.arch)
        } else {
            self.name.clone()
        }
    }

    /// Whether this package owns an enabled unit: the one test every
    /// surface uses to put a package first and to name it in
    /// [`not_carried_notice`].
    pub fn owns_enabled_unit(&self) -> bool {
        !self.enabled_units.is_empty()
    }

    /// The reason every surface gives for this entry: [`NOT_CARRIED_REASON`],
    /// followed by the [`OwnerHold`] reason for a held unit owner.
    pub fn reason(&self) -> String {
        match self.hold {
            Some(hold) => format!("{NOT_CARRIED_REASON}; {}", hold.reason()),
            None => NOT_CARRIED_REASON.to_string(),
        }
    }
}

/// Why an auto package that owns a unit enabled on the source host is not
/// carried by default ([`owner_hold`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerHold {
    /// It, or a package installing it may pull in, comes from a repo the
    /// host has disabled. The plain install line does not enable it.
    DisabledRepo,
    /// A package installing it may pull in has no repo the image enables
    /// to install it from by name: a local RPM, or one from a repo the
    /// export does not carry.
    NoRepo,
    /// It, or a package installing it may pull in, has a requirement that
    /// may need a package the host did not have
    /// (`RpmSection::unresolved_requirements`).
    Unresolved,
    /// It, or a package installing it may pull in, was installed from a
    /// repo the host enabled from a captured `.repo` file, and this export
    /// does not carry that repo enabled ([`repos_lost_in_export`]): the
    /// operator excluded it in refine, or its file is not staged.
    RepoExcluded,
}

impl OwnerHold {
    /// The short reason every surface gives.
    pub fn reason(self) -> &'static str {
        match self {
            Self::DisabledRepo => "needs a repo disabled on the host",
            Self::NoRepo => "needs a package no enabled repo serves",
            Self::Unresolved => "may need a package the host did not have",
            Self::RepoExcluded => "needs a package from a repo excluded in this export",
        }
    }
}

/// Directory holding the vendor unit files packages ship.
pub(crate) const VENDOR_UNIT_DIR: &str = "/usr/lib/systemd/system";

/// The single-host list: the excluded [`AutoCarriage::Unneeded`] and
/// [`AutoCarriage::UnitOwner`] packages that nothing the export installs
/// may pull in ([`possibly_pulled_in_by_included`]), in display order: a
/// package is listed only when it is certainly not carried. An auto
/// package an operator included by hand brings its dependencies, so they
/// are not listed although the class still holds them. Refine's view
/// rows and the unit plan read this list; the export's block, table and
/// notice read [`export_not_carried_packages`], which adds the fleet case.
///
/// Packages owning an enabled unit come first, then the rest; each part
/// is sorted by name, then arch. Empty under the [`classify_auto_carriage`]
/// fallback.
///
/// Which enabled units are a package's is `enabled_units_by_owner`'s
/// answer. Ownership is recorded by name, not `name.arch`, so every arch
/// of a name is credited with the same units. The units are for display
/// and ordering only: whether a unit is emitted is the presence rules'
/// decision.
pub fn not_carried_packages(snap: &InspectionSnapshot) -> Vec<NotCarriedPackage> {
    let Some(classes) = classify_auto_carriage(snap) else {
        return Vec::new();
    };
    let Some(rpm) = snap.rpm.as_ref() else {
        return Vec::new();
    };
    let units_by_owner = enabled_units_by_owner(snap);
    let may_pull = possibly_pulled_in_by_included(snap, &InstallRepos::of(snap));

    let mut list: Vec<NotCarriedPackage> = rpm
        .packages_added
        .iter()
        .filter(|pkg| {
            let id = format!("{}.{}", pkg.name, pkg.arch);
            pkg.disposition == FindingKind::excluded()
                && matches!(
                    classes.get(&id),
                    Some(
                        AutoCarriage::Unneeded
                            | AutoCarriage::UnitOwner
                            | AutoCarriage::HeldOwner(_)
                    )
                )
                && !may_pull.contains(&id)
        })
        .map(|pkg| NotCarriedPackage {
            name: pkg.name.clone(),
            arch: pkg.arch.clone(),
            source_repo: pkg.source_repo.clone(),
            enabled_units: units_by_owner
                .get(pkg.name.as_str())
                .map(|units| units.iter().map(|u| (*u).to_string()).collect())
                .unwrap_or_default(),
            hold: match classes.get(&format!("{}.{}", pkg.name, pkg.arch)) {
                Some(AutoCarriage::HeldOwner(hold)) => Some(*hold),
                _ => None,
            },
            arch_qualified: false,
        })
        .collect();

    let mut arches_per_name: HashMap<String, usize> = HashMap::new();
    for pkg in &list {
        *arches_per_name.entry(pkg.name.clone()).or_default() += 1;
    }
    for pkg in &mut list {
        pkg.arch_qualified = arches_per_name[&pkg.name] > 1;
    }

    sort_for_display(&mut list);
    list
}

/// Unit owners first, then the rest; each part by name, then arch.
fn sort_for_display(list: &mut [NotCarriedPackage]) {
    list.sort_by(|a, b| {
        // `false < true`, so unit owners sort first.
        a.enabled_units
            .is_empty()
            .cmp(&b.enabled_units.is_empty())
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.arch.cmp(&b.arch))
    });
}

/// The `name.arch` ids of the auto packages carried by default because
/// they own a unit enabled on the source host: the [`unit_owner_holds`]
/// entries with no [`OwnerHold`].
///
/// Membership is fixed by the snapshot alone. It does not depend on
/// whether something the export installs pulls the package in, so
/// excluding the leaf that does never takes the owner out with it, and it
/// does not depend on the tree fallback, which decides only whether other
/// auto packages can safely be left out. Empty for an aggregate, or when
/// the collector had no leaf data.
///
/// Refine includes one by default and shows it in its package list; the
/// Containerfile names an included one that no installed leaf pulls in on
/// the plain install line, and its recorded closure counts as pulled in
/// (`unit_plan::export_installs`, `pulled_in_by_default_roots`).
pub fn default_carried_unit_owners(snap: &InspectionSnapshot) -> HashSet<String> {
    unit_owner_holds(snap)
        .into_iter()
        .filter(|(_, hold)| hold.is_none())
        .map(|(id, _)| id)
        .collect()
}

/// Every auto package that owns a unit enabled on the source host, by
/// `name.arch` id, with the [`owner_hold`] that keeps it from being
/// carried by default, if any: an auto package, not a leaf, outside the
/// existing handling, that owns an enabled unit. Empty for an aggregate,
/// or when the collector had no leaf data.
fn unit_owner_holds(snap: &InspectionSnapshot) -> HashMap<String, Option<OwnerHold>> {
    if snap.aggregate_meta.is_some() {
        return HashMap::new();
    }
    let Some(rpm) = snap.rpm.as_ref() else {
        return HashMap::new();
    };
    let (Some(leaves), Some(auto)) = (rpm.leaf_packages.as_ref(), rpm.auto_packages.as_ref())
    else {
        return HashMap::new();
    };
    let leaves: HashSet<&str> = leaves.iter().map(String::as_str).collect();
    let auto: HashSet<&str> = auto.iter().map(String::as_str).collect();
    let repos = InstallRepos::of(snap);
    let existing = ExistingHandling::of(rpm, &repos);
    let owners = enabled_units_by_owner(snap);
    let lost = repos_lost_in_export(rpm, &repos);
    let added: HashMap<String, &PackageEntry> = rpm
        .packages_added
        .iter()
        .map(|pkg| (format!("{}.{}", pkg.name, pkg.arch), pkg))
        .collect();
    rpm.packages_added
        .iter()
        .filter(|pkg| owners.contains_key(pkg.name.as_str()) && !existing.keeps(pkg))
        .map(|pkg| (format!("{}.{}", pkg.name, pkg.arch), pkg))
        .filter(|(id, _)| auto.contains(id.as_str()) && !leaves.contains(id.as_str()))
        .map(|(id, pkg)| {
            let hold = owner_hold(rpm, &repos, &lost, &added, pkg, &id);
            (id, hold)
        })
        .collect()
}

/// Why the unit owner `pkg` (`id`) cannot be named on the plain install
/// line, or `None` when it can (Mark Russell, 2026-11-21).
///
/// That line enables no disabled repo, so the owner must install by name
/// from a repo the image enables, and so must every package installing it
/// may pull in ([`possible_closure`]): every provider dnf may pick and
/// every branch the image may take, since dnf fails the transaction when a
/// requirement it settles on cannot be fetched. Every entry of
/// `packages_added` counts, whatever its name: the collector subtracts the
/// base image by `name.arch`, so an entry whose name the base image also
/// has is another architecture, which the base image does not provide.
///
/// The closure names installed packages only. When the owner, or a
/// package in its closure, has a requirement that may need a package the
/// host did not have (`unresolved_requirements`: one no installed package
/// satisfies in some branch, or one the collector could not read), the
/// owner is [`OwnerHold::Unresolved`] (Mark Russell, 2026-11-21).
///
/// When the owner, or a package in its closure, was installed from a repo
/// this export does not carry enabled ([`repos_lost_in_export`]), the
/// owner is [`OwnerHold::RepoExcluded`] (Mark Russell, 2026-11-21). The
/// export decides it, so it follows refine's current selection: excluding
/// a custom repo holds every owner that may pull in one of its packages,
/// as one operand of an `or` among them, and including the repo again
/// carries them. It wins over the
/// others: until the repo is back, including the owner cannot make it
/// installable. [`OwnerHold::DisabledRepo`] comes next: it is the one the
/// operator can act on by including the owner in refine.
///
/// A snapshot with no recorded closure for the owner, a hand-built one,
/// is checked on the owner alone.
fn owner_hold(
    rpm: &RpmSection,
    repos: &InstallRepos,
    lost: &HashSet<String>,
    added: &HashMap<String, &PackageEntry>,
    pkg: &PackageEntry,
    id: &str,
) -> Option<OwnerHold> {
    let no_uploads = VerifiedUploads::new();
    let closure = possible_closure(rpm, id);
    let loses_its_repo = |entry: &PackageEntry| lost.contains(entry.source_repo.as_str());
    if loses_its_repo(pkg)
        || closure
            .iter()
            .filter_map(|dep| added.get(dep))
            .any(|entry| loses_its_repo(entry))
    {
        return Some(OwnerHold::RepoExcluded);
    }
    if matches!(
        route_for(rpm, repos, pkg, &no_uploads),
        PackageRoute::DisabledRepo(_)
    ) {
        return Some(OwnerHold::DisabledRepo);
    }
    let unresolved: HashSet<&str> = rpm
        .unresolved_requirements
        .iter()
        .flatten()
        .map(String::as_str)
        .collect();
    let mut hold = (unresolved.contains(id)
        || closure.iter().any(|dep| unresolved.contains(dep.as_str())))
    .then_some(OwnerHold::Unresolved);
    for dep in &closure {
        let Some(entry) = added.get(dep) else {
            continue;
        };
        match route_for(rpm, repos, entry, &no_uploads) {
            PackageRoute::DisabledRepo(_) => return Some(OwnerHold::DisabledRepo),
            PackageRoute::ByName if is_package_installable(entry) => {}
            _ => hold = Some(OwnerHold::NoRepo),
        }
    }
    hold
}

/// The repos a package from a captured custom repo cannot install from in
/// this export, by section spelling: each section that a captured `.repo`
/// file other than a default repo file defines, that the source host's
/// dnf had enabled, and whose definition among the files the build has
/// when it installs ([`InstallRepos::effective`]) is missing or disabled.
///
/// A distro repo, and a section a default repo file also defines, is
/// never in it: the base image enables its own. Neither is a section the
/// host had disabled, which [`OwnerHold::DisabledRepo`] covers. A repo is
/// lost when the operator excludes it in refine and its file goes with it,
/// or when the stager withholds the file; a file another included section
/// keeps still carries the repo, and dnf can still fetch from it.
fn repos_lost_in_export(rpm: &RpmSection, repos: &InstallRepos) -> HashSet<String> {
    let host_files = || {
        rpm.repo_files
            .iter()
            .map(|f| (f.path.as_str(), f.content.as_str()))
    };
    let default_ids: HashSet<String> = rpm
        .repo_files
        .iter()
        .filter(|f| f.is_default_repo)
        .flat_map(|f| parse_repo_sections(&f.content))
        .map(|sec| sec.id.as_str().to_owned())
        .collect();
    rpm.repo_files
        .iter()
        .filter(|f| !f.is_default_repo)
        .flat_map(|f| parse_repo_sections(&f.content))
        .map(|sec| sec.id)
        .filter(|id| {
            !default_ids.contains(id.as_str())
                && !matches!(repo_tier(id.as_str()), RepoTier::Distro)
        })
        .filter(|id| effective_definition(host_files(), id).is_some_and(|def| def.enabled))
        .filter(|id| !repos.effective(id).is_some_and(|def| def.enabled))
        .map(|id| id.as_str().to_owned())
        .collect()
}

/// Exclude every included unit owner this export holds because it may
/// pull in a package whose repo the export does not carry
/// ([`OwnerHold::RepoExcluded`]), except the `name.arch` ids in `by_hand`,
/// which the operator included explicitly.
///
/// The scan's defaults exclude such an owner with the other held ones
/// ([`apply_scan_package_defaults`]). Refine sets its defaults when it
/// opens a snapshot, before the operator excludes any repo, so it runs
/// this over every projection: the owner is held while the repo is
/// excluded and carried again once it is back. An owner the operator
/// included by hand installs as any held owner they include does.
pub fn hold_owners_losing_a_repo(snap: &mut InspectionSnapshot, by_hand: &HashSet<String>) {
    let held: HashSet<String> = unit_owner_holds(snap)
        .into_iter()
        .filter(|(id, hold)| *hold == Some(OwnerHold::RepoExcluded) && !by_hand.contains(id))
        .map(|(id, _)| id)
        .collect();
    let Some(rpm) = snap.rpm.as_mut() else {
        return;
    };
    for pkg in &mut rpm.packages_added {
        if pkg.disposition.is_included() && held.contains(&format!("{}.{}", pkg.name, pkg.arch)) {
            pkg.disposition = FindingKind::excluded();
        }
    }
}

/// Which packages a fleet merge provably left out of the image, by name.
///
/// The aggregate merge keeps in `packages_added` only packages that are a
/// leaf on every host (or, for one not on every host, a leaf where it is),
/// and drops the rest. A package is dropped when it is in neither
/// `baseline_package_names` nor `packages_added`, by name, and nothing the
/// fleet export installs may pull it in, as the kept packages' closures
/// and alternatives joined across every host with leaf data say
/// ([`possibly_pulled_in_by_included`]): a partial package the operator
/// includes counts as a universal leaf does, and a provider dnf may pick
/// among several is not called dropped. Without a baseline a base-image package cannot be told from a
/// dropped one, with no host of leaf authority the merge drops nothing,
/// and with a package the export installs that has no recorded closure
/// what it pulls in is unknown, so none of them proves anything.
pub(crate) struct FleetDrops<'a> {
    kept: HashSet<&'a str>,
    pulled_in: HashSet<String>,
}

impl<'a> FleetDrops<'a> {
    /// `None` for a single host, and for an aggregate that cannot prove a
    /// package absent.
    pub(crate) fn of(snap: &'a InspectionSnapshot) -> Option<Self> {
        snap.aggregate_meta.as_ref()?;
        let rpm = snap.rpm.as_ref()?;
        // `leaf_authority_hosts`, not `leaf_packages`: refine clears an
        // aggregate's leaf list when it opens one, and the count says the
        // same thing, that the merge had leaf data and filtered by it.
        let merge_filtered = rpm.leaf_authority_hosts.is_some_and(|hosts| hosts > 0);
        let baseline = rpm
            .baseline_package_names
            .as_ref()
            .filter(|_| merge_filtered)?;
        let repos = InstallRepos::of(snap);
        if !every_installed_package_has_a_closure(rpm, &repos) {
            return None;
        }
        let kept = baseline
            .iter()
            .map(String::as_str)
            .chain(rpm.packages_added.iter().map(|pkg| pkg.name.as_str()))
            .collect();
        let pulled_in = possibly_pulled_in_by_included(snap, &repos)
            .into_iter()
            .filter_map(|id| id.rsplit_once('.').map(|(name, _)| name.to_string()))
            .collect();
        Some(Self { kept, pulled_in })
    }

    /// Whether the merge left the package named `name` out of the image.
    pub(crate) fn dropped(&self, name: &str) -> bool {
        !self.kept.contains(name) && !self.pulled_in.contains(name)
    }
}

/// The packages a fleet export drops ([`FleetDrops`]) that own a unit
/// enabled on any of its hosts, in display order; empty for a single host
/// and for an aggregate that cannot prove a package absent.
///
/// A unit counts when the merged inventory has it enabled or any host did
/// (`units.enabled_on_any_host`): the inventory keeps the state most hosts
/// have, which a hostname order can tip. The merge keeps only the name, so
/// `arch` and `source_repo` are empty.
pub fn fleet_not_carried_packages(snap: &InspectionSnapshot) -> Vec<NotCarriedPackage> {
    let Some(drops) = FleetDrops::of(snap) else {
        return Vec::new();
    };
    let mut list: Vec<NotCarriedPackage> = enabled_units_by_owner(snap)
        .into_iter()
        .filter(|(owner, _)| drops.dropped(owner))
        .map(|(owner, units)| NotCarriedPackage {
            name: owner.to_string(),
            arch: String::new(),
            source_repo: String::new(),
            enabled_units: units.into_iter().map(str::to_string).collect(),
            hold: None,
            arch_qualified: false,
        })
        .collect();
    sort_for_display(&mut list);
    list
}

/// What the export lists as not carried: the `# Not carried` block, the
/// audit table and the notice. [`fleet_not_carried_packages`] for an
/// aggregate, [`not_carried_packages`] otherwise.
pub fn export_not_carried_packages(snap: &InspectionSnapshot) -> Vec<NotCarriedPackage> {
    if snap.aggregate_meta.is_some() {
        fleet_not_carried_packages(snap)
    } else {
        not_carried_packages(snap)
    }
}

/// Map each owning package name to the enabled units it owns, sorted.
///
/// A unit is enabled when the inventory says so, or, for an aggregate,
/// when any host had it enabled (`units.enabled_on_any_host`). A unit in
/// `INCOMPATIBLE_SERVICES` is never counted: refine drops those from the
/// inventory when it loads a snapshot, so counting them here would make
/// the scan carry a package refine does not.
///
/// An enabled unit is credited to:
/// - the `owning_package` of its `state_changes` row, when it has one;
/// - each package that owns a vendor unit file of its own name in
///   `rpm.file_ownership`;
/// - for a template instance (`worker@blue.service`) with neither, each
///   package that owns its template's vendor file (`worker@.service`),
///   which is the file systemd loads for it. The collector records no row
///   for an instance enabled as its preset says, so the inventory is all
///   there is. A recorded row decides on its own: its fragment names the
///   file that loads, which may be a local replacement no package owns.
fn enabled_units_by_owner(snap: &InspectionSnapshot) -> HashMap<&str, BTreeSet<&str>> {
    let mut owners: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    let Some(units) = snap.units.as_ref() else {
        return owners;
    };
    let enabled: BTreeSet<&UnitName> = units
        .inventory
        .iter()
        .filter(|e| e.state == UnitState::Enabled)
        .map(|e| &e.unit)
        .chain(&units.enabled_on_any_host)
        .filter(|unit| {
            !INCOMPATIBLE_SERVICES
                .iter()
                .any(|s| s.unit == unit.as_str())
        })
        .collect();
    let recorded: HashMap<&str, &UnitStateChange> = units
        .state_changes
        .iter()
        .map(|change| (change.unit().name().as_str(), change))
        .collect();
    let mut vendor_owners: HashMap<&str, Vec<&str>> = HashMap::new();
    for entry in snap.rpm.iter().flat_map(|rpm| &rpm.file_ownership) {
        for path in &entry.paths {
            if let Some(file) = path
                .strip_prefix(VENDOR_UNIT_DIR)
                .and_then(|rest| rest.strip_prefix('/'))
            {
                vendor_owners
                    .entry(file)
                    .or_default()
                    .push(entry.package_name.as_str());
            }
        }
    }

    for unit in enabled {
        let name = unit.as_str();
        let row = recorded.get(name);
        let own_file = vendor_owners.get(name);
        let template = unit.instance().map(|parts| parts.template());
        let template_file = template
            .as_ref()
            .filter(|_| row.is_none() && own_file.is_none())
            .and_then(|template| vendor_owners.get(template.as_str()));
        let credited = row
            .and_then(|change| change.unit().owning_package())
            .into_iter()
            .chain(own_file.into_iter().flatten().copied())
            .chain(template_file.into_iter().flatten().copied());
        for owner in credited {
            owners.entry(owner).or_default().insert(name);
        }
    }
    owners
}

/// How many package names the not-carried notice spells out before it
/// counts the rest. Three keeps the notice to one line in the Export
/// dialog and names every unit owner measured on the RHEL 10.2
/// validation host (`cronie`, `firewalld`, `rsyslog`) and all but one of
/// the four on RHEL 9.8.
const NOT_CARRIED_NOTICE_NAMES: usize = 3;

/// The most bytes one name in the not-carried notice may take, after
/// sanitizing. The snapshot names these packages, so without a bound
/// their length is the snapshot's choice, and the notice travels in a
/// response header.
const NOT_CARRIED_NOTICE_NAME_MAX: usize = 64;

/// The not-carried notice for these unit owners, or `None` when there
/// are none: [`not_carried_notice`] with the owners already chosen.
///
/// `owners` are those packages' display names, each with its
/// [`OwnerHold`] if it has one, in the `# Not carried` block's order,
/// which puts unit owners first. The first [`NOT_CARRIED_NOTICE_NAMES`]
/// are named, a held one with its reason in parentheses, and the rest
/// counted. Each name is sanitized like a package id in any other export
/// warning, then bounded, so the whole entry is printable ASCII of
/// bounded size.
fn not_carried_notice_for_owners(owners: &[(String, Option<OwnerHold>)]) -> Option<ExportWarning> {
    if owners.is_empty() {
        return None;
    }
    let shown: Vec<String> = owners
        .iter()
        .take(NOT_CARRIED_NOTICE_NAMES)
        .map(|(name, hold)| {
            let name = sanitize_bounded(name, NOT_CARRIED_NOTICE_NAME_MAX);
            match hold {
                Some(hold) => format!("{name} ({})", hold.reason()),
                None => name,
            }
        })
        .collect();
    let rest = owners.len() - shown.len();
    let names = if rest > 0 {
        format!("{} and {rest} more", shown.join(", "))
    } else {
        shown.join(", ")
    };
    let lead = match owners.len() {
        1 => "1 package that owns a unit enabled on the source host is not carried".to_string(),
        n => format!("{n} packages that own a unit enabled on the source host are not carried"),
    };
    Some(ExportWarning::notice(format!(
        "{lead}: {names}. See the Not carried table in audit-report.md \
         and the # Not carried block in the Containerfile."
    )))
}

/// The one notice an export carries when it leaves out not-carried
/// packages that own a unit enabled on the source host, or `None` when
/// it leaves out none.
///
/// The owners are the [`export_not_carried_packages`] entries with
/// enabled units, so the notice names the same packages, in the same
/// order, as the `# Not carried` block and the audit table rendered from
/// `snap`, for a single host and a fleet alike.
pub fn not_carried_notice(snap: &InspectionSnapshot) -> Option<ExportWarning> {
    not_carried_notice_for(&export_not_carried_packages(snap))
}

/// [`not_carried_notice`] for a list [`not_carried_packages`] already
/// built, so a caller that shows the list and the notice derives both
/// from one list and they cannot name different packages.
pub fn not_carried_notice_for(list: &[NotCarriedPackage]) -> Option<ExportWarning> {
    let owners: Vec<(String, Option<OwnerHold>)> = list
        .iter()
        .filter(|p| p.owns_enabled_unit())
        .map(|p| (p.display_name(), p.hold))
        .collect();
    not_carried_notice_for_owners(&owners)
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
    use inspectah_core::types::aggregate::AggregateSnapshotMeta;
    use inspectah_core::types::rpm::{FileOwnershipEntry, PackageEntry, RpmSection};
    use inspectah_core::types::units::{
        PresetDefault, UnitInventoryEntry, UnitSection, UnitState, UnitStateChange,
    };

    fn pkg(name: &str, arch: &str, include: bool) -> PackageEntry {
        PackageEntry {
            name: name.into(),
            arch: arch.into(),
            source_repo: "appstream".into(),
            disposition: if include {
                FindingKind::included()
            } else {
                FindingKind::excluded()
            },
            ..Default::default()
        }
    }

    /// A small classified host: two leaves (one excluded), auto packages
    /// in every relation to them, the exceptions, and one name at two
    /// arches.
    fn snapshot() -> InspectionSnapshot {
        let mut locked = pkg("rhc", "x86_64", true);
        locked.locked = true;
        let mut anaconda = pkg("grubby", "x86_64", true);
        anaconda.source_repo = ANACONDA_SOURCE_REPO.into();
        let mut repoless = pkg("vendor-agent", "x86_64", true);
        repoless.repoless_annotation = "no enabled repo serves this package".into();
        let mut uploaded = pkg("vendor-tools", "x86_64", true);
        uploaded.uploaded = true;
        let packages = vec![
            pkg("httpd", "x86_64", true),
            pkg("cups", "x86_64", false),
            pkg("apr", "x86_64", true),
            pkg("cups-libs", "x86_64", true),
            pkg("firewalld", "noarch", true),
            pkg("python3-firewall", "noarch", true),
            pkg("glibc", "x86_64", true),
            pkg("glibc", "i686", true),
            locked,
            anaconda,
            pkg("bash", "x86_64", true),
            repoless,
            uploaded,
        ];
        let auto = [
            "httpd.x86_64",
            "apr.x86_64",
            "cups-libs.x86_64",
            "firewalld.noarch",
            "python3-firewall.noarch",
            "glibc.x86_64",
            "glibc.i686",
            "rhc.x86_64",
            "grubby.x86_64",
            "bash.x86_64",
            "vendor-agent.x86_64",
            "vendor-tools.x86_64",
        ];
        InspectionSnapshot {
            rpm: Some(RpmSection {
                packages_added: packages,
                leaf_packages: Some(vec!["httpd.x86_64".into(), "cups.x86_64".into()]),
                auto_packages: Some(auto.iter().map(|s| (*s).to_string()).collect()),
                leaf_dep_tree: serde_json::json!({
                    "httpd.x86_64": ["apr.x86_64", "glibc.x86_64"],
                    "cups.x86_64": ["cups-libs.x86_64"],
                }),
                baseline_package_names: Some(vec!["bash".into()]),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn classifies_each_added_package_by_name_arch() {
        use AutoCarriage::*;
        let cases: &[(&str, AutoCarriage, &str)] = &[
            (
                "httpd.x86_64",
                NotAuto,
                "an included leaf, also listed as auto",
            ),
            ("cups.x86_64", NotAuto, "an excluded leaf"),
            ("apr.x86_64", PulledIn, "listed under an included leaf"),
            (
                "cups-libs.x86_64",
                Unneeded,
                "listed only under an excluded leaf",
            ),
            ("firewalld.noarch", Unneeded, "listed under no leaf"),
            (
                "python3-firewall.noarch",
                Unneeded,
                "needed only by another Unneeded package",
            ),
            (
                "glibc.x86_64",
                PulledIn,
                "this arch is under an included leaf",
            ),
            ("glibc.i686", Unneeded, "this arch is under no leaf"),
            ("rhc.x86_64", NotAuto, "locked"),
            ("grubby.x86_64", NotAuto, "anaconda-sourced"),
            ("bash.x86_64", NotAuto, "named in the baseline"),
            ("vendor-agent.x86_64", NotAuto, "routed Repoless"),
            ("vendor-tools.x86_64", NotAuto, "uploaded"),
        ];
        let classes = classify_auto_carriage(&snapshot()).expect("leaf data is available");
        assert_eq!(classes.len(), cases.len(), "one class per added package");
        for (id, expected, why) in cases {
            assert_eq!(classes.get(*id), Some(expected), "{id}: {why}");
        }
    }

    type Degrade = fn(&mut InspectionSnapshot);

    /// Each fallback condition from the module documentation, applied to
    /// the classified host.
    fn fallback_cases() -> Vec<(&'static str, Degrade)> {
        vec![
            ("the collector's unavailable state", |snap| {
                let rpm = snap.rpm.as_mut().expect("rpm section");
                rpm.leaf_packages = None;
                rpm.auto_packages = None;
                rpm.leaf_dep_tree = serde_json::json!({});
            }),
            ("leaf_dep_tree is null", |snap| {
                snap.rpm.as_mut().expect("rpm section").leaf_dep_tree = serde_json::Value::Null;
            }),
            ("leaf_dep_tree is empty with auto packages", |snap| {
                snap.rpm.as_mut().expect("rpm section").leaf_dep_tree = serde_json::json!({});
            }),
            ("an aggregate snapshot", |snap| {
                snap.aggregate_meta = Some(AggregateSnapshotMeta {
                    label: "fleet".into(),
                    host_count: 2,
                    hostnames: vec!["a".into(), "b".into()],
                    merged_at: "2026-11-09T00:00:00Z".into(),
                    baseline_provisional: false,
                    section_host_counts: Default::default(),
                });
            }),
        ]
    }

    #[test]
    fn returns_none_when_the_snapshot_cannot_support_the_judgement() {
        for (why, degrade) in fallback_cases() {
            let mut snap = snapshot();
            degrade(&mut snap);
            assert_eq!(classify_auto_carriage(&snap), None, "{why}");
        }
    }

    #[test]
    fn scan_defaults_exclude_unneeded_packages_and_nothing_else() {
        let before = snapshot();
        let classes = classify_auto_carriage(&before).expect("leaf data is available");
        let mut after = before.clone();
        apply_scan_package_defaults(&mut after);

        let before_pkgs = &before.rpm.as_ref().expect("rpm section").packages_added;
        let after_pkgs = &after.rpm.as_ref().expect("rpm section").packages_added;
        assert_eq!(after_pkgs.len(), before_pkgs.len());
        for (old, new) in before_pkgs.iter().zip(after_pkgs) {
            let id = format!("{}.{}", old.name, old.arch);
            if classes[&id] == AutoCarriage::Unneeded {
                assert_eq!(new.disposition, FindingKind::excluded(), "{id} is Unneeded");
                let mut rest = new.clone();
                rest.disposition = old.disposition.clone();
                assert_eq!(&rest, old, "{id}: only the disposition changes");
            } else {
                assert_eq!(new, old, "{id} is not Unneeded and is untouched");
            }
        }
        let mut rest = after.clone();
        rest.rpm.as_mut().expect("rpm section").packages_added = before_pkgs.clone();
        assert_eq!(rest, before, "nothing outside packages_added changes");
    }

    /// The scan writes `repoless_annotation` late (the dnf-cache pass), and
    /// the Repoless exception reads it, so the defaults must run after it.
    /// The same unneeded auto package is excluded without the annotation
    /// and kept with it.
    #[test]
    fn scan_defaults_read_the_repoless_annotation() {
        const ID: &str = "firewalld.noarch";
        let disposition_after_defaults = |annotation: &str| {
            let mut snap = snapshot();
            let rpm = snap.rpm.as_mut().expect("rpm section");
            let pkg = rpm
                .packages_added
                .iter_mut()
                .find(|p| format!("{}.{}", p.name, p.arch) == ID)
                .expect("fixture lists the package");
            pkg.repoless_annotation = annotation.into();
            apply_scan_package_defaults(&mut snap);
            snap.rpm
                .expect("rpm section")
                .packages_added
                .into_iter()
                .find(|p| format!("{}.{}", p.name, p.arch) == ID)
                .expect("fixture lists the package")
                .disposition
        };
        assert_eq!(
            disposition_after_defaults(""),
            FindingKind::excluded(),
            "{ID} with no annotation is Unneeded"
        );
        assert_eq!(
            disposition_after_defaults("No repo source \u{2014} manual resolution needed"),
            FindingKind::included(),
            "{ID} annotated repo-less keeps its disposition"
        );
    }

    #[test]
    fn scan_defaults_are_idempotent() {
        let mut once = snapshot();
        apply_scan_package_defaults(&mut once);
        let mut twice = once.clone();
        apply_scan_package_defaults(&mut twice);
        assert_eq!(twice, once);
        assert_eq!(
            classify_auto_carriage(&twice),
            classify_auto_carriage(&snapshot()),
            "excluding Unneeded packages leaves the class unchanged"
        );
    }

    #[test]
    fn scan_defaults_change_nothing_under_each_fallback() {
        for (why, degrade) in fallback_cases() {
            let mut snap = snapshot();
            degrade(&mut snap);
            let before = snap.clone();
            apply_scan_package_defaults(&mut snap);
            assert_eq!(snap, before, "{why}");
        }
    }

    fn vendor_unit(unit: &str) -> String {
        format!("/usr/lib/systemd/system/{unit}")
    }

    fn enabled_change(unit: &str, owner: &str) -> UnitStateChange {
        UnitStateChange::new(
            test_carried_unit(unit.to_owned().try_into().unwrap(), Some(owner.into())),
            UnitState::Enabled,
            None,
        )
        .unwrap()
    }

    /// The classified host with its services: `firewalld.service` is
    /// preset-matched and found only through file ownership,
    /// `firewall-helper.service` through a state change, and every other
    /// unit is one the not-carried packages must not be credited with.
    fn host() -> InspectionSnapshot {
        let mut snap = snapshot();
        snap.units = Some(UnitSection {
            state_changes: vec![
                enabled_change("firewall-helper.service", "python3-firewall"),
                enabled_change("httpd.service", "httpd"),
            ],
            inventory: vec![
                UnitInventoryEntry {
                    unit: "cups.service"
                        .to_owned()
                        .try_into()
                        .expect("valid unit name"),
                    state: UnitState::Enabled,
                    preset: None,
                },
                UnitInventoryEntry {
                    unit: "firewall-helper.service"
                        .to_owned()
                        .try_into()
                        .expect("valid unit name"),
                    state: UnitState::Enabled,
                    preset: None,
                },
                UnitInventoryEntry {
                    unit: "firewalld.service"
                        .to_owned()
                        .try_into()
                        .expect("valid unit name"),
                    state: UnitState::Enabled,
                    preset: Some(PresetDefault::Enable),
                },
                UnitInventoryEntry {
                    unit: "httpd.service"
                        .to_owned()
                        .try_into()
                        .expect("valid unit name"),
                    state: UnitState::Enabled,
                    preset: None,
                },
                UnitInventoryEntry {
                    unit: "libs-local.service"
                        .to_owned()
                        .try_into()
                        .expect("valid unit name"),
                    state: UnitState::Enabled,
                    preset: None,
                },
            ],
            drop_ins: vec![],
            mount_findings: Vec::new(),
            manual_links: Vec::new(),
            undetermined_installs: Vec::new(),
            default_target: None,
            enabled_on_any_host: Vec::new(),
        });
        let rpm = snap.rpm.as_mut().expect("rpm section");
        rpm.file_ownership = vec![
            FileOwnershipEntry {
                package_name: "firewalld".into(),
                paths: vec![
                    vendor_unit("firewalld.service"),
                    "/usr/sbin/firewalld".into(),
                ],
            },
            FileOwnershipEntry {
                package_name: "cups".into(),
                paths: vec![vendor_unit("cups.service")],
            },
            FileOwnershipEntry {
                package_name: "cups-libs".into(),
                // Not enabled, and enabled but not a vendor unit file.
                paths: vec![
                    vendor_unit("cups-lpd.socket"),
                    "/etc/systemd/system/libs-local.service".into(),
                ],
            },
        ];
        snap
    }

    /// The host as the scan writes it. `firewalld` and `python3-firewall`
    /// own an enabled unit, so the scan keeps both.
    fn scanned_host() -> InspectionSnapshot {
        let mut snap = host();
        apply_scan_package_defaults(&mut snap);
        snap
    }

    /// The unit owners of [`host`], which the scan carries by default.
    const HOST_UNIT_OWNERS: [&str; 2] = ["firewalld.noarch", "python3-firewall.noarch"];

    /// The scanned host after an operator excluded both unit owners, so
    /// every not-carried kind is listed.
    fn owners_excluded() -> InspectionSnapshot {
        let mut snap = scanned_host();
        for id in HOST_UNIT_OWNERS {
            set_disposition(&mut snap, id, FindingKind::excluded());
        }
        snap
    }

    fn ids(list: &[NotCarriedPackage]) -> Vec<String> {
        list.iter()
            .map(|p| format!("{}.{}", p.name, p.arch))
            .collect()
    }

    fn set_disposition(snap: &mut InspectionSnapshot, id: &str, disposition: FindingKind) {
        snap.rpm
            .as_mut()
            .expect("rpm section")
            .packages_added
            .iter_mut()
            .find(|p| format!("{}.{}", p.name, p.arch) == id)
            .expect("fixture lists the package")
            .disposition = disposition;
    }

    /// Owners of an enabled unit are `UnitOwner`, found through either
    /// owner record, and the scan keeps them; an operator's exclusion
    /// lists them again and the notice names them.
    #[test]
    fn unit_owners_are_carried_by_default_and_listed_once_excluded() {
        let classes = classify_auto_carriage(&host()).expect("leaf data is available");
        for id in HOST_UNIT_OWNERS {
            assert_eq!(classes[id], AutoCarriage::UnitOwner, "{id}");
        }
        let mut owners: Vec<String> = default_carried_unit_owners(&host()).into_iter().collect();
        owners.sort();
        assert_eq!(owners, HOST_UNIT_OWNERS);

        let scanned = scanned_host();
        for pkg in &scanned.rpm.as_ref().expect("rpm section").packages_added {
            let id = format!("{}.{}", pkg.name, pkg.arch);
            if HOST_UNIT_OWNERS.contains(&id.as_str()) {
                assert!(pkg.disposition.is_included(), "{id} is carried");
            }
        }
        assert_eq!(
            not_carried_notice(&scanned),
            None,
            "every unit owner is carried, so there is no notice"
        );

        let excluded = owners_excluded();
        let listed = ids(&not_carried_packages(&excluded));
        for id in HOST_UNIT_OWNERS {
            assert!(listed.contains(&id.to_string()), "{id} excluded is listed");
        }
        let notice = not_carried_notice(&excluded).expect("an excluded owner gives a notice");
        assert!(
            notice.text().starts_with(
                "2 packages that own a unit enabled on the source host are not carried: \
                 firewalld, python3-firewall."
            ),
            "{}",
            notice.text()
        );
    }

    /// Refine drops the incompatible services from the inventory when it
    /// loads a snapshot, so an enabled one makes no package a unit owner
    /// on the scan path either, and the two paths keep the same default.
    #[test]
    fn an_incompatible_service_makes_no_unit_owner() {
        let mut snap = host();
        let units = snap.units.as_mut().expect("units section");
        units.inventory.push(UnitInventoryEntry {
            unit: "packagekit.service"
                .to_owned()
                .try_into()
                .expect("valid unit name"),
            state: UnitState::Enabled,
            preset: None,
        });
        snap.rpm
            .as_mut()
            .expect("rpm section")
            .file_ownership
            .push(FileOwnershipEntry {
                package_name: "glibc".into(),
                paths: vec![vendor_unit("packagekit.service")],
            });
        let classes = classify_auto_carriage(&snap).expect("leaf data is available");
        assert_eq!(classes["glibc.i686"], AutoCarriage::Unneeded);
    }

    #[test]
    fn lists_exactly_the_excluded_unneeded_packages() {
        let classes = classify_auto_carriage(&host()).expect("leaf data is available");
        let mut unneeded: Vec<String> = classes
            .iter()
            .filter(|(_, class)| **class == AutoCarriage::Unneeded)
            .map(|(id, _)| id.clone())
            .collect();
        unneeded.sort();

        let mut listed = ids(&not_carried_packages(&scanned_host()));
        listed.sort();
        assert_eq!(listed, unneeded, "the scan excludes every Unneeded package");

        assert!(
            not_carried_packages(&host()).is_empty(),
            "an Unneeded package that is still included is not listed"
        );

        let mut reincluded = scanned_host();
        set_disposition(&mut reincluded, "glibc.i686", FindingKind::included());
        assert!(
            !ids(&not_carried_packages(&reincluded)).contains(&"glibc.i686".to_string()),
            "an operator's include takes a package off the list"
        );

        assert!(
            !listed.contains(&"cups.x86_64".to_string()),
            "an excluded package that is not Unneeded is not listed"
        );
    }

    #[test]
    fn lists_nothing_under_each_fallback() {
        for (why, degrade) in fallback_cases() {
            let mut snap = scanned_host();
            degrade(&mut snap);
            assert!(not_carried_packages(&snap).is_empty(), "{why}");
        }
    }

    #[test]
    fn orders_unit_owners_first_then_by_name_and_arch() {
        let list = not_carried_packages(&owners_excluded());
        assert_eq!(
            ids(&list),
            [
                "firewalld.noarch",
                "python3-firewall.noarch",
                "cups-libs.x86_64",
                "glibc.i686",
            ],
        );
    }

    #[test]
    fn attributes_only_the_enabled_units_a_package_owns() {
        let list = not_carried_packages(&owners_excluded());
        let units = |name: &str| {
            list.iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| panic!("{name} is listed"))
                .enabled_units
                .clone()
        };
        assert_eq!(
            units("firewalld"),
            ["firewalld.service"],
            "a preset-matched unit is found through the vendor unit file"
        );
        assert_eq!(
            units("python3-firewall"),
            ["firewall-helper.service"],
            "a state change's owning_package"
        );
        assert!(
            units("cups-libs").is_empty(),
            "a unit it owns that is not enabled, or outside the vendor directory, is not attributed"
        );
        let attributed: Vec<String> = list.iter().flat_map(|p| p.enabled_units.clone()).collect();
        for other in ["cups.service", "httpd.service"] {
            assert!(
                !attributed.contains(&other.to_string()),
                "{other} is owned by a package not on the list"
            );
        }
    }

    /// The inventory holds every unit kind, so an enabled vendor socket
    /// makes its owner a unit owner like a service does, carried by the
    /// scan and credited once excluded; the same socket disabled credits
    /// nothing and the scan excludes the package.
    #[test]
    fn an_enabled_socket_names_its_not_carried_owner() {
        for (state, named) in [(UnitState::Enabled, true), (UnitState::Disabled, false)] {
            let mut snap = host();
            snap.units
                .as_mut()
                .expect("units section")
                .inventory
                .push(UnitInventoryEntry {
                    unit: "cups-lpd.socket"
                        .to_owned()
                        .try_into()
                        .expect("valid unit name"),
                    state,
                    preset: Some(PresetDefault::Disable),
                });
            apply_scan_package_defaults(&mut snap);
            let carried = snap
                .rpm
                .as_ref()
                .expect("rpm section")
                .packages_added
                .iter()
                .find(|p| p.name == "cups-libs")
                .expect("fixture lists cups-libs")
                .disposition
                .is_included();
            assert_eq!(carried, named, "{state:?}: an owner is carried");
            for id in HOST_UNIT_OWNERS.iter().chain(&["cups-libs.x86_64"]) {
                set_disposition(&mut snap, id, FindingKind::excluded());
            }
            let list = not_carried_packages(&snap);
            let cups_libs = list
                .iter()
                .find(|p| p.name == "cups-libs")
                .expect("cups-libs is not carried");
            let notice = not_carried_notice(&snap).expect("other owners give a notice");
            if named {
                assert_eq!(cups_libs.enabled_units, ["cups-lpd.socket"], "{state:?}");
                assert!(notice.text().contains("cups-libs"), "{}", notice.text());
            } else {
                assert!(cups_libs.enabled_units.is_empty(), "{state:?}");
                assert!(!notice.text().contains("cups-libs"), "{}", notice.text());
            }
        }
    }

    #[test]
    fn display_name_adds_the_arch_only_when_the_name_is_listed_twice() {
        let single = not_carried_packages(&owners_excluded());
        let names: Vec<String> = single.iter().map(NotCarriedPackage::display_name).collect();
        assert_eq!(
            names,
            ["firewalld", "python3-firewall", "cups-libs", "glibc"],
            "one arch of each name"
        );

        // With httpd no longer pulling glibc.x86_64 in, both arches are listed.
        let mut both = host();
        both.rpm.as_mut().expect("rpm section").leaf_dep_tree = serde_json::json!({
            "httpd.x86_64": ["apr.x86_64"],
            "cups.x86_64": ["cups-libs.x86_64"],
        });
        apply_scan_package_defaults(&mut both);
        for id in HOST_UNIT_OWNERS {
            set_disposition(&mut both, id, FindingKind::excluded());
        }
        let names: Vec<String> = not_carried_packages(&both)
            .iter()
            .map(NotCarriedPackage::display_name)
            .collect();
        assert_eq!(
            names,
            [
                "firewalld",
                "python3-firewall",
                "cups-libs",
                "glibc.i686",
                "glibc.x86_64"
            ],
        );
    }

    /// The pipeline-side half of the parity check: dispositions as refine's
    /// leaf default sets them for the same host (leaves kept as decided,
    /// every auto package excluded but the unit owners) give the scan
    /// path's list. The cross-crate test against refine itself lives in
    /// the CLI crate.
    #[test]
    fn leaf_default_dispositions_give_the_scan_list() {
        let mut leaf_default = host();
        let owners = default_carried_unit_owners(&leaf_default);
        let rpm = leaf_default.rpm.as_mut().expect("rpm section");
        let leaves: HashSet<String> = rpm
            .leaf_packages
            .clone()
            .expect("leaf data is available")
            .into_iter()
            .collect();
        for pkg in &mut rpm.packages_added {
            let id = format!("{}.{}", pkg.name, pkg.arch);
            if !leaves.contains(&id) && !owners.contains(&id) {
                pkg.disposition = FindingKind::excluded();
            }
        }
        let scan = not_carried_packages(&scanned_host());
        assert!(
            !scan.is_empty(),
            "the comparison is not between two empty lists"
        );
        assert_eq!(not_carried_packages(&leaf_default), scan);
    }

    /// A merged fleet: `httpd` is the one leaf every host has, and it
    /// pulls in `apr`; `app`, which needs `firewalld`, is kept from the
    /// hosts that have it and left out. Every other package the hosts had
    /// is gone from `packages_added`, as the merge leaves it. Enabled units
    /// are owned by a base-image package (`openssh-server`), a kept leaf
    /// (`httpd`), the pulled-in `apr`, and two dropped packages
    /// (`firewalld`, `cronie`).
    fn fleet() -> InspectionSnapshot {
        let enabled = |unit: &str| UnitInventoryEntry {
            unit: unit.to_owned().try_into().expect("valid unit name"),
            state: UnitState::Enabled,
            preset: None,
        };
        let owns = |package: &str, unit: &str| FileOwnershipEntry {
            package_name: package.into(),
            paths: vec![vendor_unit(unit)],
        };
        InspectionSnapshot {
            aggregate_meta: Some(AggregateSnapshotMeta {
                label: "fleet".into(),
                host_count: 2,
                hostnames: vec!["a".into(), "b".into()],
                merged_at: "2026-11-21T00:00:00Z".into(),
                baseline_provisional: false,
                section_host_counts: Default::default(),
            }),
            rpm: Some(RpmSection {
                packages_added: vec![pkg("httpd", "x86_64", true), pkg("app", "x86_64", false)],
                leaf_packages: Some(vec!["httpd.x86_64".into()]),
                leaf_dep_tree: serde_json::json!({ "httpd.x86_64": ["apr.x86_64"] }),
                dep_closures: Some(
                    [
                        ("httpd.x86_64", vec!["apr.x86_64".to_string()]),
                        ("app.x86_64", vec!["firewalld.x86_64".to_string()]),
                    ]
                    .into_iter()
                    .map(|(id, deps)| (id.to_string(), deps))
                    .collect(),
                ),
                leaf_authority_hosts: Some(2),
                leaf_total_hosts: Some(2),
                baseline_package_names: Some(vec!["openssh-server".into()]),
                file_ownership: vec![
                    owns("openssh-server", "sshd.service"),
                    owns("httpd", "httpd.service"),
                    owns("apr", "apr-cache.service"),
                    owns("firewalld", "firewalld.service"),
                    owns("cronie", "crond.service"),
                ],
                ..Default::default()
            }),
            units: Some(UnitSection {
                state_changes: vec![],
                inventory: [
                    "sshd.service",
                    "httpd.service",
                    "apr-cache.service",
                    "firewalld.service",
                    "crond.service",
                ]
                .into_iter()
                .map(enabled)
                .collect(),
                drop_ins: vec![],
                mount_findings: Vec::new(),
                manual_links: Vec::new(),
                undetermined_installs: Vec::new(),
                default_target: None,
                enabled_on_any_host: Vec::new(),
            }),
            ..Default::default()
        }
    }

    /// The fleet export names exactly the dropped unit owners, in the
    /// notice as in the export list; a single host and a fleet that cannot
    /// tell base packages from dropped ones name none.
    #[test]
    fn a_fleet_names_the_unit_owners_its_merge_drops() {
        let snap = fleet();
        let listed: Vec<String> = export_not_carried_packages(&snap)
            .iter()
            .map(NotCarriedPackage::display_name)
            .collect();
        assert_eq!(listed, ["cronie", "firewalld"]);
        assert_eq!(
            not_carried_notice(&snap).expect("a fleet notice").text(),
            "2 packages that own a unit enabled on the source host are not carried: \
             cronie, firewalld. See the Not carried table in audit-report.md \
             and the # Not carried block in the Containerfile."
        );
        assert!(
            not_carried_packages(&snap).is_empty(),
            "refine's view rows stay single-host: a fleet row has no entry to include"
        );

        let mut no_baseline = fleet();
        no_baseline
            .rpm
            .as_mut()
            .expect("rpm section")
            .baseline_package_names = None;
        assert!(fleet_not_carried_packages(&no_baseline).is_empty());

        let mut no_authority = fleet();
        no_authority
            .rpm
            .as_mut()
            .expect("rpm section")
            .leaf_authority_hosts = Some(0);
        assert!(fleet_not_carried_packages(&no_authority).is_empty());

        let mut single = fleet();
        single.aggregate_meta = None;
        assert!(fleet_not_carried_packages(&single).is_empty());
    }

    /// `app`, a package only some hosts have, is kept but left out, so
    /// what it needs is not installed. Included, it installs `firewalld`,
    /// which then is not named, nor when it is only one provider `app` may
    /// install. An installed package with no recorded closure could
    /// install anything, so the fleet names nothing.
    #[test]
    fn a_fleet_counts_every_package_its_export_installs() {
        let names = |snap: &InspectionSnapshot| -> Vec<String> {
            export_not_carried_packages(snap)
                .iter()
                .map(NotCarriedPackage::display_name)
                .collect()
        };
        let include_app = |snap: &mut InspectionSnapshot| {
            let rpm = snap.rpm.as_mut().expect("rpm section");
            let app = rpm
                .packages_added
                .iter_mut()
                .find(|p| p.name == "app")
                .expect("app is kept");
            app.disposition = FindingKind::included();
        };

        let mut included = fleet();
        include_app(&mut included);
        assert_eq!(names(&included), ["cronie"]);

        // `firewalld` as one of several providers `app` may install: dnf
        // may pick it, so it is not called dropped either.
        let mut alternative = fleet();
        include_app(&mut alternative);
        let rpm = alternative.rpm.as_mut().expect("rpm section");
        let firewalld = rpm
            .dep_closures
            .as_mut()
            .and_then(|closures| closures.get_mut("app.x86_64"))
            .map(std::mem::take)
            .expect("app's closure");
        rpm.dep_alternatives = Some([("app.x86_64".to_string(), firewalld)].into());
        assert_eq!(names(&alternative), ["cronie"]);

        let mut no_closure = fleet();
        include_app(&mut no_closure);
        no_closure
            .rpm
            .as_mut()
            .and_then(|rpm| rpm.dep_closures.as_mut())
            .expect("closures")
            .remove("app.x86_64");
        assert!(names(&no_closure).is_empty(), "{:?}", names(&no_closure));

        let mut excluded_without_closure = fleet();
        excluded_without_closure
            .rpm
            .as_mut()
            .and_then(|rpm| rpm.dep_closures.as_mut())
            .expect("closures")
            .remove("app.x86_64");
        assert_eq!(
            names(&excluded_without_closure),
            ["cronie", "firewalld"],
            "a package the export leaves out needs no closure"
        );

        let mut leaf_tree_only = fleet();
        leaf_tree_only
            .rpm
            .as_mut()
            .expect("rpm section")
            .dep_closures = None;
        assert!(
            names(&leaf_tree_only).is_empty(),
            "the leaf tree leaves out user-installed dependencies"
        );
    }

    /// Which repos a package installed by name loses in an export: a
    /// host-enabled custom section the build's repo files no longer carry
    /// enabled, and nothing else.
    #[test]
    fn a_repo_is_lost_only_when_the_export_drops_a_host_enabled_custom_section() {
        use inspectah_core::types::rpm::RepoFile;
        const VENDOR: &str = "[vendor]\nbaseurl=https://example.invalid/v\nenabled=1\n";
        const OTHER: &str = "[other]\nbaseurl=https://example.invalid/o\nenabled=1\n";
        let file = |name: &str, content: &str, include: bool, default: bool| RepoFile {
            path: format!("/etc/yum.repos.d/{name}"),
            content: content.into(),
            is_default_repo: default,
            disposition: FindingKind::from_bool(include),
            ..Default::default()
        };
        let both = format!("{VENDOR}{OTHER}");
        let cases: Vec<(&str, Vec<RepoFile>, &[&str])> = vec![
            (
                "carried",
                vec![file("vendor.repo", VENDOR, true, false)],
                &[],
            ),
            (
                "excluded beside another custom file",
                vec![
                    file("vendor.repo", VENDOR, false, false),
                    file("other.repo", OTHER, true, false),
                ],
                &["vendor"],
            ),
            (
                "excluded, nothing else staged",
                vec![file("vendor.repo", VENDOR, false, false)],
                &["vendor"],
            ),
            (
                "disabled on the host",
                vec![
                    file(
                        "vendor.repo",
                        &VENDOR.replace("enabled=1", "enabled=0"),
                        false,
                        false,
                    ),
                    file("other.repo", OTHER, true, false),
                ],
                &[],
            ),
            (
                "a default repo file defines it too",
                vec![
                    file("vendor.repo", VENDOR, false, false),
                    file("redhat.repo", VENDOR, false, true),
                ],
                &[],
            ),
            (
                "a distro repo",
                vec![file(
                    "local.repo",
                    &VENDOR.replace("[vendor]", "[appstream]"),
                    false,
                    false,
                )],
                &[],
            ),
            (
                "a file another section keeps",
                vec![file("both.repo", &both, true, false)],
                &[],
            ),
        ];
        for (case, repo_files, expected) in cases {
            let mut snap = InspectionSnapshot::new();
            snap.rpm = Some(RpmSection {
                repo_files,
                ..Default::default()
            });
            let rpm = snap.rpm.as_ref().expect("rpm section");
            let lost = repos_lost_in_export(rpm, &InstallRepos::of(&snap));
            let expected: HashSet<String> = expected.iter().map(|s| s.to_string()).collect();
            assert_eq!(lost, expected, "{case}");
        }
    }

    /// The not-carried notice: absent with no unit owners, singular for
    /// one, three names and a count past three, and a hostile name
    /// sanitized and bounded so the entry stays a valid header value.
    #[test]
    fn not_carried_notice_names_owners_and_bounds_its_text() {
        use crate::export_warning::ExportWarningKind;
        const TAIL: &str = ". See the Not carried table in audit-report.md \
                            and the # Not carried block in the Containerfile.";
        let names = |v: &[&str]| v.iter().map(|s| (s.to_string(), None)).collect::<Vec<_>>();

        assert_eq!(not_carried_notice_for_owners(&[]), None);

        let cases: &[(&[&str], &str)] = &[
            (
                &["firewalld"],
                "1 package that owns a unit enabled on the source host is not carried: firewalld",
            ),
            (
                &["cronie", "firewalld", "rsyslog"],
                "3 packages that own a unit enabled on the source host are not carried: \
                 cronie, firewalld, rsyslog",
            ),
            (
                &["cronie", "firewalld", "insights-client", "rsyslog", "rhc"],
                "5 packages that own a unit enabled on the source host are not carried: \
                 cronie, firewalld, insights-client and 2 more",
            ),
        ];
        for (owners, lead) in cases {
            let notice =
                not_carried_notice_for_owners(&names(owners)).expect("owners give a notice");
            assert_eq!(notice.kind(), ExportWarningKind::Notice);
            assert_eq!(notice.text(), format!("{lead}{TAIL}"), "{owners:?}");
        }

        // A held owner carries its reason after its name.
        let notice = not_carried_notice_for_owners(&[
            ("agent".to_string(), Some(OwnerHold::DisabledRepo)),
            ("cronie".to_string(), None),
            ("tool".to_string(), Some(OwnerHold::NoRepo)),
        ])
        .expect("owners give a notice");
        assert_eq!(
            notice.text(),
            format!(
                "3 packages that own a unit enabled on the source host are not carried: \
                 agent (needs a repo disabled on the host), cronie, \
                 tool (needs a package no enabled repo serves){TAIL}"
            )
        );

        let hostile = format!("evil\r\nX-Evil: 1\u{7f}\u{20ac}{}", "a".repeat(10_000));
        let held = |name: String| (name, Some(OwnerHold::DisabledRepo));
        let notice = not_carried_notice_for_owners(&[
            held(hostile.clone()),
            held(hostile.clone()),
            held(hostile),
        ])
        .unwrap();
        let text = notice.text();
        assert!(text.contains("evil__X-Evil__1__aaa"), "{text}");
        assert!(text.len() < 512, "{} bytes", text.len());
        assert!(text.is_ascii() && !text.chars().any(|c| c.is_ascii_control()));
    }
}
