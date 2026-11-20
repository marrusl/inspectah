use std::collections::{BTreeMap, BTreeSet, HashSet};

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::repo::{self, RepoId};
use inspectah_core::types::rpm::RepoFile;

use crate::types::{RepoProvenance, RepoTier};

/// Re-export from core for backward compatibility.
pub use repo::DISTRO_REPOS;
/// Re-export from core for backward compatibility.
pub use repo::OFFICIAL_OPTIONAL_REPOS;

/// Index mapping repo section IDs to packages, repo file paths, and GPG keys.
/// Built once at session construction from snapshot data.
///
/// The inventory maps are keyed by [`RepoId`]: a package's recorded
/// `source_repo` and a `.repo` file's `[section]` header fold ASCII case
/// against each other, so a mixed-case spelling on either side still
/// resolves to the same entry. Provenance is judged on the one definition
/// dnf uses, per exact spelling ([`Self::provenance`]).
pub struct RepoIndex {
    /// Packages grouped by their `source_repo` section ID.
    packages_by_repo: BTreeMap<RepoId, Vec<String>>,
    /// Repo file paths that define each section ID.
    repo_file_by_section: BTreeMap<RepoId, Vec<String>>,
    /// GPG key paths referenced by each section ID: the `path` of each
    /// captured key the section names, or the path as written when no
    /// captured key answers it.
    gpg_keys_by_section: BTreeMap<RepoId, Vec<String>>,
    /// Repo file path -> the sections it defines that the host enabled.
    /// Only these keep the file carried ([`Self::repo_file_carried`]).
    enabled_sections_by_file: BTreeMap<String, BTreeSet<RepoId>>,
    /// GPG key path (as in `gpg_keys_by_section`) -> the host-enabled
    /// sections that name it. Only these keep the key carried
    /// ([`Self::gpg_key_carried`]).
    enabled_sections_by_gpg_key: BTreeMap<String, BTreeSet<RepoId>>,
    /// Provenance per section id, keyed by the id's exact spelling: dnf
    /// keys repos case-sensitively, so `[ab]` and `[AB]` each have their
    /// own. Read through [`Self::provenance`], which picks the spelling.
    provenance_map: BTreeMap<String, RepoProvenance>,
}

/// The paths of the captured keys a section's `gpgkey=` path names
/// ([`RepoFile::is_key_named_by`]), sorted and without repeats; the path
/// as written when none does. A path written with dnf variables names the
/// key the scan recorded at its expanded path, and in an aggregate it can
/// name one key per major.
fn captured_key_paths(keys: &[RepoFile], reference: &str) -> Vec<String> {
    let paths: BTreeSet<String> = keys
        .iter()
        .filter(|k| k.is_key_named_by(reference))
        .map(|k| k.path.clone())
        .collect();
    if paths.is_empty() {
        vec![reference.to_string()]
    } else {
        paths.into_iter().collect()
    }
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
                    enabled_sections_by_file: BTreeMap::new(),
                    enabled_sections_by_gpg_key: BTreeMap::new(),
                    provenance_map: BTreeMap::new(),
                };
            }
        };

        // 1. Parse repo files for sections and GPG key directives, using the
        // INI parser shared with collect (`inspectah_core::types::repo`).
        // These are the inventory of every defining file and key, which
        // exclusion needs whichever definition dnf uses; provenance is
        // judged on that one definition alone, in step 3.
        let mut repo_file_by_section: BTreeMap<RepoId, Vec<String>> = BTreeMap::new();
        let mut gpg_keys_by_section: BTreeMap<RepoId, Vec<String>> = BTreeMap::new();
        let mut enabled_sections_by_file: BTreeMap<String, BTreeSet<RepoId>> = BTreeMap::new();
        let mut enabled_sections_by_gpg_key: BTreeMap<String, BTreeSet<RepoId>> = BTreeMap::new();
        // Every exact spelling any file defines.
        let mut spellings: BTreeSet<String> = BTreeSet::new();

        for repo_file in &rpm.repo_files {
            for section in repo::parse_repo_sections(&repo_file.content) {
                repo_file_by_section
                    .entry(section.id.clone())
                    .or_default()
                    .push(repo_file.path.clone());
                // Recorded per file, not per id: the same id can be enabled
                // in one file and disabled in another.
                if section.enabled {
                    enabled_sections_by_file
                        .entry(repo_file.path.clone())
                        .or_default()
                        .insert(section.id.clone());
                }

                for key_path in section
                    .gpg_key_paths
                    .iter()
                    .flat_map(|reference| captured_key_paths(&rpm.gpg_keys, reference))
                {
                    gpg_keys_by_section
                        .entry(section.id.clone())
                        .or_default()
                        .push(key_path.clone());

                    if section.enabled {
                        enabled_sections_by_gpg_key
                            .entry(key_path)
                            .or_default()
                            .insert(section.id.clone());
                    }
                }

                spellings.insert(section.id.as_str().to_owned());
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

        // 3. Compute provenance for every section id, from the definition
        // dnf uses on the host (`effective_definition`: the first file in
        // dnf's read order), never from the order of `repo_files`, which is
        // the host's `read_dir` order. Each exact spelling is its own repo.
        let mut provenance_map: BTreeMap<String, RepoProvenance> = BTreeMap::new();
        for spelling in spellings {
            let Some(section) = repo::effective_definition(
                rpm.repo_files
                    .iter()
                    .map(|f| (f.path.as_str(), f.content.as_str())),
                &RepoId::new(spelling.as_str()),
            ) else {
                continue;
            };
            let prov = if !section.enabled {
                RepoProvenance::Disabled
            } else if section.has_source
                && section
                    .gpg_key_paths
                    .iter()
                    .all(|p| rpm.gpg_keys.iter().any(|k| k.is_key_named_by(p)))
            {
                RepoProvenance::Verified
            } else {
                RepoProvenance::Incomplete
            };
            provenance_map.insert(spelling, prov);
        }

        Self {
            packages_by_repo,
            repo_file_by_section,
            gpg_keys_by_section,
            enabled_sections_by_file,
            enabled_sections_by_gpg_key,
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

    /// Whether the repo file at `path` stays carried when the sections in
    /// `excluded` are excluded: at least one section the host enabled in it
    /// is not excluded.
    ///
    /// A section the host disabled does not count. The image never uses
    /// it, so it cannot be what keeps the file: EPEL's `epel.repo` holds
    /// `[epel]` beside the host-disabled `[epel-debuginfo]` and
    /// `[epel-source]`, and excluding `[epel]` has to drop the file. A file
    /// with no host-enabled section at all is therefore not carried once a
    /// repo exclusion reaches it.
    pub fn repo_file_carried(&self, path: &str, excluded: &HashSet<RepoId>) -> bool {
        self.enabled_sections_by_file
            .get(path)
            .is_some_and(|ids| ids.iter().any(|id| !excluded.contains(id)))
    }

    /// Whether the GPG key at `key_path` (a path as in
    /// [`Self::gpg_keys_by_section`]) stays carried when the sections in
    /// `excluded` are excluded: a section the host enabled names it and is
    /// not excluded, so the file holding that section is itself carried
    /// ([`Self::repo_file_carried`]). Sections the host disabled do not
    /// count, for the same reason they do not keep a file.
    pub fn gpg_key_carried(&self, key_path: &str, excluded: &HashSet<RepoId>) -> bool {
        self.enabled_sections_by_gpg_key
            .get(key_path)
            .is_some_and(|ids| ids.iter().any(|id| !excluded.contains(id)))
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
        let defined = self.provenance_map.keys().map(String::as_str);
        if let Some(spelling) = repo::section_spelling(defined, id.as_str()) {
            return self.provenance_map[spelling];
        }
        let any_variant = self
            .provenance_map
            .keys()
            .any(|spelling| spelling.eq_ignore_ascii_case(id.as_str()));
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
