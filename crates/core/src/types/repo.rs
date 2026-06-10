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

/// Classification tier for a repo section ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoTier {
    Distro,
    OfficialOptional,
    ThirdParty,
    None,
}

/// Classify a repo section ID into its tier.
///
/// Empty, missing, or `@commandline` section IDs return `None` --
/// "no repo identity" is distinct from "known third-party repo."
pub fn repo_tier(section_id: &str) -> RepoTier {
    if section_id.is_empty() {
        return RepoTier::None;
    }
    let lower = section_id.to_lowercase();
    let id = lower.as_str();
    if id == "@commandline" {
        return RepoTier::None;
    }
    if DISTRO_REPOS.contains(&id) {
        RepoTier::Distro
    } else if OFFICIAL_OPTIONAL_REPOS.contains(&id) {
        RepoTier::OfficialOptional
    } else {
        RepoTier::ThirdParty
    }
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
    fn test_repo_tier_official_optional() {
        assert_eq!(repo_tier("crb"), RepoTier::OfficialOptional);
        assert_eq!(repo_tier("CRB"), RepoTier::OfficialOptional);
        assert_eq!(repo_tier("rhel-extensions"), RepoTier::OfficialOptional);
    }

    #[test]
    fn test_repo_tier_third_party() {
        assert_eq!(repo_tier("epel"), RepoTier::ThirdParty);
        assert_eq!(repo_tier("copr:mytools"), RepoTier::ThirdParty);
    }

    #[test]
    fn test_repo_tier_none() {
        assert_eq!(repo_tier(""), RepoTier::None);
        assert_eq!(repo_tier("@commandline"), RepoTier::None);
        assert_eq!(repo_tier("@CommandLine"), RepoTier::None);
    }
}
