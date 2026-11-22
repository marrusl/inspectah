//! What the export says about the `.repo` files and GPG keys the repo
//! section owns, beyond copying them: a shared file copied whole for one
//! repo while another it defines is excluded, a fleet file whose hosts
//! differ, and an entry no copied repo file names.
//!
//! Every text here names paths, section ids and host names, never file
//! content, and each Containerfile line is one comment line, escaped when
//! [`super::comment::join_lines`] joins the file.

use std::collections::{BTreeMap, BTreeSet};

use inspectah_core::repo_ownership::{OwnedKind, repo_ownership};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::destination::Destination;
use inspectah_core::types::repo::{parse_repo_sections, section_spelling};
use inspectah_core::types::rpm::RepoFile;

use super::configtree::{StagedFileKind, StagedWrites};
use super::language_packages::sanitize_for_comment;
use crate::export_warning::ExportWarning;

/// How many names a list in a line or notice spells out before
/// `and <m> more`.
const NAMED_MAX: usize = 3;

/// `a, b, c and 2 more`, or the names joined by `, ` when there are at
/// most [`NAMED_MAX`].
fn shortened<S: AsRef<str>>(names: &[S]) -> String {
    let shown: Vec<&str> = names.iter().take(NAMED_MAX).map(AsRef::as_ref).collect();
    let rest = names.len().saturating_sub(NAMED_MAX);
    if rest == 0 {
        shown.join(", ")
    } else {
        format!("{} and {rest} more", shown.join(", "))
    }
}

/// Whether the stager writes `entry`'s own payload at its destination.
fn ships(staged: &StagedWrites<'_>, entry: &RepoFile) -> bool {
    Destination::new(&entry.path).is_ok_and(|d| {
        staged
            .writes
            .get(&d)
            .is_some_and(|w| matches!(w.source, StagedFileKind::RepoFile | StagedFileKind::GpgKey))
    })
}

/// Each shipped repo file that also defines a repo excluded in refine,
/// with the excluded ids and the ids keeping the file (§ 6.3 of the repo
/// ownership spec). A keeper is a section of the file the operator kept
/// that the host enabled there, or one the host disabled while an
/// included, unlocked package attributed to it installs with
/// `--enablerepo`.
fn shared_files(
    snap: &InspectionSnapshot,
    staged: &StagedWrites<'_>,
    excluded: &BTreeSet<String>,
) -> Vec<(String, Vec<String>, Vec<String>)> {
    let Some(rpm) = snap.rpm.as_ref() else {
        return Vec::new();
    };
    if excluded.is_empty() {
        return Vec::new();
    }
    let spellings: Vec<String> = rpm
        .repo_files
        .iter()
        .flat_map(|f| parse_repo_sections(&f.content))
        .map(|s| s.id.as_str().to_owned())
        .collect();
    let attributed: BTreeSet<&str> = rpm
        .packages_added
        .iter()
        .filter(|p| p.disposition.is_included() && !p.locked)
        .filter_map(|p| section_spelling(spellings.iter().map(String::as_str), &p.source_repo))
        .collect();
    let mut out = Vec::new();
    for file in rpm
        .repo_files
        .iter()
        .filter(|f| f.disposition.is_included())
    {
        if !ships(staged, file) {
            continue;
        }
        let sections = parse_repo_sections(&file.content);
        let gone: BTreeSet<String> = sections
            .iter()
            .map(|s| s.id.as_str())
            .filter(|id| excluded.contains(*id))
            .map(str::to_owned)
            .collect();
        if gone.is_empty() {
            continue;
        }
        let keeping: BTreeSet<String> = sections
            .iter()
            .filter(|s| !excluded.contains(s.id.as_str()))
            .filter(|s| s.enabled || attributed.contains(s.id.as_str()))
            .map(|s| s.id.as_str().to_owned())
            .collect();
        out.push((
            file.path.clone(),
            gone.into_iter().collect(),
            keeping.into_iter().collect(),
        ));
    }
    out
}

/// The § 6.3 text for one shared file, without the comment marker.
fn shared_file_text(path: &str, gone: &[String], keeping: &[String]) -> String {
    let gone = shortened(gone);
    format!(
        "{path} also defines repo {gone}, excluded in refine; the file is copied whole for repo \
         {}, so {gone} stays defined as the host had it",
        shortened(keeping)
    )
}

/// The Custom Repositories section's lines for a shipped file that also
/// defines a repo excluded in refine (§ 6.3). `excluded` is refine's
/// decision; a scan has none, so the scan tarball never has the line.
pub fn shared_file_lines(
    snap: &InspectionSnapshot,
    staged: &StagedWrites<'_>,
    excluded: &BTreeSet<String>,
) -> Vec<String> {
    shared_files(snap, staged, excluded)
        .into_iter()
        .map(|(path, gone, keeping)| {
            format!(
                "# {}",
                sanitize_for_comment(&shared_file_text(&path, &gone, &keeping))
            )
        })
        .collect()
}

/// One comment line for each shipped entry of `kind` in a fleet aggregate
/// whose hosts did not all have the copied version (§ 6.5): the version
/// most hosts have is copied, and the line names the hosts whose file the
/// image will not match.
pub fn version_lines(
    snap: &InspectionSnapshot,
    staged: &StagedWrites<'_>,
    kind: OwnedKind,
) -> Vec<String> {
    let Some(rpm) = snap.rpm.as_ref() else {
        return Vec::new();
    };
    let list = match kind {
        OwnedKind::RepoFile => &rpm.repo_files,
        OwnedKind::GpgKey => &rpm.gpg_keys,
    };
    list.iter()
        .filter(|e| e.disposition.is_included() && ships(staged, e))
        .filter_map(|e| {
            let prevalence = e.aggregate.as_ref()?;
            let copied = prevalence.payload_hosts.as_ref()?;
            let different: Vec<&String> = prevalence
                .hosts
                .iter()
                .filter(|h| !copied.contains(h))
                .collect();
            if different.is_empty() {
                return None;
            }
            let text = format!(
                "{}: the export copies the version {} of {} hosts with this file have; a \
                 different version is on {}",
                e.path,
                copied.len(),
                prevalence.hosts.len(),
                shortened(&different)
            );
            Some(format!("# {}", sanitize_for_comment(&text)))
        })
        .collect()
}

/// The repo files `snapshot.redactions` names, by the path of the
/// `rpm.repo_files` entry at each finding's destination, sorted.
pub fn redacted_repo_files(snap: &InspectionSnapshot) -> Vec<String> {
    let Some(rpm) = snap.rpm.as_ref() else {
        return Vec::new();
    };
    let found: BTreeSet<Destination> = snap
        .redactions
        .iter()
        .filter_map(|f| Destination::new(&f.path).ok())
        .collect();
    let files: BTreeSet<String> = rpm
        .repo_files
        .iter()
        .filter(|f| Destination::new(&f.path).is_ok_and(|d| found.contains(&d)))
        .map(|f| f.path.clone())
        .collect();
    files.into_iter().collect()
}

/// The review sentence a notice ends with when redaction may be why no
/// copied repo file names a key (§ 6.6).
const REVIEW_REDACTED_KEY: &str = "Review the redacted reference; if it named this key, restore it and add the key to the \
     image yourself.";

/// The notice for a repo file or key no stored repo file defines or names.
fn no_owner_notice(entry: &RepoFile, kind: OwnedKind, redacted: &[String]) -> ExportWarning {
    let path = &entry.path;
    match (kind, &entry.aggregate) {
        (OwnedKind::RepoFile, _) => {
            let cause = if redacted.iter().any(|f| f == path) {
                "redaction changed the [section] headers in it".to_owned()
            } else {
                "its [section] headers were changed after the scan".to_owned()
            };
            ExportWarning::notice(format!(
                "{path} is not copied: {cause}, so it defines no repo the export can decide. \
                 Review the redacted headers; if the file is needed, restore them and add the \
                 file to the image yourself."
            ))
        }
        (OwnedKind::GpgKey, Some(prevalence)) => {
            let mut text = format!(
                "{path} is not copied: no version of a repo file that the export copies names \
                 it. {} have it because a different version of a repo file names it there.",
                shortened(&prevalence.hosts)
            );
            if !redacted.is_empty() {
                text.push(' ');
                text.push_str(REVIEW_REDACTED_KEY);
            }
            ExportWarning::notice(text)
        }
        (OwnedKind::GpgKey, None) => {
            let cause = if redacted.is_empty() {
                "a gpgkey= reference that named it was changed after the scan".to_owned()
            } else {
                format!(
                    "redaction changed a gpgkey= reference in {}",
                    shortened(redacted)
                )
            };
            ExportWarning::notice(format!(
                "{path} is not copied: {cause}, so no repo file the export copies names it. \
                 {REVIEW_REDACTED_KEY}"
            ))
        }
    }
}

/// The export's notices about repo-owned entries, for `x-export-warnings`
/// and the CLI's stderr: one per shared file a refine exclusion leaves
/// defining an excluded repo (§ 6.3), and one per repo file or key no
/// stored repo file defines or names (§ 6.5, § 6.6), each entry once.
pub fn export_notices(
    snap: &InspectionSnapshot,
    staged: &StagedWrites<'_>,
    excluded: &BTreeSet<String>,
) -> Vec<ExportWarning> {
    let mut out: Vec<ExportWarning> = shared_files(snap, staged, excluded)
        .into_iter()
        .map(|(path, gone, keeping)| {
            ExportWarning::notice(shared_file_text(&path, &gone, &keeping))
        })
        .collect();
    let Some(rpm) = snap.rpm.as_ref() else {
        return out;
    };
    let ownership = repo_ownership(rpm);
    let redacted = redacted_repo_files(snap);
    let mut seen: BTreeMap<Destination, ()> = BTreeMap::new();
    for (kind, list) in [
        (OwnedKind::RepoFile, &rpm.repo_files),
        (OwnedKind::GpgKey, &rpm.gpg_keys),
    ] {
        for entry in list {
            let Ok(dest) = Destination::new(&entry.path) else {
                continue;
            };
            let unowned = ownership.get(&dest).is_none_or(|e| e.owners.is_empty());
            if unowned && seen.insert(dest, ()).is_none() {
                out.push(no_owner_notice(entry, kind, &redacted));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repo file with no owner gets its own wording, never the key's:
    /// redaction rewrote its headers, not a `gpgkey=` reference.
    #[test]
    fn an_ownerless_repo_file_is_named_as_a_repo_file() {
        let file = RepoFile {
            path: "/etc/yum.repos.d/x.repo".into(),
            ..Default::default()
        };
        let redacted = vec![file.path.clone()];
        let text = no_owner_notice(&file, OwnedKind::RepoFile, &redacted)
            .text()
            .to_owned();
        assert_eq!(
            text,
            "/etc/yum.repos.d/x.repo is not copied: redaction changed the [section] headers in \
             it, so it defines no repo the export can decide. Review the redacted headers; if \
             the file is needed, restore them and add the file to the image yourself."
        );
        assert!(!text.contains("gpgkey"));
        let unexplained = no_owner_notice(&file, OwnedKind::RepoFile, &[])
            .text()
            .to_owned();
        assert!(unexplained.contains("its [section] headers were changed after the scan"));
    }

    #[test]
    fn a_long_list_is_shortened_after_three() {
        assert_eq!(shortened(&["a"]), "a");
        assert_eq!(shortened(&["a", "b", "c"]), "a, b, c");
        assert_eq!(shortened(&["a", "b", "c", "d", "e"]), "a, b, c and 2 more");
    }
}
