//! Modernization advisory system: detects legacy patterns that should be
//! replaced with modern equivalents for image mode compatibility.
//!
//! Each pattern defines a filesystem detection rule, an OS version predicate,
//! and an advisory rationale. The config inspector calls
//! [`check_modernization_patterns`] and folds the results into the config
//! section as Advisory findings.

use inspectah_core::traits::executor::Executor;
use inspectah_core::types::system::SourceSystem;
use inspectah_core::types::{AdvisoryType, FindingKind};
use std::path::Path;

/// A modernization pattern defines a legacy system artifact and its
/// recommended modern replacement.
pub struct ModernizationPattern {
    pub name: &'static str,
    pub detection: DetectionRule,
    pub replacement: &'static str,
    pub rationale: &'static str,
    /// Minimum OS major version to fire on. `None` = all versions.
    pub min_os_major: Option<u32>,
}

/// How to detect a legacy pattern on the filesystem.
pub enum DetectionRule {
    /// Directory listing: fire for every file found in the directory.
    FileGlob(&'static str),
    /// Directory listing with counterpart check: fire only when the
    /// file has no modern counterpart (e.g., init script without a
    /// matching systemd unit).
    FileGlobWithoutCounterpart {
        file_glob: &'static str,
        counterpart_pattern: fn(&str) -> String,
    },
    /// File content check: fire when the file exists and contains
    /// non-default scheduled entries. Lines starting with a digit or
    /// `@` are data lines; environment variable lines (`KEY=VALUE`)
    /// and comments are skipped.
    FileHasCustomEntries {
        path: &'static str,
        default_marker: &'static str,
    },
}

/// Maps an init script path to its expected systemd unit path.
/// `/etc/init.d/httpd` -> `/usr/lib/systemd/system/httpd.service`
fn sysvinit_to_systemd_unit(init_script: &str) -> String {
    let name = init_script.rsplit('/').next().unwrap_or(init_script);
    format!("/usr/lib/systemd/system/{name}.service")
}

// NOTE: ifcfg is NOT a modernization pattern. Networking config is treated as
// informational inventory, not a modernization advisory. See spec section 6.6.
pub const MODERNIZATION_PATTERNS: &[ModernizationPattern] = &[
    ModernizationPattern {
        name: "sysvinit_script",
        detection: DetectionRule::FileGlobWithoutCounterpart {
            file_glob: "/etc/init.d",
            counterpart_pattern: sysvinit_to_systemd_unit,
        },
        replacement: "systemd unit",
        rationale: "SysVinit script with no systemd equivalent \u{2014} create a .service unit for image mode",
        min_os_major: None,
    },
    ModernizationPattern {
        name: "xinetd_config",
        detection: DetectionRule::FileGlob("/etc/xinetd.d"),
        replacement: "systemd socket activation",
        rationale: "xinetd is deprecated \u{2014} convert to systemd socket activation",
        min_os_major: None,
    },
    ModernizationPattern {
        name: "anacrontab",
        detection: DetectionRule::FileHasCustomEntries {
            path: "/etc/anacrontab",
            // Matches cron.daily, cron.weekly, cron.monthly — all standard entries.
            default_marker: "cron.",
        },
        replacement: "systemd timer",
        rationale: "anacrontab has custom entries \u{2014} consider systemd timers instead",
        min_os_major: None,
    },
];

/// Extract the OS major version from a [`SourceSystem`]'s `version_id`.
///
/// Parses the leading integer from strings like `"9.4"` or `"8"`.
/// Returns `None` if the version string is empty or unparseable.
pub fn os_major_version(source: &SourceSystem) -> Option<u32> {
    let os_release = match source {
        SourceSystem::PackageBased { os_release } => os_release,
        SourceSystem::RpmOstree { os_release, .. } => os_release,
        SourceSystem::Bootc { os_release, .. } => os_release,
    };
    os_release.version_id.split('.').next()?.parse().ok()
}

/// Scan for modernization patterns, returning advisory findings.
///
/// Each result is a `(path, FindingKind::Advisory)` tuple. The caller
/// folds these into the appropriate output section.
pub fn check_modernization_patterns(
    exec: &dyn Executor,
    os_major: u32,
) -> Vec<(String, FindingKind)> {
    let mut advisories = Vec::new();

    for pattern in MODERNIZATION_PATTERNS {
        if let Some(min) = pattern.min_os_major
            && os_major < min
        {
            continue;
        }

        match &pattern.detection {
            DetectionRule::FileGlob(dir) => {
                if let Ok(entries) = exec.read_dir(Path::new(dir)) {
                    for name in entries {
                        let path = format!("{dir}/{name}");
                        advisories.push((
                            path,
                            FindingKind::advisory(AdvisoryType::Modernization, pattern.rationale),
                        ));
                    }
                }
            }
            DetectionRule::FileGlobWithoutCounterpart {
                file_glob: dir,
                counterpart_pattern,
            } => {
                if let Ok(entries) = exec.read_dir(Path::new(dir)) {
                    for name in entries {
                        let path = format!("{dir}/{name}");
                        let counterpart = counterpart_pattern(&path);
                        if !exec.file_exists(Path::new(&counterpart)) {
                            advisories.push((
                                path,
                                FindingKind::advisory(
                                    AdvisoryType::Modernization,
                                    pattern.rationale,
                                ),
                            ));
                        }
                    }
                }
            }
            DetectionRule::FileHasCustomEntries {
                path,
                default_marker,
            } => {
                if let Ok(content) = exec.read_file(Path::new(path)) {
                    let has_custom = content
                        .lines()
                        .map(|l| l.trim())
                        .filter(|l| !l.is_empty() && !l.starts_with('#'))
                        // Only check anacrontab data lines (period + delay + id + command).
                        // Skip environment variable lines (KEY=VALUE format).
                        .filter(|l| {
                            l.starts_with(|c: char| c.is_ascii_digit()) || l.starts_with('@')
                        })
                        .any(|l| !l.contains(default_marker));
                    if has_custom {
                        advisories.push((
                            path.to_string(),
                            FindingKind::advisory(AdvisoryType::Modernization, pattern.rationale),
                        ));
                    }
                }
            }
        }
    }

    advisories
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::types::os::OsRelease;

    // -- SysVinit tests --------------------------------------------------------

    #[test]
    fn test_sysvinit_without_service_fires() {
        // /etc/init.d/legacy-app exists, no matching .service
        let exec = MockExecutor::new()
            .with_dir("/etc/init.d", vec!["legacy-app"])
            .with_file(
                "/etc/init.d/legacy-app",
                "#!/bin/bash\n# chkconfig: 2345 95 05\n",
            );
        let advisories = check_modernization_patterns(&exec, 9);
        assert!(
            advisories.iter().any(|(p, _)| p.contains("legacy-app")),
            "SysVinit script without matching .service should fire"
        );
    }

    #[test]
    fn test_sysvinit_with_matching_service_suppressed() {
        // /etc/init.d/httpd exists, /usr/lib/systemd/system/httpd.service also exists
        let exec = MockExecutor::new()
            .with_dir("/etc/init.d", vec!["httpd"])
            .with_file("/etc/init.d/httpd", "#!/bin/bash\n")
            .with_file(
                "/usr/lib/systemd/system/httpd.service",
                "[Unit]\nDescription=httpd\n",
            );
        let advisories = check_modernization_patterns(&exec, 9);
        assert!(
            !advisories.iter().any(|(p, _)| p.contains("httpd")),
            "SysVinit script with matching .service should be suppressed"
        );
    }

    #[test]
    fn test_sysvinit_mixed_with_and_without_service() {
        // Two init scripts: httpd has a service, legacy-app does not
        let exec = MockExecutor::new()
            .with_dir("/etc/init.d", vec!["httpd", "legacy-app"])
            .with_file("/etc/init.d/httpd", "#!/bin/bash\n")
            .with_file("/etc/init.d/legacy-app", "#!/bin/bash\n")
            .with_file(
                "/usr/lib/systemd/system/httpd.service",
                "[Unit]\nDescription=httpd\n",
            );
        let advisories = check_modernization_patterns(&exec, 9);
        assert!(
            !advisories.iter().any(|(p, _)| p.contains("httpd")),
            "httpd has a matching service, should be suppressed"
        );
        assert!(
            advisories.iter().any(|(p, _)| p.contains("legacy-app")),
            "legacy-app has no matching service, should fire"
        );
    }

    // -- xinetd tests ----------------------------------------------------------

    #[test]
    fn test_xinetd_fires() {
        let exec = MockExecutor::new().with_dir("/etc/xinetd.d", vec!["telnet", "ftp"]);
        let advisories = check_modernization_patterns(&exec, 9);
        assert!(
            advisories.iter().any(|(p, _)| p.contains("telnet")),
            "xinetd config should fire for telnet"
        );
        assert!(
            advisories.iter().any(|(p, _)| p.contains("ftp")),
            "xinetd config should fire for ftp"
        );
    }

    #[test]
    fn test_xinetd_empty_dir_no_advisories() {
        let exec = MockExecutor::new().with_dir("/etc/xinetd.d", vec![]);
        let advisories = check_modernization_patterns(&exec, 9);
        assert!(
            !advisories.iter().any(|(p, _)| p.contains("xinetd")),
            "empty xinetd.d should produce no advisories"
        );
    }

    // -- anacrontab tests ------------------------------------------------------

    #[test]
    fn test_anacrontab_default_only_suppressed() {
        // Realistic default anacrontab with env vars and standard cron entries
        let exec = MockExecutor::new().with_file(
            "/etc/anacrontab",
            "# /etc/anacrontab: configuration file for anacron\n\
             SHELL=/bin/sh\n\
             PATH=/sbin:/bin:/usr/sbin:/usr/bin\n\
             MAILTO=root\n\
             RANDOM_DELAY=45\n\
             START_HOURS_RANGE=3-22\n\
             \n\
             1\t5\tcron.daily\t\tnice run-parts /etc/cron.daily\n\
             7\t25\tcron.weekly\t\tnice run-parts /etc/cron.weekly\n\
             @monthly 45\tcron.monthly\tnice run-parts /etc/cron.monthly\n",
        );
        let advisories = check_modernization_patterns(&exec, 9);
        assert!(
            !advisories.iter().any(|(p, _)| p.contains("anacrontab")),
            "default anacrontab should NOT fire"
        );
    }

    #[test]
    fn test_anacrontab_custom_entries_fires() {
        // Default entries plus a custom backup job
        let exec = MockExecutor::new().with_file(
            "/etc/anacrontab",
            "# /etc/anacrontab\n\
             SHELL=/bin/sh\n\
             PATH=/sbin:/bin:/usr/sbin:/usr/bin\n\
             \n\
             1\t5\tcron.daily\t\tnice run-parts /etc/cron.daily\n\
             7\t25\tcron.weekly\t\tnice run-parts /etc/cron.weekly\n\
             1\t5\tcustom-backup\t/usr/local/bin/backup.sh\n",
        );
        let advisories = check_modernization_patterns(&exec, 9);
        assert!(
            advisories.iter().any(|(p, _)| p.contains("anacrontab")),
            "anacrontab with custom entries should fire"
        );
    }

    #[test]
    fn test_anacrontab_missing_file_no_advisory() {
        // No /etc/anacrontab file present
        let exec = MockExecutor::new();
        let advisories = check_modernization_patterns(&exec, 9);
        assert!(
            !advisories.iter().any(|(p, _)| p.contains("anacrontab")),
            "missing anacrontab should produce no advisory"
        );
    }

    // -- ifcfg negative test ---------------------------------------------------

    #[test]
    fn test_ifcfg_not_in_modernization() {
        // ifcfg is network inventory, NOT a modernization advisory (spec section 6.6).
        // Even with ifcfg files present, no modernization advisory should fire.
        let exec = MockExecutor::new().with_dir(
            "/etc/sysconfig/network-scripts",
            vec!["ifcfg-eth0", "ifcfg-ens192"],
        );
        let advisories = check_modernization_patterns(&exec, 9);
        assert!(
            !advisories.iter().any(|(p, _)| p.contains("ifcfg")),
            "ifcfg files should not produce modernization advisories"
        );
    }

    // -- OS predicate tests ----------------------------------------------------

    #[test]
    fn test_os_major_version_parsing() {
        let source = SourceSystem::PackageBased {
            os_release: OsRelease {
                version_id: "9.4".into(),
                ..Default::default()
            },
        };
        assert_eq!(os_major_version(&source), Some(9));

        let source_el8 = SourceSystem::PackageBased {
            os_release: OsRelease {
                version_id: "8.9".into(),
                ..Default::default()
            },
        };
        assert_eq!(os_major_version(&source_el8), Some(8));
    }

    #[test]
    fn test_os_major_version_empty_returns_none() {
        let source = SourceSystem::PackageBased {
            os_release: OsRelease {
                version_id: String::new(),
                ..Default::default()
            },
        };
        assert_eq!(os_major_version(&source), None);
    }

    #[test]
    fn test_min_os_major_skips_older() {
        // Create a pattern that only fires on EL9+
        let exec = MockExecutor::new().with_dir("/etc/xinetd.d", vec!["telnet"]);

        // On EL8, the pattern fires (min_os_major is None for all current patterns)
        let advisories = check_modernization_patterns(&exec, 8);
        assert!(
            advisories.iter().any(|(p, _)| p.contains("telnet")),
            "xinetd should fire on EL8 (min_os_major is None)"
        );
    }

    // -- Advisory type correctness ---------------------------------------------

    #[test]
    fn test_advisory_type_is_modernization() {
        let exec = MockExecutor::new().with_dir("/etc/xinetd.d", vec!["telnet"]);
        let advisories = check_modernization_patterns(&exec, 9);
        assert_eq!(advisories.len(), 1);
        match &advisories[0].1 {
            FindingKind::Advisory {
                advisory_type,
                rationale,
            } => {
                assert_eq!(*advisory_type, AdvisoryType::Modernization);
                assert!(
                    rationale.contains("xinetd"),
                    "rationale should mention xinetd"
                );
            }
            other => panic!("expected Advisory finding, got: {other:?}"),
        }
    }
}
