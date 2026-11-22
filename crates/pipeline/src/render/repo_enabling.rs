//! Export-time proof for repo-enabling packages.
//!
//! A repo-enabling package carries the repository definition that an
//! enabled repo on the source host served it from. The scan records those
//! sections in `repo_enabling_from`: **non-empty means the host's repos
//! proved they serve it by name**, and that is the one source for that
//! fact. Nothing here re-derives it from a name suffix, from
//! `repoless_annotation`'s text, or from anything else.
//!
//! What this module adds is the half the scan could not know. Includes are
//! a refine decision made after the scan, so between classification and
//! render a serving section can be dropped from the export, carried with
//! `enabled=0`, or left without the key it names -- and the package's own
//! repo files can be dropped out from under it. When no proof survives,
//! the package is demoted to the repo-less rendering rather than installed
//! by name from a definition the export does not carry.
//!
//! **Non-empty is therefore no longer the same as "will install by
//! name".** A demoted package keeps `repo_enabling_from` -- what the host
//! proved did not stop being true -- and gains a `repoless_annotation`
//! beside it. The two fields answer different questions and the pair is
//! the state the renderer and the refine view both read:
//! [`repo_enabling_export`] is the only thing that decides which.

use std::collections::BTreeSet;

use inspectah_core::repo_ownership::{OwnedKind, repo_ownership};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::destination::Destination;
use inspectah_core::types::repo::{
    self as repo, RepoId, RepoSectionDef, SectionId, parse_repo_sections, section_spelling,
};
use inspectah_core::types::rpm::{PackageEntry, RpmSection};

use super::repoless::InstallRepos;

/// Where a `.repo` file lives.
const YUM_REPOS_DIR: &str = "/etc/yum.repos.d/";

/// Directories whose contents make up the repository definition an export
/// carries, mirroring the set the scan classified over.
const REPO_CONFIG_DIRS: [&str; 2] = [YUM_REPOS_DIR, "/etc/pki/rpm-gpg/"];

/// What a `.repo` file is named.
const REPO_FILE_SUFFIX: &str = ".repo";

/// What the Containerfile and the projection name when a repo-enabling
/// package loses its proof ([`serving_loss`]).
pub(crate) struct ServingLoss {
    /// The serving section the export no longer provides.
    pub id: RepoId,
    /// The repo, spelled as its section is, whose re-inclusion in refine
    /// serves the package again ([`toggle_restores`]). `None` when no repo
    /// the refine toggle accepts would: the advice that names one is
    /// dropped rather than invented.
    pub repo: Option<String>,
}

/// The export-time status of one package's repo-enabling proof.
pub(crate) enum RepoEnablingExport {
    /// `repo_enabling_from` is empty: the package is not repo-enabling.
    NotEnabling,
    /// A serving section survives the export, and every repo file and key
    /// the package itself provides is still included.
    Served,
    /// No proof survives. The package installs from a repo definition this
    /// export does not carry; [`serving_loss`] says what would restore it.
    Lost,
}

/// Decide whether `pkg`'s repo-enabling proof survives this export.
///
/// A serving id survives when the definition dnf uses for it among the
/// repo files the build has when it installs ([`InstallRepos::effective`]:
/// the first file in dnf's read order, with the content the export
/// stages) is *enabled*, has a *source*, and is *keyed* (every
/// `gpgkey=file://` path it names is a key the image has,
/// [`keys_included`]). Another file defining the
/// same id does not count: dnf drops it as "listed more than once". The
/// files and keys the package itself owns are checked too: a dropped
/// `epel.repo` leaves nothing to install from even when another file
/// still defines the serving section.
pub(crate) fn repo_enabling_export(
    rpm: &RpmSection,
    repos: &InstallRepos,
    pkg: &PackageEntry,
) -> RepoEnablingExport {
    if pkg.repo_enabling_from.is_empty() {
        return RepoEnablingExport::NotEnabling;
    }
    if dropped_owned_path(rpm, pkg).is_some() {
        return RepoEnablingExport::Lost;
    }
    if pkg.repo_enabling_from.iter().any(|id| {
        repos
            .effective(id)
            .is_some_and(|sec| serves(rpm, repos, &sec))
    }) {
        RepoEnablingExport::Served
    } else {
        RepoEnablingExport::Lost
    }
}

/// What to name for `pkg`, a package whose proof this export lost
/// ([`RepoEnablingExport::Lost`]): its first serving id, and the first
/// repo whose re-inclusion in refine would serve it again.
///
/// **The advice names only a control refine offers.** The repo toggle is
/// the one control for a repo's `.repo` file and the keys it names: their
/// Configuration Files rows, where the config walk records one, are
/// read-only inventory decided under Repositories. What brings a repo
/// file and its keys back is the repo toggle, which also re-includes the
/// repo's packages.
/// So the candidates are repos, each judged by doing what the toggle
/// does ([`toggle_restores`]), in this order:
///
/// 1. the serving ids, in `repo_enabling_from` order, spelled as a
///    captured section spells them;
/// 2. the sections the package's own repo files define, sorted: a dropped
///    `vendor.repo` comes back only with a repo it defines.
///
/// Each must be a repo refine can toggle ([`toggleable`]). `None` only for
/// a package that is not repo-enabling or a snapshot with no RPM section,
/// neither of which can lose a proof.
///
/// Separate from the verdict because answering it re-stages the export
/// once per candidate, which only the advice needs.
pub(crate) fn serving_loss(snap: &InspectionSnapshot, pkg: &PackageEntry) -> Option<ServingLoss> {
    let rpm = snap.rpm.as_ref()?;
    let first_id = pkg.repo_enabling_from.first()?;
    let spellings: Vec<String> = rpm
        .repo_files
        .iter()
        .flat_map(|f| parse_repo_sections(&f.content))
        .map(|s| s.id.as_str().to_owned())
        .collect();
    let serving = pkg.repo_enabling_from.iter().filter_map(|id| {
        section_spelling(spellings.iter().map(String::as_str), id.as_str()).map(str::to_owned)
    });
    let owned_paths: Vec<&str> = rpm
        .file_ownership
        .iter()
        .filter(|entry| entry.package_name == pkg.name)
        .flat_map(|entry| entry.paths.iter().map(String::as_str))
        .collect();
    let owned: BTreeSet<String> = rpm
        .repo_files
        .iter()
        .filter(|f| owned_paths.contains(&f.path.as_str()))
        .flat_map(|f| parse_repo_sections(&f.content))
        .map(|s| s.id.as_str().to_owned())
        .collect();
    let mut tried: BTreeSet<String> = BTreeSet::new();
    let repo = serving
        .chain(owned)
        .filter(|repo| tried.insert(repo.clone()))
        .filter(|repo| toggleable(rpm, repo))
        .find(|repo| toggle_restores(snap, pkg, repo));
    Some(ServingLoss {
        id: first_id.clone(),
        repo,
    })
}

/// Whether dnf can install a repo-enabling package from `sec`, the
/// definition it uses for a serving id: enabled, with a source to fetch
/// from (`baseurl`, `metalink` or `mirrorlist` left defined), and every
/// key it names in the image. An enabled copy of the section with no
/// source cannot supply the package, whichever file it sits in.
fn serves(rpm: &RpmSection, repos: &InstallRepos, sec: &RepoSectionDef) -> bool {
    sec.enabled && sec.has_source && keys_included(rpm, repos, sec.local_keys())
}

/// Whether refine's repo toggle cascades to `repo`: core's
/// [`repo::toggleable`] over the snapshot's repo files. The advice names
/// only a repo whose op the server accepts and whose switch reaches its
/// packages.
fn toggleable(rpm: &RpmSection, repo: &str) -> bool {
    repo::toggleable(
        rpm.repo_files
            .iter()
            .map(|f| (f.path.as_str(), f.content.as_str())),
        repo,
    )
}

/// Whether re-including `repo` in refine serves `pkg` again, judged by
/// [`repo_enabling_export`] on the export re-staged as the toggle leaves
/// it: every carriable repo file and key that exact section owns
/// included (`RepoOwnership::carriable`: owned, not locked, not a
/// default repo file, a rejected key or a key the base image ships).
///
/// Re-staging, rather than adding files to the current set, is what keeps
/// the advice honest: a repo whose file is disabled, shadowed by one dnf
/// reads earlier, or withheld by the stager (a config entry at
/// `a.repo/stray` withholds both it and `a.repo`), or one that leaves a
/// file the package owns dropped, would not recover the proof, and advice
/// naming it would send the operator in a circle. The packages the toggle
/// also re-includes play no part in the proof.
fn toggle_restores(snap: &InspectionSnapshot, pkg: &PackageEntry, repo: &str) -> bool {
    let mut proposed = snap.clone();
    let Some(rpm) = proposed.rpm.as_mut() else {
        return false;
    };
    // The include arm's rule: every carriable entry the repo owns.
    let ownership = repo_ownership(rpm);
    let owned: Vec<Destination> = ownership
        .owned_by(&SectionId::new(repo))
        .into_iter()
        .cloned()
        .collect();
    for (kind, list) in [
        (OwnedKind::RepoFile, &mut rpm.repo_files),
        (OwnedKind::GpgKey, &mut rpm.gpg_keys),
    ] {
        for entry in list.iter_mut() {
            let at_owned = Destination::new(&entry.path).is_ok_and(|d| owned.contains(&d));
            if at_owned && ownership.carriable(kind, entry) {
                entry.disposition = FindingKind::included();
            }
        }
    }
    let repos = InstallRepos::of(&proposed);
    proposed.rpm.as_ref().is_some_and(|rpm| {
        matches!(
            repo_enabling_export(rpm, &repos, pkg),
            RepoEnablingExport::Served
        )
    })
}

/// Whether every `gpgkey=file://` path a section names is a key the image
/// has when the package phase installs: one the GPG Keys section copies
/// ([`InstallRepos::copies_key`]: included, and actually staged, so a key
/// the stager withholds or refuses does not count), or one the base image
/// ships ([`RepoFile::shipped_by_base`]). A path written with dnf variables
/// names the key the scan recorded at the expanded path
/// ([`RepoFile::is_key_named_by`]).
///
/// Shared with `repoless::disabled_repo_install`: a section dnf cannot
/// verify packages from is no more installable from a disabled repo than
/// from a repo-enabling one.
///
/// [`RepoFile::is_key_named_by`]: inspectah_core::types::rpm::RepoFile::is_key_named_by
/// [`RepoFile::shipped_by_base`]: inspectah_core::types::rpm::RepoFile::shipped_by_base
pub(crate) fn keys_included<'a>(
    rpm: &RpmSection,
    repos: &InstallRepos,
    mut paths: impl Iterator<Item = &'a str>,
) -> bool {
    paths.all(|path| {
        rpm.gpg_keys.iter().any(|k| {
            k.is_key_named_by(path) && (k.shipped_by_base.is_some() || repos.copies_key(&k.path))
        })
    })
}

/// The first repo file or key the package provides that this export has
/// dropped, if any.
fn dropped_owned_path<'a>(rpm: &'a RpmSection, pkg: &PackageEntry) -> Option<&'a str> {
    let owned = rpm
        .file_ownership
        .iter()
        .find(|entry| entry.package_name == pkg.name)?;
    owned
        .paths
        .iter()
        .filter(|path| REPO_CONFIG_DIRS.iter().any(|dir| path.starts_with(dir)))
        .find(|path| {
            !rpm.repo_files
                .iter()
                .chain(rpm.gpg_keys.iter())
                .any(|f| f.path == **path && f.is_in_image())
        })
        .map(|path| path.as_str())
}

/// The basename of the `.repo` file this package provides, when the
/// export captured one.
///
/// `min()` is both the choice and the ordering: with several files there
/// is no "first" to prefer, and picking the least makes the answer a
/// property of the set rather than of the order the host listed them.
pub(crate) fn owned_repo_file_name<'a>(rpm: &'a RpmSection, pkg: &PackageEntry) -> Option<&'a str> {
    rpm.file_ownership
        .iter()
        .find(|entry| entry.package_name == pkg.name)?
        .paths
        .iter()
        .filter(|path| path.starts_with(YUM_REPOS_DIR) && path.ends_with(REPO_FILE_SUFFIX))
        .min()
        .and_then(|path| path.rsplit('/').next())
}

/// Annotate every included repo-enabling package whose serving definition
/// this export no longer carries.
///
/// The annotation is what the refine view keys the upload control off, so
/// the demotion has to land on the projection rather than only in the
/// rendered Containerfile. Excluded packages are left alone: a package the
/// export is not installing has no proof to lose.
pub fn demote_unserved_repo_enabling(snap: &mut InspectionSnapshot) {
    let repos = InstallRepos::of(snap);
    let demoted: Vec<(usize, String)> = {
        let read: &InspectionSnapshot = snap;
        let Some(rpm) = read.rpm.as_ref() else {
            return;
        };
        rpm.packages_added
            .iter()
            .enumerate()
            .filter(|(_, pkg)| pkg.disposition.is_included())
            .filter(|(_, pkg)| {
                matches!(
                    repo_enabling_export(rpm, &repos, pkg),
                    RepoEnablingExport::Lost
                )
            })
            .filter_map(|(i, pkg)| {
                serving_loss(read, pkg).map(|loss| {
                    (
                        i,
                        format!(
                            "No repo source: the repo that served {} ({}) is not carried by this export",
                            pkg.name, loss.id
                        ),
                    )
                })
            })
            .collect()
    };
    let Some(rpm) = snap.rpm.as_mut() else {
        return;
    };
    for (i, annotation) in demoted {
        rpm.packages_added[i].repoless_annotation = annotation;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::FindingKind;
    use inspectah_core::types::rpm::RepoFile;

    /// A section's key counts as present when the export copies it or the
    /// base image ships it; an excluded key with no such reason does not.
    #[test]
    fn a_key_the_base_image_ships_is_present_in_the_image() {
        const KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-redhat-release";
        let cases = [
            ("copied", FindingKind::included(), None, true),
            (
                "left to the base image",
                FindingKind::excluded(),
                Some("redhat-release"),
                true,
            ),
            ("excluded", FindingKind::excluded(), None, false),
        ];
        for (name, disposition, shipped_by_base, present) in cases {
            let snap = InspectionSnapshot {
                rpm: Some(RpmSection {
                    gpg_keys: vec![RepoFile {
                        path: KEY.into(),
                        disposition,
                        shipped_by_base: shipped_by_base.map(String::from),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            };
            let rpm = snap.rpm.as_ref().expect("an rpm section");
            assert_eq!(
                keys_included(rpm, &InstallRepos::of(&snap), [KEY].into_iter()),
                present,
                "{name}"
            );
        }
    }

    /// dnf keeps the last `gpgkey=` of a reopened section, so a key an
    /// earlier block named and a later one replaced does not cost the
    /// package its proof.
    #[test]
    fn a_replaced_key_the_export_lacks_does_not_demote_the_package() {
        const CARRIED: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9";
        let rpm = RpmSection {
            repo_files: vec![RepoFile {
                path: "/etc/yum.repos.d/epel.repo".into(),
                content: format!(
                    "[epel]\nbaseurl=https://example.invalid/epel\n\
                     gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-gone\n\
                     [epel]\ngpgkey=file://{CARRIED}\n"
                ),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            gpg_keys: vec![RepoFile {
                path: CARRIED.into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let pkg = PackageEntry {
            name: "epel-release".into(),
            repo_enabling_from: vec![RepoId::new("epel")],
            disposition: FindingKind::included(),
            ..Default::default()
        };
        let snap = InspectionSnapshot {
            rpm: Some(rpm),
            ..Default::default()
        };
        let rpm = snap.rpm.as_ref().expect("an rpm section");
        assert!(matches!(
            repo_enabling_export(rpm, &InstallRepos::of(&snap), &pkg),
            RepoEnablingExport::Served
        ));
    }
}
