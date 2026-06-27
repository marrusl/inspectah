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
}
