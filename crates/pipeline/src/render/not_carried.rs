//! The not-carried class: auto packages that nothing the image installs
//! needs.
//!
//! An added package is in the class when it is in `auto_packages` (not
//! user-installed on the source host) and no leaf the export installs
//! pulls it in, as [`pulled_in_by_included`] answers from `leaf_dep_tree`.
//! An included leaf the export renders only as a manual step pulls nothing
//! in, since the image never installs it. Identity is
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
//! **Limits inherited from the tree.** `leaf_dep_tree` maps each leaf to
//! the transitive closure of its auto dependencies and records nothing
//! else. An auto package that only another not-carried package needs is
//! therefore in the class too, which is correct: nothing included pulls
//! either of them in. A dependency of an auto package that an operator
//! includes on its own is not recorded, so it stays in the class although
//! dnf would install it with that package.

use std::collections::{BTreeSet, HashMap, HashSet};

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::units::UnitState;
use inspectah_core::types::verified_upload::VerifiedUploads;

use super::repoless::{PackageRoute, route_for};
use super::unit_plan::pulled_in_by_included;
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
    /// An auto package a leaf the export installs pulls in
    /// (`pulled_in_by_included`).
    PulledIn,
    /// An auto package nothing included pulls in.
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

    let pulled_in = pulled_in_by_included(rpm);
    let baseline: HashSet<&str> = rpm
        .baseline_package_names
        .iter()
        .flatten()
        .map(String::as_str)
        .collect();
    let no_uploads = VerifiedUploads::new();

    let classes = rpm
        .packages_added
        .iter()
        .map(|pkg| {
            let id = format!("{}.{}", pkg.name, pkg.arch);
            let class = if leaves.contains(id.as_str()) || !auto.contains(id.as_str()) {
                AutoCarriage::NotAuto
            } else if pulled_in.contains(&id) {
                AutoCarriage::PulledIn
            } else if pkg.locked
                || pkg.source_repo == ANACONDA_SOURCE_REPO
                || baseline.contains(pkg.name.as_str())
                || pkg.uploaded
                || route_for(rpm, pkg, &no_uploads) == PackageRoute::Repoless
            {
                AutoCarriage::NotAuto
            } else {
                AutoCarriage::Unneeded
            };
            (id, class)
        })
        .collect();
    Some(classes)
}

/// Exclude every [`AutoCarriage::Unneeded`] package from a scan snapshot.
///
/// This is the scan's default disposition for the class; every other
/// package is left as it is, and under the fallback nothing changes.
/// Idempotent: only leaves contribute to [`pulled_in_by_included`], so
/// excluding auto packages cannot change the class.
pub fn apply_scan_package_defaults(snap: &mut InspectionSnapshot) {
    let Some(classes) = classify_auto_carriage(snap) else {
        return;
    };
    let Some(rpm) = snap.rpm.as_mut() else {
        return;
    };
    for pkg in &mut rpm.packages_added {
        let id = format!("{}.{}", pkg.name, pkg.arch);
        if classes.get(&id) == Some(&AutoCarriage::Unneeded) {
            pkg.disposition = FindingKind::excluded();
        }
    }
}

/// An excluded [`AutoCarriage::Unneeded`] package, as every surface shows
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotCarriedPackage {
    /// Package name.
    pub name: String,
    /// Package architecture.
    pub arch: String,
    /// Repository the package was installed from.
    pub source_repo: String,
    /// Units enabled in `units.inventory` that this package owns.
    pub enabled_units: Vec<String>,
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

    /// Whether this package owns a unit enabled in `units.inventory`: the
    /// one test every surface uses to put a package first and to name it
    /// in [`not_carried_notice`].
    pub fn owns_enabled_unit(&self) -> bool {
        !self.enabled_units.is_empty()
    }
}

/// Directory holding the vendor unit files packages ship.
const VENDOR_UNIT_DIR: &str = "/usr/lib/systemd/system";

/// Every surface: the excluded [`AutoCarriage::Unneeded`] packages, in
/// display order.
///
/// Packages owning a unit enabled in `units.inventory` come first, then
/// the rest; each part is sorted by name, then arch. Empty under the
/// [`classify_auto_carriage`] fallback.
///
/// A unit is a package's when it is enabled and either a `state_changes`
/// entry names the package as `owning_package`, or the package owns the
/// vendor unit file in `rpm.file_ownership`. Both record the owner by
/// name, not `name.arch`, so every arch of a name is credited with the
/// same units. The units are for display and ordering only: whether a
/// unit is emitted is the presence rules' decision.
pub fn not_carried_packages(snap: &InspectionSnapshot) -> Vec<NotCarriedPackage> {
    let Some(classes) = classify_auto_carriage(snap) else {
        return Vec::new();
    };
    let Some(rpm) = snap.rpm.as_ref() else {
        return Vec::new();
    };
    let units_by_owner = enabled_units_by_owner(snap);

    let mut list: Vec<NotCarriedPackage> = rpm
        .packages_added
        .iter()
        .filter(|pkg| {
            pkg.disposition == FindingKind::excluded()
                && classes.get(&format!("{}.{}", pkg.name, pkg.arch))
                    == Some(&AutoCarriage::Unneeded)
        })
        .map(|pkg| NotCarriedPackage {
            name: pkg.name.clone(),
            arch: pkg.arch.clone(),
            source_repo: pkg.source_repo.clone(),
            enabled_units: units_by_owner
                .get(pkg.name.as_str())
                .map(|units| units.iter().map(|u| (*u).to_string()).collect())
                .unwrap_or_default(),
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

    list.sort_by(|a, b| {
        // `false < true`, so unit owners sort first.
        a.enabled_units
            .is_empty()
            .cmp(&b.enabled_units.is_empty())
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.arch.cmp(&b.arch))
    });
    list
}

/// Map each owning package name to the enabled units it owns, sorted.
fn enabled_units_by_owner(snap: &InspectionSnapshot) -> HashMap<&str, BTreeSet<&str>> {
    let mut owners: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    let Some(services) = snap.units.as_ref() else {
        return owners;
    };
    let enabled: HashSet<&str> = services
        .inventory
        .iter()
        .filter(|e| e.state == UnitState::Enabled)
        .map(|e| e.unit.as_str())
        .collect();

    for change in &services.state_changes {
        if let Some(owner) = change.unit().owning_package()
            && enabled.contains(change.unit().name().as_str())
        {
            owners
                .entry(owner)
                .or_default()
                .insert(change.unit().name().as_str());
        }
    }

    if let Some(rpm) = snap.rpm.as_ref() {
        for entry in &rpm.file_ownership {
            for path in &entry.paths {
                if let Some(unit) = path
                    .strip_prefix(VENDOR_UNIT_DIR)
                    .and_then(|rest| rest.strip_prefix('/'))
                    && enabled.contains(unit)
                {
                    owners
                        .entry(entry.package_name.as_str())
                        .or_default()
                        .insert(unit);
                }
            }
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
/// `owners` are those packages' display names, in the `# Not carried`
/// block's order, which puts unit owners first. The first
/// [`NOT_CARRIED_NOTICE_NAMES`] are named and the rest counted. Each name
/// is sanitized like a package id in any other export warning, then
/// bounded, so the whole entry is printable ASCII of bounded size.
fn not_carried_notice_for_owners(owners: &[String]) -> Option<ExportWarning> {
    if owners.is_empty() {
        return None;
    }
    let shown: Vec<String> = owners
        .iter()
        .take(NOT_CARRIED_NOTICE_NAMES)
        .map(|name| sanitize_bounded(name, NOT_CARRIED_NOTICE_NAME_MAX))
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
/// The owners are the [`not_carried_packages`] entries with enabled
/// units, so the notice names the same packages, in the same order, as
/// the `# Not carried` block and the audit table rendered from `snap`.
pub fn not_carried_notice(snap: &InspectionSnapshot) -> Option<ExportWarning> {
    not_carried_notice_for(&not_carried_packages(snap))
}

/// [`not_carried_notice`] for a list [`not_carried_packages`] already
/// built, so a caller that shows the list and the notice derives both
/// from one list and they cannot name different packages.
pub fn not_carried_notice_for(list: &[NotCarriedPackage]) -> Option<ExportWarning> {
    let owners: Vec<String> = list
        .iter()
        .filter(|p| p.owns_enabled_unit())
        .map(NotCarriedPackage::display_name)
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

    /// The host as the scan writes it.
    fn scanned_host() -> InspectionSnapshot {
        let mut snap = host();
        apply_scan_package_defaults(&mut snap);
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
        let list = not_carried_packages(&scanned_host());
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
        let list = not_carried_packages(&scanned_host());
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
    /// credits its not-carried owner like a service does; the same socket
    /// disabled credits nothing.
    #[test]
    fn an_enabled_socket_names_its_not_carried_owner() {
        for (state, named) in [(UnitState::Enabled, true), (UnitState::Disabled, false)] {
            let mut snap = scanned_host();
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
        let single = not_carried_packages(&scanned_host());
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
    /// every auto package excluded) give the scan path's list. The
    /// cross-crate test against refine itself lives in the CLI crate.
    #[test]
    fn leaf_default_dispositions_give_the_scan_list() {
        let mut leaf_default = host();
        let rpm = leaf_default.rpm.as_mut().expect("rpm section");
        let leaves: HashSet<String> = rpm
            .leaf_packages
            .clone()
            .expect("leaf data is available")
            .into_iter()
            .collect();
        for pkg in &mut rpm.packages_added {
            if !leaves.contains(&format!("{}.{}", pkg.name, pkg.arch)) {
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

    /// The not-carried notice: absent with no unit owners, singular for
    /// one, three names and a count past three, and a hostile name
    /// sanitized and bounded so the entry stays a valid header value.
    #[test]
    fn not_carried_notice_names_owners_and_bounds_its_text() {
        use crate::export_warning::ExportWarningKind;
        const TAIL: &str = ". See the Not carried table in audit-report.md \
                            and the # Not carried block in the Containerfile.";
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();

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

        let hostile = format!("evil\r\nX-Evil: 1\u{7f}\u{20ac}{}", "a".repeat(10_000));
        let notice =
            not_carried_notice_for_owners(&[hostile.clone(), hostile.clone(), hostile]).unwrap();
        let text = notice.text();
        assert!(text.contains("evil__X-Evil__1__aaa"), "{text}");
        assert!(text.len() < 512, "{} bytes", text.len());
        assert!(text.is_ascii() && !text.chars().any(|c| c.is_ascii_control()));
    }
}
