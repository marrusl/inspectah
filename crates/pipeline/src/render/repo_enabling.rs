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
use inspectah_core::types::repo::{RepoId, parse_repo_sections};
use inspectah_core::types::rpm::{PackageEntry, RpmSection};

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
    /// The `.repo` file that would restore it. `None` when the export
    /// captured no file defining any serving section at all: there is then
    /// nothing to re-include, and the advice that names a file is dropped
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
/// A serving section survives when its `.repo` file is *carried* (included
/// in `repo_files`), the section is *enabled* in the content that file
/// carries, and it is *keyed* (every `gpgkey=file://` path it names is an
/// included `gpg_keys` entry). The files and keys the package itself owns
/// are checked too: a dropped `epel.repo` leaves nothing to install from
/// even when another file still defines the serving section.
pub(crate) fn repo_enabling_export(rpm: &RpmSection, pkg: &PackageEntry) -> RepoEnablingExport {
    let Some(first_id) = pkg.repo_enabling_from.first() else {
        return RepoEnablingExport::NotEnabling;
    };

    // Prefer an id the export has a definition for, so the advice points
    // at a file that exists; fall back to the first id when none does.
    let (named_id, named_file) = pkg
        .repo_enabling_from
        .iter()
        .find_map(|id| defining_file(rpm, id).map(|f| (id.clone(), Some(f.to_string()))))
        .unwrap_or_else(|| (first_id.clone(), None));

    if let Some(dropped) = dropped_owned_path(rpm, pkg) {
        return RepoEnablingExport::Lost(ServingLoss {
            id: named_id,
            file: Some(dropped.to_string()),
        });
    }

    if pkg
        .repo_enabling_from
        .iter()
        .any(|id| section_survives(rpm, id))
    {
        RepoEnablingExport::Served
    } else {
        RepoEnablingExport::Lost(ServingLoss {
            id: named_id,
            file: named_file,
        })
    }
}

/// The path of the first captured `.repo` file defining `id`, whether or
/// not the export is carrying it.
fn defining_file<'a>(rpm: &'a RpmSection, id: &RepoId) -> Option<&'a str> {
    rpm.repo_files
        .iter()
        .find(|f| parse_repo_sections(&f.content).iter().any(|s| s.id == *id))
        .map(|f| f.path.as_str())
}

/// Whether an included `.repo` file defines `id` as enabled with every key
/// it names included.
fn section_survives(rpm: &RpmSection, id: &RepoId) -> bool {
    rpm.repo_files
        .iter()
        .filter(|f| f.disposition.is_included())
        .any(|f| {
            parse_repo_sections(&f.content)
                .iter()
                .any(|s| s.id == *id && s.enabled && keys_included(rpm, &s.gpg_key_paths))
        })
}

/// Whether every `gpgkey=file://` path a section names is an included
/// `gpg_keys` entry.
///
/// Shared with `repoless::disabled_repo_install`: a section dnf cannot
/// verify packages from is no more installable from a disabled repo than
/// from a repo-enabling one.
pub(crate) fn keys_included(rpm: &RpmSection, paths: &[String]) -> bool {
    paths.iter().all(|path| {
        rpm.gpg_keys
            .iter()
            .any(|k| k.disposition.is_included() && k.path == *path)
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
                .any(|f| f.path == **path && f.disposition.is_included())
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
    let Some(rpm) = snap.rpm.as_mut() else {
        return;
    };
    let demoted: Vec<(usize, String)> = {
        let read = &*rpm;
        read.packages_added
            .iter()
            .enumerate()
            .filter(|(_, pkg)| pkg.disposition.is_included())
            .filter_map(|(i, pkg)| match repo_enabling_export(read, pkg) {
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
