//! Who owns each `.repo` file and GPG key a snapshot records, and the
//! pass that makes the repo toggle the one control for both.
//!
//! A repo is a `[section]` by its exact spelling ([`SectionId`]). It owns
//! every entry of `rpm.repo_files` that defines it and every entry of
//! `rpm.gpg_keys` a [`GpgKeyRef::Local`](crate::types::repo::GpgKeyRef) of
//! its final `gpgkey=` names ([`RepoFile::is_key_named_by`]). Ownership is
//! read from the repo files the snapshot holds, as stored, through
//! [`parse_repo_sections`], never from how the snapshot was collected:
//! redaction can rewrite a reference after collection, and an aggregate
//! merges repo files and keys to independent representatives, so either
//! can leave an entry no stored repo file names.
//!
//! [`settle_repo_ownership`] establishes the invariants over a whole
//! snapshot, run by every producer after its last content change: the
//! collector, the scan after redaction, and the aggregate merge.
//! [`check_repo_ownership`] checks them at load.

use std::collections::{BTreeMap, BTreeSet};

use crate::snapshot::InspectionSnapshot;
use crate::types::FindingKind;
use crate::types::config::{ConfigShadow, shadow_matching_config_entries};
use crate::types::destination::Destination;
use crate::types::repo::{SectionId, parse_repo_sections};
use crate::types::rpm::{KeyRejection, RepoFile, RpmSection};

/// Which inventory an owned entry is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OwnedKind {
    /// An entry of `rpm.repo_files`.
    RepoFile,
    /// An entry of `rpm.gpg_keys`.
    GpgKey,
}

/// One section that owns an entry, with the repo file that defines it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Owner {
    /// The section, by exact spelling.
    pub section: SectionId,
    /// The repo file whose definition of `section` owns the entry.
    pub file: Destination,
    /// Whether that file enables `section` (`enabled=`, absent is on).
    pub enabled: bool,
}

/// What [`repo_ownership`] records for one destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedEntry {
    pub kind: OwnedKind,
    /// Every owning definition, sorted, without repeats. Empty for an
    /// entry no stored repo file defines or names.
    pub owners: Vec<Owner>,
}

impl OwnedEntry {
    /// The owning sections, by exact spelling.
    pub fn sections(&self) -> BTreeSet<SectionId> {
        self.owners.iter().map(|o| o.section.clone()).collect()
    }
}

/// Who owns each destination of `rpm.repo_files` and `rpm.gpg_keys`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoOwnership {
    /// Every destination either list records. `[site]` and `[SITE]` are
    /// two owners.
    entries: BTreeMap<Destination, OwnedEntry>,
}

impl RepoOwnership {
    /// What is recorded at `dest`, when either list records it.
    pub fn get(&self, dest: &Destination) -> Option<&OwnedEntry> {
        self.entries.get(dest)
    }

    /// Whether either list records `dest`.
    pub fn contains(&self, dest: &Destination) -> bool {
        self.entries.contains_key(dest)
    }

    /// Every recorded destination, sorted, with what it records.
    pub fn entries(&self) -> impl Iterator<Item = (&Destination, &OwnedEntry)> {
        self.entries.iter()
    }

    /// The destinations `section` owns, sorted.
    pub fn owned_by(&self, section: &SectionId) -> Vec<&Destination> {
        self.entries
            .iter()
            .filter(|(_, e)| e.owners.iter().any(|o| o.section == *section))
            .map(|(d, _)| d)
            .collect()
    }

    /// Every section that owns anything, sorted.
    pub fn sections(&self) -> BTreeSet<SectionId> {
        self.entries
            .values()
            .flat_map(|e| e.owners.iter().map(|o| o.section.clone()))
            .collect()
    }

    /// Whether `entry`, an inventory entry of `kind`, can be carried by a
    /// repo op: it has an owner, is not locked, and is not a default repo
    /// file, a rejected key or a key the base image ships (§ 3.2).
    pub fn carriable(&self, kind: OwnedKind, entry: &RepoFile) -> bool {
        let owned = Destination::new(&entry.path)
            .ok()
            .and_then(|d| self.entries.get(&d))
            .is_some_and(|e| !e.owners.is_empty());
        let uncarriable = match kind {
            OwnedKind::RepoFile => entry.is_default_repo,
            OwnedKind::GpgKey => entry.rejected.is_some() || entry.shipped_by_base.is_some(),
        };
        owned && !uncarriable && !entry.locked
    }
}

/// Ownership read from the repo files `rpm` holds, through
/// [`parse_repo_sections`] and [`RepoFile::is_key_named_by`].
///
/// `RepoIndex::build` in refine takes its owner maps from this, so refine,
/// the pass and the load check cannot disagree about who owns what.
pub fn repo_ownership(rpm: &RpmSection) -> RepoOwnership {
    let mut entries: BTreeMap<Destination, OwnedEntry> = BTreeMap::new();
    for (kind, list) in [
        (OwnedKind::RepoFile, &rpm.repo_files),
        (OwnedKind::GpgKey, &rpm.gpg_keys),
    ] {
        for entry in list {
            if let Ok(dest) = Destination::new(&entry.path) {
                entries.entry(dest).or_insert(OwnedEntry {
                    kind,
                    owners: Vec::new(),
                });
            }
        }
    }

    for file in &rpm.repo_files {
        let Ok(file_dest) = Destination::new(&file.path) else {
            continue;
        };
        for section in parse_repo_sections(&file.content) {
            let owner = Owner {
                section: section.id.clone(),
                file: file_dest.clone(),
                enabled: section.enabled,
            };
            if let Some(e) = entries.get_mut(&file_dest) {
                e.owners.push(owner.clone());
            }
            for reference in section.local_keys() {
                for key in rpm.gpg_keys.iter().filter(|k| k.is_key_named_by(reference)) {
                    if let Ok(key_dest) = Destination::new(&key.path)
                        && let Some(e) = entries.get_mut(&key_dest)
                    {
                        e.owners.push(owner.clone());
                    }
                }
            }
        }
    }

    for e in entries.values_mut() {
        e.owners.sort();
        e.owners.dedup();
    }
    RepoOwnership { entries }
}

/// Establishes the repo ownership invariants over `snapshot` as it
/// stands. Idempotent.
///
/// 1. Excludes every repo file or key no repo op can carry: a default
///    repo file, a rejected key, a base-shipped key, and a repo file or
///    key with no owner.
/// 2. Marks every config entry at a destination [`repo_ownership`]
///    records, every content variant and every path spelling of it, as
///    `FindingKind::Inventory` with
///    `shadowed_by: Some(ConfigShadow::Repositories)`, an advisory or
///    machine-state entry excepted.
/// 3. Replaces the content of every config entry at a rejected key's
///    destination, every variant and spelling included, with that key's
///    placeholder, and drops its diff: the config walk reads the file on
///    its own, and the bytes the scan refused to store as a key must not
///    reach the snapshot through its config row either
///    ([`rejected_key_placeholders`]). An entry with no content stays
///    empty.
pub fn settle_repo_ownership(snapshot: &mut InspectionSnapshot) {
    let Some(rpm) = snapshot.rpm.as_mut() else {
        return;
    };
    let ownership = repo_ownership(rpm);
    for (kind, list) in [
        (OwnedKind::RepoFile, &mut rpm.repo_files),
        (OwnedKind::GpgKey, &mut rpm.gpg_keys),
    ] {
        for entry in list.iter_mut() {
            if !uncarried_by_rule(&ownership, kind, entry) {
                continue;
            }
            if entry.disposition.is_included() {
                entry.disposition = FindingKind::excluded();
            }
        }
    }
    let owned: BTreeSet<Destination> = ownership.entries().map(|(d, _)| d.clone()).collect();
    let placeholders = rejected_key_placeholders(rpm);
    shadow_matching_config_entries(snapshot.config.as_mut(), &owned, ConfigShadow::Repositories);
    for entry in snapshot.config.iter_mut().flat_map(|c| c.files.iter_mut()) {
        let Some(placeholder) = Destination::new(&entry.path)
            .ok()
            .and_then(|d| placeholders.get(&d))
        else {
            continue;
        };
        if !entry.content.is_empty() {
            entry.content = placeholder.clone();
        }
        entry.diff_against_rpm = None;
    }
}

/// The content every config entry at a rejected key's destination holds:
/// the canonical placeholder of the rejected entry
/// ([`KeyRejection::placeholder`] of its path), the first in list order
/// where two spellings of one destination are recorded. Read from the
/// rejection, never from the entry's stored content, so a load check
/// cannot be satisfied by an edited key whose twins match its edit.
fn rejected_key_placeholders(rpm: &RpmSection) -> BTreeMap<Destination, String> {
    let mut out = BTreeMap::new();
    for key in &rpm.gpg_keys {
        if let (Some(rejection), Ok(dest)) = (key.rejected, Destination::new(&key.path)) {
            out.entry(dest)
                .or_insert_with(|| rejection.placeholder(&key.path));
        }
    }
    out
}

/// Whether the ownership rules alone keep `entry` out of the export,
/// whatever any repo op does: no owner, a default repo file, a rejected
/// key or a base-shipped key. A locked entry is held excluded elsewhere.
fn uncarried_by_rule(ownership: &RepoOwnership, kind: OwnedKind, entry: &RepoFile) -> bool {
    let unowned = Destination::new(&entry.path)
        .ok()
        .and_then(|d| ownership.get(&d))
        .is_none_or(|e| e.owners.is_empty());
    unowned
        || match kind {
            OwnedKind::RepoFile => entry.is_default_repo,
            OwnedKind::GpgKey => entry.rejected.is_some() || entry.shipped_by_base.is_some(),
        }
}

/// An advisory entry is never materialized, and shadowing it would erase
/// its warning; a machine-state entry is decided in its own list.
fn exempt_from_shadow(kind: &FindingKind) -> bool {
    kind.is_advisory() || kind.is_machine_state()
}

/// A snapshot that breaks the repo ownership invariants.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RepoOwnershipViolation {
    /// A config entry, not advisory, at a repo-owned destination that is
    /// not `Inventory` shadowed by `Repositories`.
    #[error("{path}: a configuration row at a repo-owned path is not decided under Repositories")]
    UnshadowedConfig { path: String },
    /// A config entry shadowed by `Repositories` at a destination the repo
    /// inventory does not record.
    #[error("{path}: a configuration row is decided under Repositories, but no repo records it")]
    ShadowWithoutOwner { path: String },
    /// An included default repo file, rejected key, base-shipped key, or
    /// repo file or key with no owner.
    #[error("{path}: a repo file or key no repo op can carry is included")]
    UncarriableIncluded { path: String },
    /// A key recorded as usable whose content the scan would have refused
    /// ([`KeyRejection::of_content`]): not a PGP public key, or one with
    /// private key armor in it.
    #[error("{path}: a key recorded as usable is not a PGP public key, or holds private key armor")]
    UnusableKey { path: String },
    /// A rejected key whose content is not its canonical placeholder
    /// ([`KeyRejection::placeholder`] of its path), or a config entry at
    /// its destination that holds anything but that placeholder, or a
    /// diff.
    #[error("{path}: a key the scan refused to store, or its configuration row, holds its content")]
    RejectedKeyContent { path: String },
}

/// The invariants [`settle_repo_ownership`] establishes, checked without
/// changing anything. Returns the first violation by destination.
pub fn check_repo_ownership(snapshot: &InspectionSnapshot) -> Result<(), RepoOwnershipViolation> {
    let empty = RpmSection::default();
    let rpm = snapshot.rpm.as_ref().unwrap_or(&empty);
    let ownership = repo_ownership(rpm);
    let mut violations: BTreeMap<String, RepoOwnershipViolation> = BTreeMap::new();
    let mut note = |path: &str, v: RepoOwnershipViolation| {
        let key = Destination::new(path)
            .map(|d| d.as_str().to_owned())
            .unwrap_or_else(|_| path.to_owned());
        violations.entry(key).or_insert(v);
    };

    for key in &rpm.gpg_keys {
        let found = KeyRejection::of_content(&key.content);
        match key.rejected {
            None if found.is_some() => note(
                &key.path,
                RepoOwnershipViolation::UnusableKey {
                    path: key.path.clone(),
                },
            ),
            Some(rejection)
                if found != Some(KeyRejection::NotPgp)
                    || key.content != rejection.placeholder(&key.path) =>
            {
                note(
                    &key.path,
                    RepoOwnershipViolation::RejectedKeyContent {
                        path: key.path.clone(),
                    },
                )
            }
            _ => {}
        }
    }
    let placeholders = rejected_key_placeholders(rpm);
    for entry in snapshot.config.iter().flat_map(|c| &c.files) {
        let Some(placeholder) = Destination::new(&entry.path)
            .ok()
            .and_then(|d| placeholders.get(&d))
        else {
            continue;
        };
        let holds = !entry.content.is_empty() && entry.content != *placeholder;
        if holds || entry.diff_against_rpm.is_some() {
            note(
                &entry.path,
                RepoOwnershipViolation::RejectedKeyContent {
                    path: entry.path.clone(),
                },
            );
        }
    }
    for (kind, list) in [
        (OwnedKind::RepoFile, &rpm.repo_files),
        (OwnedKind::GpgKey, &rpm.gpg_keys),
    ] {
        for entry in list {
            if entry.disposition.is_included() && uncarried_by_rule(&ownership, kind, entry) {
                note(
                    &entry.path,
                    RepoOwnershipViolation::UncarriableIncluded {
                        path: entry.path.clone(),
                    },
                );
            }
        }
    }
    if let Some(config) = snapshot.config.as_ref() {
        for entry in &config.files {
            let owned = Destination::new(&entry.path).is_ok_and(|d| ownership.contains(&d));
            let shadowed = entry.shadowed_by == Some(ConfigShadow::Repositories);
            if owned && !exempt_from_shadow(&entry.disposition) {
                if !(shadowed && entry.disposition.is_inventory()) {
                    note(
                        &entry.path,
                        RepoOwnershipViolation::UnshadowedConfig {
                            path: entry.path.clone(),
                        },
                    );
                }
            } else if shadowed && !owned {
                note(
                    &entry.path,
                    RepoOwnershipViolation::ShadowWithoutOwner {
                        path: entry.path.clone(),
                    },
                );
            }
        }
    }
    match violations.into_values().next() {
        Some(v) => Err(v),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::config::{ConfigFileEntry, ConfigSection};

    fn repo(path: &str, content: &str) -> RepoFile {
        RepoFile {
            path: path.into(),
            content: content.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }
    }

    fn config(path: &str) -> ConfigFileEntry {
        ConfigFileEntry {
            path: path.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }
    }

    const PUBLIC: &str =
        "-----BEGIN PGP PUBLIC KEY BLOCK-----\nmQENB\n-----END PGP PUBLIC KEY BLOCK-----\n";
    const PRIVATE: &str = "-----BEGIN PGP PRIVATE KEY BLOCK-----\nlQdGBsecret\n\
                           -----END PGP PRIVATE KEY BLOCK-----\n";

    fn snapshot() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            repo_files: vec![repo(
                "/etc/yum.repos.d/site.repo",
                "[site]\ngpgkey=file:///etc/pki/rpm-gpg//K\n[SITE]\nenabled=0\n\
                 gpgkey=file:///etc/pki/rpm-gpg/K\n",
            )],
            gpg_keys: vec![
                repo("/etc/pki/rpm-gpg/K", PUBLIC),
                repo("/etc/pki/rpm-gpg/ORPHAN", PUBLIC),
            ],
            ..Default::default()
        });
        snap.config = Some(ConfigSection {
            files: vec![
                config("etc/yum.repos.d/site.repo"),
                config("/etc/pki/rpm-gpg/K"),
                config("/etc/pki/rpm-gpg/other"),
            ],
            ..Default::default()
        });
        snap
    }

    #[test]
    fn ownership_keys_by_destination_and_exact_spelling() {
        let snap = snapshot();
        let own = repo_ownership(snap.rpm.as_ref().unwrap());
        let key = own
            .get(&Destination::new("/etc/pki/rpm-gpg/K").unwrap())
            .unwrap();
        let ids: Vec<&str> = key.owners.iter().map(|o| o.section.as_str()).collect();
        assert_eq!(ids, ["SITE", "site"], "a `//` reference names the key too");
        let orphan = own
            .get(&Destination::new("/etc/pki/rpm-gpg/ORPHAN").unwrap())
            .unwrap();
        assert!(orphan.owners.is_empty());
    }

    #[test]
    fn settle_establishes_what_check_requires_and_is_idempotent() {
        let mut snap = snapshot();
        assert!(matches!(
            check_repo_ownership(&snap),
            Err(RepoOwnershipViolation::UncarriableIncluded { .. })
                | Err(RepoOwnershipViolation::UnshadowedConfig { .. })
        ));
        settle_repo_ownership(&mut snap);
        check_repo_ownership(&snap).expect("settled");
        let files = &snap.config.as_ref().unwrap().files;
        assert_eq!(files[0].shadowed_by, Some(ConfigShadow::Repositories));
        assert_eq!(files[1].shadowed_by, Some(ConfigShadow::Repositories));
        assert_eq!(files[2].shadowed_by, None, "an unnamed path keeps its row");
        let orphan = &snap.rpm.as_ref().unwrap().gpg_keys[1];
        assert!(!orphan.disposition.is_included(), "no owner: excluded");
        let once = snap.clone();
        settle_repo_ownership(&mut snap);
        assert_eq!(snap, once);
    }

    #[test]
    fn check_names_each_violation() {
        let mut snap = snapshot();
        settle_repo_ownership(&mut snap);

        let mut unshadowed = snap.clone();
        let row = &mut unshadowed.config.as_mut().unwrap().files[1];
        row.shadowed_by = None;
        row.disposition = FindingKind::included();
        assert_eq!(
            check_repo_ownership(&unshadowed),
            Err(RepoOwnershipViolation::UnshadowedConfig {
                path: "/etc/pki/rpm-gpg/K".into()
            })
        );

        let mut stray = snap.clone();
        let row = &mut stray.config.as_mut().unwrap().files[2];
        row.shadowed_by = Some(ConfigShadow::Repositories);
        row.disposition = FindingKind::inventory();
        assert_eq!(
            check_repo_ownership(&stray),
            Err(RepoOwnershipViolation::ShadowWithoutOwner {
                path: "/etc/pki/rpm-gpg/other".into()
            })
        );

        let mut included = snap.clone();
        included.rpm.as_mut().unwrap().gpg_keys[1].disposition = FindingKind::included();
        assert_eq!(
            check_repo_ownership(&included),
            Err(RepoOwnershipViolation::UncarriableIncluded {
                path: "/etc/pki/rpm-gpg/ORPHAN".into()
            })
        );

        let key = "/etc/pki/rpm-gpg/K".to_owned();
        for content in ["not a key", PRIVATE, &format!("{PUBLIC}{PRIVATE}")] {
            let mut unusable = snap.clone();
            unusable.rpm.as_mut().unwrap().gpg_keys[0].content = content.into();
            assert_eq!(
                check_repo_ownership(&unusable),
                Err(RepoOwnershipViolation::UnusableKey { path: key.clone() }),
                "{content}"
            );
        }

        let mut rejected = snap.clone();
        let entry = &mut rejected.rpm.as_mut().unwrap().gpg_keys[0];
        entry.rejected = Some(KeyRejection::PrivateKeyMaterial);
        entry.content = KeyRejection::PrivateKeyMaterial.placeholder(&key);
        rejected.config.as_mut().unwrap().files[1].content = PRIVATE.into();
        assert_eq!(
            check_repo_ownership(&rejected),
            Err(RepoOwnershipViolation::RejectedKeyContent { path: key.clone() }),
            "a config row keeps the refused bytes"
        );
        settle_repo_ownership(&mut rejected);
        check_repo_ownership(&rejected).expect("settled");
        let mut raw = rejected.clone();
        raw.rpm.as_mut().unwrap().gpg_keys[0].content = format!("{PUBLIC}{PRIVATE}");
        assert_eq!(
            check_repo_ownership(&raw),
            Err(RepoOwnershipViolation::RejectedKeyContent { path: key.clone() }),
            "the rejected entry keeps the refused bytes"
        );
        let mut diff = rejected.clone();
        diff.config.as_mut().unwrap().files[1].diff_against_rpm = Some("+secret".into());
        assert_eq!(
            check_repo_ownership(&diff),
            Err(RepoOwnershipViolation::RejectedKeyContent { path: key.clone() }),
            "a diff of the refused bytes"
        );

        // The refused bytes put back in the key and in every twin alike,
        // with no PGP armor in them: the content is not the canonical
        // placeholder, whatever the twins agree on.
        let mut restored = rejected.clone();
        restored.rpm.as_mut().unwrap().gpg_keys[0].rejected = Some(KeyRejection::NotPgp);
        restored.rpm.as_mut().unwrap().gpg_keys[0].content = "plain text, not a key\n".into();
        for file in &mut restored.config.as_mut().unwrap().files {
            if file.path.ends_with("/K") {
                file.content = "plain text, not a key\n".into();
            }
        }
        assert_eq!(
            check_repo_ownership(&restored),
            Err(RepoOwnershipViolation::RejectedKeyContent { path: key.clone() }),
            "a NotPgp key holding its refused bytes"
        );
        let mut wrong_kind = rejected.clone();
        wrong_kind.rpm.as_mut().unwrap().gpg_keys[0].rejected = Some(KeyRejection::NotPgp);
        assert_eq!(
            check_repo_ownership(&wrong_kind),
            Err(RepoOwnershipViolation::RejectedKeyContent { path: key }),
            "another rejection's placeholder"
        );
    }

    /// Every config entry at a rejected key's destination, each spelling
    /// and variant, holds the key's placeholder after settling, and none
    /// keeps a diff; an entry the walk recorded empty stays empty.
    #[test]
    fn settle_replaces_a_rejected_keys_config_content_with_its_placeholder() {
        let mut snap = snapshot();
        let key = "/etc/pki/rpm-gpg/K";
        let placeholder = KeyRejection::PrivateKeyMaterial.placeholder(key);
        let entry = &mut snap.rpm.as_mut().unwrap().gpg_keys[0];
        entry.rejected = Some(KeyRejection::PrivateKeyMaterial);
        entry.content = placeholder.clone();
        entry.disposition = FindingKind::excluded();
        let files = &mut snap.config.as_mut().unwrap().files;
        files[1].content = PRIVATE.into();
        files[1].diff_against_rpm = Some(format!("+{PRIVATE}"));
        files.push(ConfigFileEntry {
            content: format!("{PUBLIC}{PRIVATE}"),
            ..config("/etc/pki/rpm-gpg//K")
        });
        files.push(config("etc/pki/rpm-gpg/K"));
        settle_repo_ownership(&mut snap);
        check_repo_ownership(&snap).expect("settled");
        let at_key: Vec<_> = snap.config.as_ref().unwrap().files[1..]
            .iter()
            .filter(|f| Destination::new(&f.path).unwrap().as_str() == key)
            .map(|f| (f.content.as_str(), f.diff_against_rpm.is_some()))
            .collect();
        assert_eq!(
            at_key,
            [
                (placeholder.as_str(), false),
                (placeholder.as_str(), false),
                ("", false)
            ]
        );
    }
}
