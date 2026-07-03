/// Allowlist entry for cross-tree symlinks that are known-safe system patterns.
pub struct AllowlistEntry {
    pub source_prefix: &'static str,
    pub target_prefix: &'static str,
}

/// Allowlist of cross-tree symlinks that are standard RHEL patterns and safe
/// to preserve in image mode. These are system-managed symlinks that work
/// correctly across /etc → /usr and /etc → /var boundaries.
pub const CROSS_TREE_SYMLINK_ALLOWLIST: &[AllowlistEntry] = &[
    // Timezone config: /etc/localtime → /usr/share/zoneinfo/
    AllowlistEntry {
        source_prefix: "/etc/localtime",
        target_prefix: "/usr/share/zoneinfo/",
    },
    // Alternatives system: /etc/alternatives/ → /usr/
    AllowlistEntry {
        source_prefix: "/etc/alternatives/",
        target_prefix: "/usr/",
    },
    // CA bundle: /etc/ssl/certs/ca-bundle.crt → /etc/pki/
    AllowlistEntry {
        source_prefix: "/etc/ssl/certs/ca-bundle.crt",
        target_prefix: "/etc/pki/",
    },
    // TLS cert: /etc/pki/tls/cert.pem → /etc/pki/
    AllowlistEntry {
        source_prefix: "/etc/pki/tls/cert.pem",
        target_prefix: "/etc/pki/",
    },
    // Crypto policies backends: /etc/crypto-policies/back-ends/ → /usr/share/crypto-policies/
    AllowlistEntry {
        source_prefix: "/etc/crypto-policies/back-ends/",
        target_prefix: "/usr/share/crypto-policies/",
    },
    // resolv.conf: /etc/resolv.conf → /run/
    AllowlistEntry {
        source_prefix: "/etc/resolv.conf",
        target_prefix: "/run/",
    },
];

/// Checks if a symlink is allowlisted based on source path and fully resolved target.
///
/// Returns true if both the source and target match an allowlist entry,
/// indicating this is a known-safe system pattern.
pub fn is_allowlisted(source: &str, resolved_target: &str) -> bool {
    CROSS_TREE_SYMLINK_ALLOWLIST.iter().any(|entry| {
        source.starts_with(entry.source_prefix) && resolved_target.starts_with(entry.target_prefix)
    })
}

/// Checks if a symlink crosses a tree boundary that matters for image mode.
///
/// Returns a rationale string if the symlink crosses a problematic boundary,
/// or None if the symlink is within the same tree.
///
/// Problematic boundaries:
/// - /etc → /var: Config becomes stateful via /var persistence, not subject to /etc 3-way merge
/// - /etc → /usr: Target is in the immutable /usr layer
/// - /opt → /usr: Target is in the immutable /usr layer
pub fn crosses_tree_boundary(source: &str, target: &str) -> Option<&'static str> {
    if source.starts_with("/etc/") && target.starts_with("/var/") {
        Some(
            "Symlink crosses /etc → /var: config is stateful via /var persistence, not subject to /etc 3-way merge",
        )
    } else if source.starts_with("/etc/") && target.starts_with("/usr/") {
        Some("Symlink crosses /etc → /usr: target is in the immutable /usr layer")
    } else if source.starts_with("/opt/") && target.starts_with("/usr/") {
        Some("Symlink crosses /opt → /usr: target is in the immutable /usr layer")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowlisted_localtime_suppressed() {
        assert!(is_allowlisted("/etc/localtime", "/usr/share/zoneinfo/UTC"));
        assert!(is_allowlisted(
            "/etc/localtime",
            "/usr/share/zoneinfo/America/New_York"
        ));
    }

    #[test]
    fn test_alternatives_to_usr_suppressed() {
        assert!(is_allowlisted(
            "/etc/alternatives/python",
            "/usr/bin/python3.11"
        ));
        assert!(is_allowlisted(
            "/etc/alternatives/java",
            "/usr/lib/jvm/java-11/bin/java"
        ));
    }

    #[test]
    fn test_alternatives_retargeted_to_var_not_suppressed() {
        // /etc/alternatives/foo → /var/lib/custom/foo is NOT /usr, so NOT allowlisted
        assert!(!is_allowlisted(
            "/etc/alternatives/foo",
            "/var/lib/custom/foo"
        ));
    }

    #[test]
    fn test_app_symlink_fires() {
        let rationale = crosses_tree_boundary("/etc/mydb/config", "/var/lib/mydb/config");
        assert!(rationale.is_some());
        assert!(rationale.unwrap().contains("/etc → /var"));
    }

    #[test]
    fn test_etc_to_usr_fires() {
        let rationale = crosses_tree_boundary("/etc/custom/link", "/usr/share/custom/target");
        assert!(rationale.is_some());
        assert!(rationale.unwrap().contains("/etc → /usr"));
    }

    #[test]
    fn test_opt_to_usr_fires() {
        let rationale = crosses_tree_boundary("/opt/app/config", "/usr/lib/app/defaults");
        assert!(rationale.is_some());
        assert!(rationale.unwrap().contains("/opt → /usr"));
    }

    #[test]
    fn test_within_tree_ok() {
        // /etc → /etc is fine
        assert!(crosses_tree_boundary("/etc/foo", "/etc/bar").is_none());
        // /var → /var is fine
        assert!(crosses_tree_boundary("/var/foo", "/var/bar").is_none());
        // /usr → /usr is fine
        assert!(crosses_tree_boundary("/usr/foo", "/usr/bar").is_none());
    }

    #[test]
    fn test_ca_bundle_allowlisted() {
        assert!(is_allowlisted(
            "/etc/ssl/certs/ca-bundle.crt",
            "/etc/pki/tls/certs/ca-bundle.crt"
        ));
    }

    #[test]
    fn test_resolv_conf_allowlisted() {
        assert!(is_allowlisted(
            "/etc/resolv.conf",
            "/run/systemd/resolve/stub-resolv.conf"
        ));
    }

    #[test]
    fn test_crypto_policies_allowlisted() {
        assert!(is_allowlisted(
            "/etc/crypto-policies/back-ends/openssh.config",
            "/usr/share/crypto-policies/DEFAULT/openssh.txt"
        ));
    }

    #[test]
    fn test_source_mismatch_not_allowlisted() {
        // Source doesn't match any allowlist prefix
        assert!(!is_allowlisted(
            "/etc/custom/link",
            "/usr/share/zoneinfo/UTC"
        ));
    }

    #[test]
    fn test_target_mismatch_not_allowlisted() {
        // Source matches but target doesn't
        assert!(!is_allowlisted("/etc/localtime", "/var/lib/timezone/UTC"));
    }
}
