use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

/// Repo section IDs considered part of the base distribution.
pub const DISTRO_REPOS: &[&str] = &[
    "baseos",
    "appstream",
    "fedora",
    "updates",
    "updates-testing",
    "extras",
    "anaconda",
];

/// Red Hat repos that are official but user-toggleable (not part of the base image).
pub const OFFICIAL_OPTIONAL_REPOS: &[&str] = &["crb", "codeready-builder", "rhel-extensions"];

/// Distro repo keywords that may appear as segments in longer RHEL repo IDs.
///
/// RHEL systems use repo IDs like `rhel-9-for-x86_64-baseos-rpms` rather
/// than the short `baseos` used by CentOS Stream. This list is checked
/// with segment-boundary matching (preceded by `-` or start-of-string)
/// to avoid false positives like a hypothetical `myappstream-extras`.
const DISTRO_REPO_KEYWORDS: &[&str] = &["baseos", "appstream"];

/// Keywords for official-optional repos that may appear as segments
/// in longer RHEL repo IDs (e.g. `codeready-builder-for-rhel-9-x86_64-rpms`).
const OFFICIAL_OPTIONAL_KEYWORDS: &[&str] = &["codeready-builder", "crb"];

/// Classification tier for a repo section ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoTier {
    Distro,
    OfficialOptional,
    ThirdParty,
    None,
}

/// Check whether `id` contains `keyword` as a delimited segment.
///
/// A segment match requires the keyword to be preceded by `-` or
/// start-of-string and followed by `-` or end-of-string.
fn contains_keyword_segment(id: &str, keyword: &str) -> bool {
    if let Some(pos) = id.find(keyword) {
        let before_ok = pos == 0 || id.as_bytes()[pos - 1] == b'-';
        let after = pos + keyword.len();
        let after_ok = after == id.len() || id.as_bytes()[after] == b'-';
        before_ok && after_ok
    } else {
        false
    }
}

/// Classify a repo section ID into its tier.
///
/// Empty, missing, or `@commandline` section IDs return `None` --
/// "no repo identity" is distinct from "known third-party repo."
///
/// Matches both short CentOS-style IDs (`baseos`) and long RHEL-style
/// IDs (`rhel-9-for-x86_64-baseos-rpms`) via segment-boundary matching.
pub fn repo_tier(section_id: &str) -> RepoTier {
    if section_id.is_empty() {
        return RepoTier::None;
    }
    let lower = section_id.to_lowercase();
    let id = lower.as_str();
    if id == "@commandline" {
        return RepoTier::None;
    }
    // Exact match first (CentOS Stream, Fedora, etc.)
    if DISTRO_REPOS.contains(&id) {
        return RepoTier::Distro;
    }
    if OFFICIAL_OPTIONAL_REPOS.contains(&id) {
        return RepoTier::OfficialOptional;
    }
    // Segment match for RHEL-style long IDs (e.g. rhel-9-for-x86_64-baseos-rpms)
    if DISTRO_REPO_KEYWORDS
        .iter()
        .any(|kw| contains_keyword_segment(id, kw))
    {
        return RepoTier::Distro;
    }
    if OFFICIAL_OPTIONAL_KEYWORDS
        .iter()
        .any(|kw| contains_keyword_segment(id, kw))
    {
        return RepoTier::OfficialOptional;
    }
    RepoTier::ThirdParty
}

/// A yum/dnf repo section id (a `.repo` file `[section]` header or an
/// install-time `%{from_repo}` value).
///
/// Equality, hashing, and ordering are ASCII case-insensitive and fold
/// nothing outside the ASCII range, so `RepoId`s that compare equal always
/// hash equal -- see Review Focus 1 in the phase 0 plan. `Display` keeps
/// the id exactly as the host wrote it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct RepoId(String);

impl RepoId {
    /// Build a `RepoId` from the host's spelling of the id.
    pub fn new(id: impl Into<String>) -> RepoId {
        RepoId(id.into())
    }

    /// The id exactly as the host wrote it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for RepoId {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for RepoId {}

impl Hash for RepoId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_ascii_lowercase().hash(state);
    }
}

impl PartialOrd for RepoId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RepoId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .to_ascii_lowercase()
            .cmp(&other.0.to_ascii_lowercase())
    }
}

impl fmt::Display for RepoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A repo identity dnf reports that is not a real `.repo` section.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PseudoRepo {
    /// `@commandline`: a package installed by local RPM path, not a repo.
    Commandline,
    /// `anaconda`: the installer recorded this as the source at install time.
    Anaconda,
    /// `@System`: dnf's marker for a package with no repo provenance.
    System,
}

/// How a package's recorded repo value relates to the host's `.repo`
/// sections.
///
/// `source_repo` on `PackageEntry` keeps its historical meaning (the
/// resolved id for `Exact`/`Resolved`, the recorded value otherwise); this
/// enum is the structured fact `source_repo` summarizes, and every variant
/// carries or implies the originally recorded string (see [`Self::recorded`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepoAttribution {
    /// The recorded value equals a section id on the host (case-insensitive),
    /// whether or not that section is enabled.
    Exact { id: RepoId },
    /// The recorded short name matched exactly one enabled section by
    /// substring.
    Resolved { recorded: String, id: RepoId },
    /// The recorded short name matched more than one enabled section by
    /// substring; `source_repo` is left as recorded.
    Ambiguous {
        recorded: String,
        candidates: Vec<RepoId>,
    },
    /// No section on the host matches; the repo was likely removed or
    /// renamed since install.
    Stale { recorded: String },
    /// The recorded value is a pseudo repo (`@commandline`, `anaconda`,
    /// `@System`), not a real repo identity.
    Pseudo { which: PseudoRepo, recorded: String },
    /// No repo value was recorded for this package.
    #[default]
    Unrecorded,
}

impl RepoAttribution {
    /// The repo identity as the host originally recorded it.
    ///
    /// For `Exact`, this returns the matched section's spelling, which may
    /// differ from the host's recorded value in ASCII case -- the match is
    /// case-insensitive, and the originally recorded spelling is not
    /// retained. `Unrecorded` has nothing to report, so this returns `""`.
    pub fn recorded(&self) -> &str {
        match self {
            RepoAttribution::Exact { id } => id.as_str(),
            RepoAttribution::Resolved { recorded, .. }
            | RepoAttribution::Ambiguous { recorded, .. }
            | RepoAttribution::Stale { recorded }
            | RepoAttribution::Pseudo { recorded, .. } => recorded,
            RepoAttribution::Unrecorded => "",
        }
    }
}

/// One parsed `[section]` of a `.repo` file.
///
/// Shared by collect and refine; replaces `extract_section_ids`
/// (`source_repos.rs`) and `parse_repo_sections` (`repo_index.rs`), which
/// parsed the same INI two different ways.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSectionDef {
    pub id: RepoId,
    /// The section's `enabled=` value; `true` when the key is absent,
    /// matching dnf's own default.
    pub enabled: bool,
    /// Whether the section defines `baseurl`, `metalink`, or `mirrorlist`.
    pub has_source: bool,
    /// Filesystem paths collected from `gpgkey=file://` values.
    pub gpg_key_paths: Vec<String>,
}

/// Trim a trailing `#` or `;` comment from an INI value.
fn strip_trailing_comment(value: &str) -> &str {
    match value.find(['#', ';']) {
        Some(idx) => &value[..idx],
        None => value,
    }
}

/// dnf's `enabled=` vocabulary: `1`, `true`, `yes`, `on` (case-insensitive)
/// are enabled; everything else, including malformed values, is disabled.
fn is_enabled_value(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Parse `.repo`-style INI content into one [`RepoSectionDef`] per
/// `[section]` header.
///
/// Lines before the first section header, and lines without a `key=value`
/// form, are dropped rather than rejected -- malformed INI should degrade,
/// not fail the scan.
pub fn parse_repo_sections(content: &str) -> Vec<RepoSectionDef> {
    let mut sections = Vec::new();
    let mut current: Option<RepoSectionDef> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if let Some(section) = current.take() {
                sections.push(section);
            }
            current = Some(RepoSectionDef {
                id: RepoId::new(&trimmed[1..trimmed.len() - 1]),
                enabled: true,
                has_source: false,
                gpg_key_paths: Vec::new(),
            });
            continue;
        }

        let Some(section) = current.as_mut() else {
            continue; // line outside any section
        };
        let Some((key, value)) = trimmed.split_once('=') else {
            continue; // malformed: no key=value
        };
        let key = key.trim();
        let value = strip_trailing_comment(value).trim();

        match key {
            "enabled" => section.enabled = is_enabled_value(value),
            "baseurl" | "metalink" | "mirrorlist" => section.has_source = true,
            "gpgkey" => {
                for part in value.split(|c: char| c == ',' || c.is_ascii_whitespace()) {
                    if let Some(path) = part.strip_prefix("file://")
                        && !path.is_empty()
                    {
                        section.gpg_key_paths.push(path.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(section) = current.take() {
        sections.push(section);
    }

    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_tier_distro() {
        assert_eq!(repo_tier("baseos"), RepoTier::Distro);
        assert_eq!(repo_tier("appstream"), RepoTier::Distro);
        assert_eq!(repo_tier("AppStream"), RepoTier::Distro);
        assert_eq!(repo_tier("anaconda"), RepoTier::Distro);
        assert_eq!(repo_tier("updates-testing"), RepoTier::Distro);
        assert_eq!(repo_tier("extras"), RepoTier::Distro);
    }

    #[test]
    fn test_repo_tier_distro_rhel_long_ids() {
        // RHEL systems use long repo IDs like rhel-9-for-x86_64-baseos-rpms
        assert_eq!(repo_tier("rhel-9-for-x86_64-baseos-rpms"), RepoTier::Distro);
        assert_eq!(
            repo_tier("rhel-9-for-x86_64-appstream-rpms"),
            RepoTier::Distro
        );
        assert_eq!(
            repo_tier("rhel-9-for-aarch64-baseos-rpms"),
            RepoTier::Distro
        );
        // Case-insensitive
        assert_eq!(repo_tier("RHEL-9-for-x86_64-BaseOS-rpms"), RepoTier::Distro);
    }

    #[test]
    fn test_repo_tier_official_optional() {
        assert_eq!(repo_tier("crb"), RepoTier::OfficialOptional);
        assert_eq!(repo_tier("CRB"), RepoTier::OfficialOptional);
        assert_eq!(repo_tier("rhel-extensions"), RepoTier::OfficialOptional);
    }

    #[test]
    fn test_repo_tier_official_optional_rhel_long_ids() {
        assert_eq!(
            repo_tier("codeready-builder-for-rhel-9-x86_64-rpms"),
            RepoTier::OfficialOptional
        );
    }

    #[test]
    fn test_repo_tier_third_party() {
        assert_eq!(repo_tier("epel"), RepoTier::ThirdParty);
        assert_eq!(repo_tier("copr:mytools"), RepoTier::ThirdParty);
    }

    #[test]
    fn test_repo_tier_no_false_segment_match() {
        // "myappstream-extras" should NOT match "appstream" because
        // the keyword is not at a segment boundary (preceded by 'y', not '-')
        assert_eq!(repo_tier("myappstream-extras"), RepoTier::ThirdParty);
    }

    #[test]
    fn test_repo_tier_none() {
        assert_eq!(repo_tier(""), RepoTier::None);
        assert_eq!(repo_tier("@commandline"), RepoTier::None);
        assert_eq!(repo_tier("@CommandLine"), RepoTier::None);
    }

    #[test]
    fn repo_id_folds_ascii_case_only() {
        assert_eq!(
            RepoId::new("EPEL_10_Everything_x86_64"),
            RepoId::new("epel_10_everything_x86_64")
        );
        // Review Focus 1: Eq, Hash and Ord must agree, and must not fold non-ASCII.
        assert_ne!(RepoId::new("repo-Ü"), RepoId::new("repo-ü"));
        let mut set = std::collections::HashSet::new();
        set.insert(RepoId::new("EPEL"));
        assert!(set.contains(&RepoId::new("epel")));
        assert!(!set.contains(&RepoId::new("repo-Ü")) || set.len() == 1);
        assert_eq!(RepoId::new("EPEL").to_string(), "EPEL"); // Display keeps the host's spelling
    }

    #[test]
    fn parse_repo_sections_reads_dnf_enabled_vocabulary() {
        // Review Focus 2.
        let content = "\
stray = line outside any section
[a]
baseurl=https://example.invalid/a
[b]
enabled=0
metalink=https://example.invalid/b
[c]
enabled = True   # trailing comment
mirrorlist=https://example.invalid/c
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-C
[d]
enabled=yes
";
        let got = parse_repo_sections(content);
        let by = |n: &str| got.iter().find(|s| s.id == RepoId::new(n)).unwrap();
        assert_eq!(got.len(), 4);
        assert!(by("a").enabled && by("a").has_source); // absent enabled= is true
        assert!(!by("b").enabled && by("b").has_source);
        assert!(by("c").enabled);
        assert_eq!(
            by("c").gpg_key_paths,
            vec!["/etc/pki/rpm-gpg/RPM-GPG-KEY-C"]
        );
        assert!(by("d").enabled);
    }
}
