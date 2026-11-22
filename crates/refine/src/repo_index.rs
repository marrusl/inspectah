use std::collections::{BTreeMap, BTreeSet, HashSet};

use inspectah_core::repo_ownership::{OwnedKind, RepoOwnership, repo_ownership};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::destination::Destination;
use inspectah_core::types::repo::{self, RepoId, SectionId};

use crate::types::{RepoProvenance, RepoTier};

/// Re-export from core for backward compatibility.
pub use repo::DISTRO_REPOS;
/// Re-export from core for backward compatibility.
pub use repo::OFFICIAL_OPTIONAL_REPOS;

/// Index mapping repo section IDs to packages, repo file paths, and GPG keys.
/// Built once at session construction from snapshot data.
///
/// `packages_by_repo` is keyed by [`RepoId`], which folds ASCII case, so a
/// package's recorded spelling groups with any case of it. Everything a
/// repo toggle acts on is keyed by the section's exact spelling
/// ([`SectionId`]) instead: dnf keys repos case-sensitively, so `[site]`
/// and `[SITE]` are two repos with their own files and keys. A toggle and
/// a package each name one of them through [`Self::repo_identity`].
/// Provenance is judged on the one definition dnf uses, per exact spelling
/// ([`Self::provenance`]).
///
/// Who owns which file and key is core's [`repo_ownership`], read once:
/// the session never changes repo file content, so the ownership the
/// scan's last pass settled is the ownership every projection has.
pub struct RepoIndex {
    /// Packages grouped by their `source_repo` section ID.
    packages_by_repo: BTreeMap<RepoId, Vec<String>>,
    /// Who owns each destination of `rpm.repo_files` and `rpm.gpg_keys`.
    ownership: RepoOwnership,
    /// Repo file destinations that define each section, by exact spelling.
    repo_file_by_section: BTreeMap<SectionId, Vec<String>>,
    /// GPG key destinations each section's `gpgkey=` names, by exact
    /// spelling. A reference that names no captured key is not here.
    gpg_keys_by_section: BTreeMap<SectionId, Vec<String>>,
    /// Provenance per section id, keyed by the id's exact spelling: dnf
    /// keys repos case-sensitively, so `[ab]` and `[AB]` each have their
    /// own. Read through [`Self::provenance`], which picks the spelling.
    provenance_map: BTreeMap<SectionId, RepoProvenance>,
    /// Sections whose switch cascades to members (`repo::toggleable`).
    toggleable: BTreeSet<SectionId>,
}

/// A section that keeps an entry for an excluded owner (§ 6.1 of the repo
/// ownership spec).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Keeper {
    pub section: SectionId,
    /// Set when the keeper is host-disabled and keeps the entry for this
    /// included package's `--enablerepo` install.
    pub enablerepo_package: Option<String>,
}

impl RepoIndex {
    /// Build the index from snapshot data.
    pub fn build(snap: &InspectionSnapshot) -> Self {
        let rpm = match &snap.rpm {
            Some(r) => r,
            None => {
                return Self {
                    packages_by_repo: BTreeMap::new(),
                    ownership: RepoOwnership::default(),
                    repo_file_by_section: BTreeMap::new(),
                    gpg_keys_by_section: BTreeMap::new(),
                    provenance_map: BTreeMap::new(),
                    toggleable: BTreeSet::new(),
                };
            }
        };

        // 1. Ownership, from the one parser and the one matcher in core.
        let ownership = repo_ownership(rpm);
        let mut repo_file_by_section: BTreeMap<SectionId, Vec<String>> = BTreeMap::new();
        let mut gpg_keys_by_section: BTreeMap<SectionId, Vec<String>> = BTreeMap::new();
        for (dest, entry) in ownership.entries() {
            let map = match entry.kind {
                OwnedKind::RepoFile => &mut repo_file_by_section,
                OwnedKind::GpgKey => &mut gpg_keys_by_section,
            };
            for section in entry.sections() {
                map.entry(section)
                    .or_default()
                    .push(dest.as_str().to_owned());
            }
        }
        // Every exact spelling any file defines.
        let spellings: BTreeSet<SectionId> = rpm
            .repo_files
            .iter()
            .flat_map(|f| repo::parse_repo_sections(&f.content))
            .map(|sec| sec.id)
            .collect();

        // 2. Map packages by source_repo. RepoId folds ASCII case, so a
        // package's recorded spelling (e.g. dnf repoquery's "AppStream")
        // matches a section header of any case (e.g. "appstream") without
        // normalization here.
        let mut packages_by_repo: BTreeMap<RepoId, Vec<String>> = BTreeMap::new();
        for pkg in &rpm.packages_added {
            if !pkg.source_repo.is_empty() {
                packages_by_repo
                    .entry(RepoId::new(pkg.source_repo.clone()))
                    .or_default()
                    .push(pkg.name.clone());
            }
        }

        // 3. Compute provenance for every section id, from the definition
        // dnf uses on the host (`effective_definition`: the first file in
        // dnf's read order), never from the order of `repo_files`, which is
        // the host's `read_dir` order. Each exact spelling is its own repo.
        // A key the scan rejected is not captured.
        let files = || {
            rpm.repo_files
                .iter()
                .map(|f| (f.path.as_str(), f.content.as_str()))
        };
        let mut provenance_map: BTreeMap<SectionId, RepoProvenance> = BTreeMap::new();
        let mut toggleable: BTreeSet<SectionId> = BTreeSet::new();
        for spelling in spellings {
            let Some(section) =
                repo::effective_definition(files(), &RepoId::new(spelling.as_str()))
            else {
                continue;
            };
            let prov = if !section.enabled {
                RepoProvenance::Disabled
            } else if section.has_source
                && section.local_keys().all(|p| {
                    rpm.gpg_keys
                        .iter()
                        .any(|k| k.rejected.is_none() && k.is_key_named_by(p))
                })
            {
                RepoProvenance::Verified
            } else {
                RepoProvenance::Incomplete
            };
            if repo::toggleable(files(), spelling.as_str()) {
                toggleable.insert(spelling.clone());
            }
            provenance_map.insert(spelling, prov);
        }

        Self {
            packages_by_repo,
            ownership,
            repo_file_by_section,
            gpg_keys_by_section,
            provenance_map,
            toggleable,
        }
    }

    /// Who owns each destination of `rpm.repo_files` and `rpm.gpg_keys`.
    pub fn ownership(&self) -> &RepoOwnership {
        &self.ownership
    }

    /// Whether `repo`'s switch cascades to its members: not a distro repo,
    /// and the definition the host's dnf uses for it is enabled.
    pub fn is_toggleable(&self, repo: &str) -> bool {
        self.toggleable.contains(&SectionId::new(repo))
    }

    /// Every section any repo file defines, by exact spelling.
    pub fn sections(&self) -> impl Iterator<Item = &SectionId> {
        self.provenance_map.keys()
    }

    /// The sections that keep the entry at `dest` (§ 6.1) when the
    /// sections in `excluded`, by exact spelling, are excluded: each owner
    /// not excluded and live in `snapshot`, which is the host enabling it
    /// in the file that defines it, or the host disabling it while an
    /// included, unlocked package attributed to it installs with
    /// `--enablerepo` and needs the file and its keys.
    pub fn kept_by(
        &self,
        dest: &Destination,
        excluded: &HashSet<String>,
        snapshot: &InspectionSnapshot,
    ) -> Vec<Keeper> {
        let Some(entry) = self.ownership.get(dest) else {
            return Vec::new();
        };
        let mut keepers: BTreeSet<Keeper> = BTreeSet::new();
        let mut seen: BTreeSet<&SectionId> = BTreeSet::new();
        let mut owners: Vec<&inspectah_core::repo_ownership::Owner> = entry.owners.iter().collect();
        // An enabled definition of a section decides before a disabled one.
        owners.sort_by_key(|o| !o.enabled);
        for owner in owners {
            if excluded.contains(owner.section.as_str()) || !seen.insert(&owner.section) {
                continue;
            }
            if owner.enabled {
                keepers.insert(Keeper {
                    section: owner.section.clone(),
                    enablerepo_package: None,
                });
            } else if let Some(pkg) = self.enablerepo_package(&owner.section, snapshot) {
                keepers.insert(Keeper {
                    section: owner.section.clone(),
                    enablerepo_package: Some(pkg),
                });
            }
        }
        keepers.into_iter().collect()
    }

    /// The least-named included, unlocked package attributed to `section`
    /// in `snapshot`, which installs with `--enablerepo=<section>` while
    /// the host has the section disabled.
    fn enablerepo_package(
        &self,
        section: &SectionId,
        snapshot: &InspectionSnapshot,
    ) -> Option<String> {
        snapshot
            .rpm
            .as_ref()?
            .packages_added
            .iter()
            .filter(|p| p.disposition.is_included() && !p.locked)
            .filter(|p| self.repo_identity(&p.source_repo) == section.as_str())
            .map(|p| p.name.clone())
            .min()
    }

    /// Packages grouped by their `source_repo` section ID.
    pub fn packages_by_repo(&self) -> &BTreeMap<RepoId, Vec<String>> {
        &self.packages_by_repo
    }

    /// Repo file destinations that define each section, keyed by the
    /// section's exact spelling ([`Self::repo_identity`] names the key).
    pub fn repo_file_by_section(&self) -> &BTreeMap<SectionId, Vec<String>> {
        &self.repo_file_by_section
    }

    /// GPG key destinations each section names, keyed by the section's
    /// exact spelling ([`Self::repo_identity`] names the key).
    pub fn gpg_keys_by_section(&self) -> &BTreeMap<SectionId, Vec<String>> {
        &self.gpg_keys_by_section
    }

    /// The repo a toggle target or a package's recorded `source_repo`
    /// names: the section spelled exactly as `recorded`, or the one case
    /// variant of it any file defines ([`repo::section_spelling`]). When no
    /// section answers, because none is defined or two case variants
    /// compete, it is `recorded` itself, so it matches nothing but the same
    /// spelling.
    ///
    /// This is the one identity the repo-toggle cascade works in: which
    /// packages, files and keys a toggle reaches, which repos are excluded,
    /// and the effective-member counts.
    pub fn repo_identity<'a>(&'a self, recorded: &'a str) -> &'a str {
        repo::section_spelling(self.provenance_map.keys().map(SectionId::as_str), recorded)
            .unwrap_or(recorded)
    }

    /// Look up the provenance of a repo section ID.
    ///
    /// The section is the one spelled as `id`, or the one case variant of
    /// it ([`repo::section_spelling`]). Two or more case variants and no
    /// exact spelling is `Unknown`: no one section answers. An id with no
    /// section that a package references is `Stale`, likely removed or
    /// renamed since install.
    pub fn provenance(&self, id: &RepoId) -> RepoProvenance {
        if id.as_str().is_empty() || id.as_str().eq_ignore_ascii_case("@commandline") {
            return RepoProvenance::Unknown;
        }
        let defined = self.provenance_map.keys().map(SectionId::as_str);
        if let Some(spelling) = repo::section_spelling(defined, id.as_str()) {
            return self.provenance_map[&SectionId::new(spelling)];
        }
        let any_variant = self
            .provenance_map
            .keys()
            .any(|spelling| spelling.as_str().eq_ignore_ascii_case(id.as_str()));
        if !any_variant && self.packages_by_repo.contains_key(id) {
            RepoProvenance::Stale
        } else {
            RepoProvenance::Unknown
        }
    }

    /// Check whether a section ID is a well-known distro repo.
    ///
    /// Matches both short IDs (`baseos`) and RHEL-style long IDs
    /// (`rhel-9-for-x86_64-baseos-rpms`) by delegating to `repo_tier`.
    pub fn is_distro_repo(section_id: &str) -> bool {
        matches!(repo::repo_tier(section_id), repo::RepoTier::Distro)
    }

    /// Classify a repo section ID into its tier.
    ///
    /// Delegates to the canonical implementation in `inspectah_core::types::repo`.
    pub fn repo_tier(section_id: &str) -> RepoTier {
        match repo::repo_tier(section_id) {
            repo::RepoTier::Distro => RepoTier::Distro,
            repo::RepoTier::OfficialOptional => RepoTier::OfficialOptional,
            repo::RepoTier::ThirdParty => RepoTier::ThirdParty,
            repo::RepoTier::None => RepoTier::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RepoTier;
    use inspectah_core::types::FindingKind;

    #[test]
    fn test_is_distro_repo() {
        assert!(RepoIndex::is_distro_repo("baseos"));
        assert!(RepoIndex::is_distro_repo("appstream"));
        assert!(RepoIndex::is_distro_repo("BaseOS")); // case-insensitive
        assert!(RepoIndex::is_distro_repo("updates-testing"));
        assert!(RepoIndex::is_distro_repo("extras"));
        assert!(!RepoIndex::is_distro_repo("epel"));
        assert!(!RepoIndex::is_distro_repo("custom-internal"));
        assert!(!RepoIndex::is_distro_repo("crb")); // CRB is now official-optional
        // RHEL-style long repo IDs
        assert!(RepoIndex::is_distro_repo("rhel-9-for-x86_64-baseos-rpms"));
        assert!(RepoIndex::is_distro_repo(
            "rhel-9-for-x86_64-appstream-rpms"
        ));
    }

    #[test]
    fn test_repo_tier() {
        assert_eq!(RepoIndex::repo_tier("baseos"), RepoTier::Distro);
        assert_eq!(RepoIndex::repo_tier("appstream"), RepoTier::Distro);
        assert_eq!(RepoIndex::repo_tier("AppStream"), RepoTier::Distro); // case-insensitive
        assert_eq!(RepoIndex::repo_tier("updates-testing"), RepoTier::Distro);
        assert_eq!(RepoIndex::repo_tier("extras"), RepoTier::Distro);
        assert_eq!(RepoIndex::repo_tier("crb"), RepoTier::OfficialOptional);
        assert_eq!(RepoIndex::repo_tier("CRB"), RepoTier::OfficialOptional); // case-insensitive
        assert_eq!(
            RepoIndex::repo_tier("rhel-extensions"),
            RepoTier::OfficialOptional
        );
        assert_eq!(RepoIndex::repo_tier("epel"), RepoTier::ThirdParty);
        assert_eq!(RepoIndex::repo_tier("copr:mytools"), RepoTier::ThirdParty);
        assert_eq!(RepoIndex::repo_tier(""), RepoTier::None);
        assert_eq!(RepoIndex::repo_tier("@commandline"), RepoTier::None);
        assert_eq!(RepoIndex::repo_tier("@CommandLine"), RepoTier::None);
    }

    #[test]
    fn test_commandline_provenance_is_unknown() {
        use inspectah_core::snapshot::InspectionSnapshot;
        use inspectah_core::types::rpm::{PackageEntry, PackageState, RepoFile, RpmSection};

        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "custom-tool".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "@commandline".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            repo_files: vec![RepoFile {
                path: "/etc/yum.repos.d/centos.repo".into(),
                content: "[baseos]\nname=CentOS BaseOS\n".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let index = RepoIndex::build(&snap);
        assert_eq!(
            index.provenance(&RepoId::new("@commandline")),
            RepoProvenance::Unknown,
            "@commandline should be Unknown provenance"
        );
    }

    #[test]
    fn test_repo_index_case_insensitive() {
        use inspectah_core::snapshot::InspectionSnapshot;
        use inspectah_core::types::rpm::{PackageEntry, PackageState, RepoFile, RpmSection};

        // Lowercase section header; package recorded with dnf repoquery's
        // mixed-case spelling.
        let content =
            "[appstream]\nname=CentOS AppStream\nbaseurl=http://example.invalid/appstream\n";

        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "AppStream".into(), // Mixed case from dnf repoquery
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            repo_files: vec![RepoFile {
                path: "/etc/yum.repos.d/centos.repo".into(),
                content: content.into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let index = RepoIndex::build(&snap);
        // Package recorded as "AppStream" folds to the same RepoId as the
        // lowercase "appstream" section header -- lookups match either way.
        assert!(
            index
                .packages_by_repo()
                .contains_key(&RepoId::new("appstream"))
        );
        // The behavior under test: looking up the package's own (mixed-case)
        // spelling now resolves against the lowercase section header. Before
        // this task's RepoId-keying fix, this returned Unknown.
        assert_eq!(
            index.provenance(&RepoId::new("AppStream")),
            RepoProvenance::Verified
        );
        // Should be recognized as distro
        assert!(RepoIndex::is_distro_repo("appstream"));
    }
}
