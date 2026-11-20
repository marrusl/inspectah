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

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::repo::{RepoId, RepoSectionDef, dnf_read_order, parse_repo_sections};
use inspectah_core::types::rpm::{PackageEntry, RepoFile, RpmSection};

use super::repoless::InstallRepos;

/// Where a `.repo` file lives.
const YUM_REPOS_DIR: &str = "/etc/yum.repos.d/";

/// Directories whose contents make up the repository definition an export
/// carries, mirroring the set the scan classified over.
const REPO_CONFIG_DIRS: [&str; 2] = [YUM_REPOS_DIR, "/etc/pki/rpm-gpg/"];

/// What a `.repo` file is named.
const REPO_FILE_SUFFIX: &str = ".repo";

/// What the Containerfile and the projection name when a repo-enabling
/// package loses its proof.
pub(crate) struct ServingLoss {
    /// The serving section the export no longer provides.
    pub id: RepoId,
    /// The `.repo` file that would restore it: a captured file the export
    /// leaves out whose re-inclusion makes dnf's definition of `id` serve
    /// the package again, or the package's own dropped file. `None` when
    /// no single file would: the advice that names a file is dropped
    /// rather than invented.
    pub file: Option<String>,
}

/// The export-time status of one package's repo-enabling proof.
pub(crate) enum RepoEnablingExport {
    /// `repo_enabling_from` is empty: the package is not repo-enabling.
    NotEnabling,
    /// A serving section survives the export, and every repo file and key
    /// the package itself provides is still included.
    Served,
    /// No proof survives. The package installs from a repo definition this
    /// export does not carry.
    Lost(ServingLoss),
}

/// Decide whether `pkg`'s repo-enabling proof survives this export.
///
/// A serving id survives when the definition dnf uses for it among the
/// repo files the build has when it installs ([`InstallRepos::effective`]:
/// the first file in dnf's read order, with the content the export
/// stages) is *enabled* and *keyed* (every `gpgkey=file://` path it names
/// is a key the image has, [`keys_included`]). Another file defining the
/// same id does not count: dnf drops it as "listed more than once". The
/// files and keys the package itself owns are checked too: a dropped
/// `epel.repo` leaves nothing to install from even when another file
/// still defines the serving section.
pub(crate) fn repo_enabling_export(
    rpm: &RpmSection,
    repos: &InstallRepos,
    pkg: &PackageEntry,
) -> RepoEnablingExport {
    let Some(first_id) = pkg.repo_enabling_from.first() else {
        return RepoEnablingExport::NotEnabling;
    };

    // The advice names a serving id and, when one exists, a file whose
    // re-inclusion would serve the package again; the first id otherwise.
    let recovery = pkg
        .repo_enabling_from
        .iter()
        .find_map(|id| restoring_file(rpm, repos, id).map(|f| (id.clone(), f)));
    let named_id = recovery
        .as_ref()
        .map_or_else(|| first_id.clone(), |(id, _)| id.clone());

    if let Some(dropped) = dropped_owned_path(rpm, pkg) {
        return RepoEnablingExport::Lost(ServingLoss {
            id: named_id,
            file: Some(dropped.to_string()),
        });
    }

    if pkg
        .repo_enabling_from
        .iter()
        .any(|id| repos.effective(id).is_some_and(|sec| serves(rpm, &sec)))
    {
        RepoEnablingExport::Served
    } else {
        RepoEnablingExport::Lost(ServingLoss {
            id: named_id,
            file: recovery.map(|(_, f)| f.to_string()),
        })
    }
}

/// Whether dnf can install a repo-enabling package from `sec`, the
/// definition it uses for a serving id: enabled, and every key it names
/// in the image.
fn serves(rpm: &RpmSection, sec: &RepoSectionDef) -> bool {
    sec.enabled && keys_included(rpm, &sec.gpg_key_paths)
}

/// The first captured `.repo` file, in dnf's read order, that the export
/// leaves out and whose re-inclusion alone would make dnf's definition of
/// `id` serve the package ([`InstallRepos::effective_with`]).
///
/// Neither the order of `repo_files` nor the first file that merely
/// defines `id` decides it: re-including a file whose definition is
/// disabled, or one an earlier file still shadows, would not recover the
/// proof, and advice naming it would send the operator in a circle.
fn restoring_file<'a>(rpm: &'a RpmSection, repos: &InstallRepos, id: &RepoId) -> Option<&'a str> {
    let mut left_out: Vec<&RepoFile> = rpm
        .repo_files
        .iter()
        .filter(|f| !f.disposition.is_included())
        .filter(|f| parse_repo_sections(&f.content).iter().any(|s| s.id == *id))
        .collect();
    left_out.sort_by(|a, b| {
        (dnf_read_order(&a.path), &a.content).cmp(&(dnf_read_order(&b.path), &b.content))
    });
    left_out
        .into_iter()
        .find(|f| {
            repos
                .effective_with(f, id)
                .is_some_and(|sec| serves(rpm, &sec))
        })
        .map(|f| f.path.as_str())
}

/// Whether every `gpgkey=file://` path a section names is a key the image
/// will have: an included `gpg_keys` entry, or one the base image ships
/// ([`RepoFile::is_in_image`]). A path written with dnf variables names the
/// key the scan recorded at the expanded path ([`RepoFile::is_key_named_by`]).
///
/// Shared with `repoless::disabled_repo_install`: a section dnf cannot
/// verify packages from is no more installable from a disabled repo than
/// from a repo-enabling one.
///
/// [`RepoFile::is_key_named_by`]: inspectah_core::types::rpm::RepoFile::is_key_named_by
/// [`RepoFile::is_in_image`]: inspectah_core::types::rpm::RepoFile::is_in_image
pub(crate) fn keys_included(rpm: &RpmSection, paths: &[String]) -> bool {
    paths.iter().all(|path| {
        rpm.gpg_keys
            .iter()
            .any(|k| k.is_in_image() && k.is_key_named_by(path))
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
    let Some(rpm) = snap.rpm.as_mut() else {
        return;
    };
    let demoted: Vec<(usize, String)> = {
        let read = &*rpm;
        read.packages_added
            .iter()
            .enumerate()
            .filter(|(_, pkg)| pkg.disposition.is_included())
            .filter_map(|(i, pkg)| match repo_enabling_export(read, &repos, pkg) {
                RepoEnablingExport::Lost(loss) => Some((
                    i,
                    format!(
                        "No repo source: the repo that served {} ({}) is not carried by this export",
                        pkg.name, loss.id
                    ),
                )),
                RepoEnablingExport::Served | RepoEnablingExport::NotEnabling => None,
            })
            .collect()
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
            let rpm = RpmSection {
                gpg_keys: vec![RepoFile {
                    path: KEY.into(),
                    disposition,
                    shipped_by_base: shipped_by_base.map(String::from),
                    ..Default::default()
                }],
                ..Default::default()
            };
            assert_eq!(keys_included(&rpm, &[KEY.to_string()]), present, "{name}");
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
