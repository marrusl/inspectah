use std::collections::{BTreeMap, BTreeSet};

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::repo::{self, RepoId, RepoSectionDef};

use crate::types::{RepoProvenance, RepoTier};

/// Re-export from core for backward compatibility.
pub use repo::DISTRO_REPOS;
/// Re-export from core for backward compatibility.
pub use repo::OFFICIAL_OPTIONAL_REPOS;

/// Index mapping repo section IDs to packages, repo file paths, and GPG keys.
/// Built once at session construction from snapshot data.
///
/// Keyed by [`RepoId`] throughout: a package's recorded `source_repo` and a
/// `.repo` file's `[section]` header fold ASCII case against each other, so
/// a mixed-case spelling on either side still resolves to the same entry.
pub struct RepoIndex {
    /// Packages grouped by their `source_repo` section ID.
    packages_by_repo: BTreeMap<RepoId, Vec<String>>,
    /// Repo file paths that define each section ID.
    repo_file_by_section: BTreeMap<RepoId, Vec<String>>,
    /// GPG key paths referenced by each section ID.
    gpg_keys_by_section: BTreeMap<RepoId, Vec<String>>,
    /// Reverse map: GPG key path -> set of section IDs that reference it.
    sections_by_gpg_key: BTreeMap<String, BTreeSet<RepoId>>,
    /// Computed provenance per section ID.
    provenance_map: BTreeMap<RepoId, RepoProvenance>,
}

impl RepoIndex {
    /// Build the index from snapshot data.
    pub fn build(snap: &InspectionSnapshot) -> Self {
        let rpm = match &snap.rpm {
            Some(r) => r,
            None => {
                return Self {
                    packages_by_repo: BTreeMap::new(),
                    repo_file_by_section: BTreeMap::new(),
                    gpg_keys_by_section: BTreeMap::new(),
                    sections_by_gpg_key: BTreeMap::new(),
                    provenance_map: BTreeMap::new(),
                };
            }
        };

        // 1. Parse repo files for sections and GPG key directives, using the
        // INI parser shared with collect (`inspectah_core::types::repo`).
        let mut repo_file_by_section: BTreeMap<RepoId, Vec<String>> = BTreeMap::new();
        let mut gpg_keys_by_section: BTreeMap<RepoId, Vec<String>> = BTreeMap::new();
        let mut sections_by_gpg_key: BTreeMap<String, BTreeSet<RepoId>> = BTreeMap::new();
        // Last section wins when the same id (folding case) appears more
        // than once; file/key associations above accumulate across all
        // occurrences regardless.
        let mut section_defs: BTreeMap<RepoId, RepoSectionDef> = BTreeMap::new();

        for repo_file in &rpm.repo_files {
            for section in repo::parse_repo_sections(&repo_file.content) {
                repo_file_by_section
                    .entry(section.id.clone())
                    .or_default()
                    .push(repo_file.path.clone());

                for key_path in &section.gpg_key_paths {
                    gpg_keys_by_section
                        .entry(section.id.clone())
                        .or_default()
                        .push(key_path.clone());

                    sections_by_gpg_key
                        .entry(key_path.clone())
                        .or_default()
                        .insert(section.id.clone());
                }

                section_defs.insert(section.id.clone(), section);
            }
        }

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

        // 3. Compute provenance for every section ID we know about.
        let captured_gpg_keys: BTreeSet<&str> =
            rpm.gpg_keys.iter().map(|k| k.path.as_str()).collect();

        let mut provenance_map: BTreeMap<RepoId, RepoProvenance> = BTreeMap::new();
        for (id, section) in &section_defs {
            let prov = if !section.enabled {
                RepoProvenance::Disabled
            } else if section.has_source
                && section
                    .gpg_key_paths
                    .iter()
                    .all(|p| captured_gpg_keys.contains(p.as_str()))
            {
                RepoProvenance::Verified
            } else {
                RepoProvenance::Incomplete
            };
            provenance_map.insert(id.clone(), prov);
        }

        // Section IDs referenced by packages but with no section on the
        // host are Stale -- likely removed or renamed since install.
        for id in packages_by_repo.keys() {
            provenance_map
                .entry(id.clone())
                .or_insert(RepoProvenance::Stale);
        }

        Self {
            packages_by_repo,
            repo_file_by_section,
            gpg_keys_by_section,
            sections_by_gpg_key,
            provenance_map,
        }
    }

    /// Packages grouped by their `source_repo` section ID.
    pub fn packages_by_repo(&self) -> &BTreeMap<RepoId, Vec<String>> {
        &self.packages_by_repo
    }

    /// Repo file paths that define each section ID.
    pub fn repo_file_by_section(&self) -> &BTreeMap<RepoId, Vec<String>> {
        &self.repo_file_by_section
    }

    /// GPG key paths referenced by each section ID.
    pub fn gpg_keys_by_section(&self) -> &BTreeMap<RepoId, Vec<String>> {
        &self.gpg_keys_by_section
    }

    /// Reverse map: GPG key path -> set of section IDs that reference it.
    pub fn sections_by_gpg_key(&self) -> &BTreeMap<String, BTreeSet<RepoId>> {
        &self.sections_by_gpg_key
    }

    /// Look up the provenance of a repo section ID.
    pub fn provenance(&self, id: &RepoId) -> RepoProvenance {
        if id.as_str().is_empty() || id.as_str().eq_ignore_ascii_case("@commandline") {
            return RepoProvenance::Unknown;
        }
        self.provenance_map
            .get(id)
            .copied()
            .unwrap_or(RepoProvenance::Unknown)
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

        let mut snap = InspectionSnapshot::new();
        snap.rpm =
            Some(RpmSection {
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
                content: "[appstream]\nname=CentOS AppStream\nbaseurl=http://example.invalid/appstream\n".into(), // Lowercase section
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
        assert!(
            index
                .packages_by_repo()
                .contains_key(&RepoId::new("AppStream"))
        );
        // Provenance should be Verified (matched the repo file section)
        assert_eq!(
            index.provenance(&RepoId::new("appstream")),
            RepoProvenance::Verified
        );
        // Should be recognized as distro
        assert!(RepoIndex::is_distro_repo("appstream"));
    }
}
