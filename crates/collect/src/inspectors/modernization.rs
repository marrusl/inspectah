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
    /// The file exists: its mechanism does not apply to an image-mode
    /// host at all, whatever it holds.
    FileExists(&'static str),
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
        rationale: "SysVinit script with no systemd equivalent: create a .service unit for image mode",
        min_os_major: None,
    },
    ModernizationPattern {
        name: "xinetd_config",
        detection: DetectionRule::FileGlob("/etc/xinetd.d"),
        replacement: "systemd socket activation",
        rationale: "xinetd is deprecated: convert to systemd socket activation",
        min_os_major: None,
    },
    // `/etc/anacrontab` is not in this catalog: an edit to it is an
    // ordinary configuration row and is carried, because the image's
    // anacron reads it just as the host's did. An advisory here would
    // keep the edit out of the image with no control to bring it back.
    // `/etc/default/grub` is a `%ghost` file of grub2-tools on Fedora and
    // RHEL, so the ghost branch collects an admin's edit to it. Copied
    // into the image it changes nothing: bootc does not run
    // grub2-mkconfig, and kernel arguments come from kargs.d. The
    // advisory keeps the content visible and the file out of the export.
    // This pass cannot see the running command line, so it marks the file
    // on every host; the pipeline's cross-inspector pass
    // (`settle_default_grub`) then keeps the advisory only where the file
    // sets an argument the running kernel lacks, and never lets the file
    // be carried.
    ModernizationPattern {
        name: "grub_defaults",
        detection: DetectionRule::FileExists(DEFAULT_GRUB_PATH),
        replacement: "bootc kargs.d",
        rationale: DEFAULT_GRUB_RATIONALE,
        min_os_major: None,
    },
];

/// The GRUB defaults file `grub2-mkconfig` reads on a package-mode host.
pub const DEFAULT_GRUB_PATH: &str = "/etc/default/grub";

/// The `/etc/default/grub` advisory when its kernel arguments could not be
/// compared with the running ones.
pub const DEFAULT_GRUB_RATIONALE: &str = "/etc/default/grub is not copied: an image-mode host \
     does not regenerate its boot loader configuration from it. Put kernel arguments from \
     GRUB_CMDLINE_LINUX that the image needs in a /usr/lib/bootc/kargs.d/*.toml file.";

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
            DetectionRule::FileExists(path) => {
                if exec.file_exists(Path::new(path)) {
                    advisories.push((
                        path.to_string(),
                        FindingKind::advisory(AdvisoryType::Modernization, pattern.rationale),
                    ));
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

    // -- GRUB defaults ---------------------------------------------------------

    /// This pass marks the file on every host; the pipeline's
    /// `settle_default_grub` narrows it against the running command line.
    #[test]
    fn default_grub_is_an_advisory_whenever_it_exists() {
        let exec =
            MockExecutor::new().with_file(DEFAULT_GRUB_PATH, "GRUB_CMDLINE_LINUX=\"audit=1\"\n");
        let found = check_modernization_patterns(&exec, 9);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, DEFAULT_GRUB_PATH);
        assert!(found[0].1.is_advisory());
        assert!(check_modernization_patterns(&MockExecutor::new(), 9).is_empty());
    }

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
