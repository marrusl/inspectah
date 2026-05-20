use std::collections::{BTreeMap, BTreeSet};

use inspectah_core::snapshot::InspectionSnapshot;

use crate::types::RepoProvenance;

/// Repo section IDs considered part of the base distribution.
pub const DISTRO_REPOS: &[&str] = &[
    "baseos",
    "appstream",
    "crb",
    "fedora",
    "updates",
    "anaconda",
];

/// A parsed INI section from a repo file.
struct RepoSection {
    /// The section ID (e.g., "baseos", "epel").
    id: String,
    /// GPG key file paths extracted from `gpgkey=file:///...` directives.
    gpg_key_paths: Vec<String>,
}

/// Index mapping repo section IDs to packages, repo file paths, and GPG keys.
/// Built once at session construction from snapshot data.
pub struct RepoIndex {
    /// Packages grouped by their `source_repo` section ID.
    pub packages_by_repo: BTreeMap<String, Vec<String>>,
    /// Repo file paths that define each section ID.
    pub repo_file_by_section: BTreeMap<String, Vec<String>>,
    /// GPG key paths referenced by each section ID.
    pub gpg_keys_by_section: BTreeMap<String, Vec<String>>,
    /// Reverse map: GPG key path -> set of section IDs that reference it.
    pub sections_by_gpg_key: BTreeMap<String, BTreeSet<String>>,
    /// Computed provenance per section ID.
    provenance_map: BTreeMap<String, RepoProvenance>,
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

        // 1. Parse repo files for INI sections and GPG key directives.
        let mut repo_file_by_section: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut gpg_keys_by_section: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut sections_by_gpg_key: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

        for repo_file in &rpm.repo_files {
            let sections = parse_repo_sections(&repo_file.content);
            for section in sections {
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
            }
        }

        // 2. Map packages by source_repo (normalized to lowercase).
        // The Go scanner emits mixed-case repo IDs (e.g. "AppStream")
        // while INI section headers from .repo files are already lowercase.
        // Normalizing here ensures packages match their repo definitions.
        let mut packages_by_repo: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for pkg in &rpm.packages_added {
            if !pkg.source_repo.is_empty() {
                packages_by_repo
                    .entry(pkg.source_repo.to_lowercase())
                    .or_default()
                    .push(pkg.name.clone());
            }
        }

        // 3. Compute provenance for every section ID we know about.
        let mut provenance_map = BTreeMap::new();

        // All section IDs from repo files are Verified.
        for section_id in repo_file_by_section.keys() {
            provenance_map.insert(section_id.clone(), RepoProvenance::Verified);
        }

        // Section IDs referenced by packages but not found in repo files
        // are Incomplete.
        for section_id in packages_by_repo.keys() {
            provenance_map
                .entry(section_id.clone())
                .or_insert(RepoProvenance::Incomplete);
        }

        Self {
            packages_by_repo,
            repo_file_by_section,
            gpg_keys_by_section,
            sections_by_gpg_key,
            provenance_map,
        }
    }

    /// Look up the provenance of a repo section ID.
    pub fn provenance(&self, section_id: &str) -> RepoProvenance {
        if section_id.is_empty() {
            return RepoProvenance::Unknown;
        }
        self.provenance_map
            .get(section_id)
            .copied()
            .unwrap_or(RepoProvenance::Unknown)
    }

    /// Check whether a section ID is a well-known distro repo.
    pub fn is_distro_repo(section_id: &str) -> bool {
        DISTRO_REPOS.contains(&section_id)
    }
}

/// Parse INI-style repo file content into sections with GPG key paths.
fn parse_repo_sections(content: &str) -> Vec<RepoSection> {
    let mut sections = Vec::new();
    let mut current_id: Option<String> = None;
    let mut current_keys: Vec<String> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Section header: [section_name]
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            // Flush previous section.
            if let Some(id) = current_id.take() {
                sections.push(RepoSection {
                    id,
                    gpg_key_paths: std::mem::take(&mut current_keys),
                });
            }
            current_id = Some(trimmed[1..trimmed.len() - 1].to_string());
            continue;
        }

        // gpgkey directive (only meaningful inside a section).
        if current_id.is_some()
            && let Some(value) = trimmed
                .strip_prefix("gpgkey=")
                .or_else(|| trimmed.strip_prefix("gpgkey ="))
        {
            // Values can be comma- or whitespace-separated.
            for part in value.split(|c: char| c == ',' || c.is_ascii_whitespace()) {
                let part = part.trim();
                if let Some(path) = part.strip_prefix("file://")
                    && !path.is_empty()
                {
                    current_keys.push(path.to_string());
                }
            }
        }
    }

    // Flush last section.
    if let Some(id) = current_id.take() {
        sections.push(RepoSection {
            id,
            gpg_key_paths: current_keys,
        });
    }

    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_section() {
        let content = "[baseos]\nname=CentOS BaseOS\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial\n";
        let sections = parse_repo_sections(content);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].id, "baseos");
        assert_eq!(sections[0].gpg_key_paths.len(), 1);
        assert_eq!(
            sections[0].gpg_key_paths[0],
            "/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial"
        );
    }

    #[test]
    fn test_parse_multiple_sections() {
        let content = "[baseos]\ngpgkey=file:///key1\n\n[appstream]\ngpgkey=file:///key1\n";
        let sections = parse_repo_sections(content);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].id, "baseos");
        assert_eq!(sections[1].id, "appstream");
    }

    #[test]
    fn test_parse_multiple_gpg_keys() {
        let content = "[myrepo]\ngpgkey=file:///key1,file:///key2\n";
        let sections = parse_repo_sections(content);
        assert_eq!(sections[0].gpg_key_paths.len(), 2);
        assert_eq!(sections[0].gpg_key_paths[0], "/key1");
        assert_eq!(sections[0].gpg_key_paths[1], "/key2");
    }

    #[test]
    fn test_parse_no_gpgkey() {
        let content = "[myrepo]\nname=My Repo\nbaseurl=http://example.com\n";
        let sections = parse_repo_sections(content);
        assert_eq!(sections.len(), 1);
        assert!(sections[0].gpg_key_paths.is_empty());
    }

    #[test]
    fn test_is_distro_repo() {
        assert!(RepoIndex::is_distro_repo("baseos"));
        assert!(RepoIndex::is_distro_repo("appstream"));
        assert!(!RepoIndex::is_distro_repo("epel"));
        assert!(!RepoIndex::is_distro_repo("custom-internal"));
    }

    #[test]
    fn test_repo_index_case_insensitive() {
        use inspectah_core::snapshot::InspectionSnapshot;
        use inspectah_core::types::rpm::{PackageEntry, PackageState, RepoFile, RpmSection};

        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "AppStream".into(), // Mixed case from Go scanner
                include: true,
                ..Default::default()
            }],
            repo_files: vec![RepoFile {
                path: "/etc/yum.repos.d/centos.repo".into(),
                content: "[appstream]\nname=CentOS AppStream\n".into(), // Lowercase section
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let index = RepoIndex::build(&snap);
        // Package with "AppStream" should map to lowercase "appstream"
        assert!(
            index.packages_by_repo.contains_key("appstream"),
            "packages_by_repo should use lowercase key"
        );
        assert!(
            !index.packages_by_repo.contains_key("AppStream"),
            "mixed-case key should not exist"
        );
        // Provenance should be Verified (matched the repo file section)
        assert_eq!(index.provenance("appstream"), RepoProvenance::Verified);
        // Should be recognized as distro
        assert!(RepoIndex::is_distro_repo("appstream"));
    }
}
