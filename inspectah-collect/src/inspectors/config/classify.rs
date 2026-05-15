use inspectah_core::types::config::ConfigCategory;

/// A classification rule mapping path prefixes to a config category.
struct CategoryRule {
    category: ConfigCategory,
    prefixes: &'static [&'static str],
}

/// Ordered category rules — checked sequentially, first match wins.
/// Each prefix is tested as exact match first, then as a prefix match
/// (only if the prefix ends in '/' or '.').
const CATEGORY_RULES: &[CategoryRule] = &[
    CategoryRule {
        category: ConfigCategory::Tmpfiles,
        prefixes: &["/etc/tmpfiles.d/"],
    },
    CategoryRule {
        category: ConfigCategory::Environment,
        prefixes: &["/etc/environment", "/etc/profile.d/"],
    },
    CategoryRule {
        category: ConfigCategory::Audit,
        prefixes: &["/etc/audit/rules.d/"],
    },
    CategoryRule {
        category: ConfigCategory::LibraryPath,
        prefixes: &["/etc/ld.so.conf.d/"],
    },
    CategoryRule {
        category: ConfigCategory::Journal,
        prefixes: &["/etc/systemd/journald.conf.d/"],
    },
    CategoryRule {
        category: ConfigCategory::Logrotate,
        prefixes: &["/etc/logrotate.d/"],
    },
    CategoryRule {
        category: ConfigCategory::Automount,
        prefixes: &["/etc/auto.master", "/etc/auto."],
    },
    CategoryRule {
        category: ConfigCategory::Sysctl,
        prefixes: &["/etc/sysctl.d/", "/etc/sysctl.conf"],
    },
    CategoryRule {
        category: ConfigCategory::CryptoPolicy,
        prefixes: &["/etc/crypto-policies/"],
    },
    CategoryRule {
        category: ConfigCategory::Identity,
        prefixes: &[
            "/etc/nsswitch.conf",
            "/etc/sssd/",
            "/etc/krb5.conf",
            "/etc/krb5.conf.d/",
            "/etc/ipa/",
        ],
    },
    CategoryRule {
        category: ConfigCategory::Limits,
        prefixes: &["/etc/security/limits."],
    },
];

/// Assigns a semantic category to a config file path.
///
/// Matching logic: for each rule, check each prefix.
/// - If `path == prefix` → exact match, return category.
/// - If prefix ends with '/' or '.' and `path.starts_with(prefix)` → prefix match.
/// - Otherwise → continue to next rule.
///
/// Default: `Other`.
pub fn classify_config_path(path: &str) -> ConfigCategory {
    for rule in CATEGORY_RULES {
        for prefix in rule.prefixes {
            if path == *prefix {
                return rule.category.clone();
            }
            if (prefix.ends_with('/') || prefix.ends_with('.'))
                && path.starts_with(prefix)
            {
                return rule.category.clone();
            }
        }
    }
    ConfigCategory::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Test 1: test_classify_tmpfiles ----

    #[test]
    fn test_classify_tmpfiles() {
        assert_eq!(
            classify_config_path("/etc/tmpfiles.d/foo.conf"),
            ConfigCategory::Tmpfiles
        );
    }

    // ---- Test 2: test_classify_sysctl ----

    #[test]
    fn test_classify_sysctl() {
        assert_eq!(
            classify_config_path("/etc/sysctl.d/99-custom.conf"),
            ConfigCategory::Sysctl
        );
    }

    // ---- Test 3: test_classify_identity ----

    #[test]
    fn test_classify_identity() {
        assert_eq!(
            classify_config_path("/etc/sssd/sssd.conf"),
            ConfigCategory::Identity
        );
    }

    // ---- Test 4: test_classify_other ----

    #[test]
    fn test_classify_other() {
        assert_eq!(
            classify_config_path("/etc/httpd/conf/httpd.conf"),
            ConfigCategory::Other
        );
    }

    // ---- Test 5: test_classify_exact_match ----

    #[test]
    fn test_classify_exact_match() {
        // /etc/sysctl.conf is an exact match, not a prefix match
        assert_eq!(
            classify_config_path("/etc/sysctl.conf"),
            ConfigCategory::Sysctl
        );
    }

    // ---- Test 6: test_classify_environment ----

    #[test]
    fn test_classify_environment() {
        // Exact match
        assert_eq!(
            classify_config_path("/etc/environment"),
            ConfigCategory::Environment
        );
        // Prefix match
        assert_eq!(
            classify_config_path("/etc/profile.d/foo.sh"),
            ConfigCategory::Environment
        );
    }
}
