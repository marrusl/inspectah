//! The not-carried class: auto packages that nothing the image installs
//! needs.
//!
//! An added package is in the class when it is in `auto_packages` (not
//! user-installed on the source host) and no included package pulls it in,
//! as [`pulled_in_by_included`] answers from `leaf_dep_tree`. Identity is
//! `name.arch` throughout. [`classify_auto_carriage`] is the one decider:
//! no function outside this module decides membership, and every other
//! function here reaches the class through it.
//!
//! Some packages keep their existing handling and are never in the class:
//! a locked package, one with `source_repo == "anaconda"` (refine's
//! anaconda classifier decides those), one whose name is in
//! `baseline_package_names`, and one that routes as
//! [`PackageRoute::Upload`] or [`PackageRoute::Repoless`].
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

use std::collections::{HashMap, HashSet};

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::verified_upload::VerifiedUploads;

use super::repoless::{PackageRoute, route_for};
use super::service_intent::pulled_in_by_included;

/// `source_repo` recorded for a package installed at system install time,
/// which refine's anaconda classifier decides.
const ANACONDA_SOURCE_REPO: &str = "anaconda";

/// How an added package reaches the target image, for the auto class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoCarriage {
    /// Not this class: a leaf, not in `auto_packages`, or an auto package
    /// that keeps its existing handling (locked, anaconda-sourced, named in
    /// the baseline, uploaded, or routed repo-less).
    NotAuto,
    /// An auto package an included leaf pulls in (`pulled_in_by_included`).
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

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::FindingKind;
    use inspectah_core::types::aggregate::AggregateSnapshotMeta;
    use inspectah_core::types::rpm::{PackageEntry, RpmSection};

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

    #[test]
    fn returns_none_when_the_snapshot_cannot_support_the_judgement() {
        type Degrade = fn(&mut InspectionSnapshot);
        let cases: &[(&str, Degrade)] = &[
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
        ];
        for (why, degrade) in cases {
            let mut snap = snapshot();
            degrade(&mut snap);
            assert_eq!(classify_auto_carriage(&snap), None, "{why}");
        }
    }
}
