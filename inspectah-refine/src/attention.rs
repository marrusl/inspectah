use crate::types::{AttentionLevel, AttentionReason, AttentionTag, RefinedConfig, RefinedPackage};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::ConfigFileKind;
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::rpm::{
    PackageEntry, PackageState, VersionChange, VersionChangeDirection,
};

const SENSITIVE_PATHS: &[&str] = &[
    "/etc/shadow",
    "/etc/gshadow",
    "/etc/ssh/",
    "/etc/pki/",
    "/etc/ssl/",
    "/etc/security/",
];

fn is_sensitive_path(path: &str) -> bool {
    SENSITIVE_PATHS.iter().any(|s| path.starts_with(s))
}

/// Well-known OS-default path prefixes in sensitive directories.
///
/// Files at these paths are delivered by standard RPM packages (ca-certificates,
/// openssh-server, pam, openssl, fwupd, nss-tools, etc.) as regular files, not
/// `%config`. The config inspector classifies them as `Unowned` because they
/// aren't tracked by `rpm -Vc`, but on a stock system they are package defaults.
///
/// Suppresses the SensitivePath promotion (Informational -> NeedsReview) so
/// these stock files don't flood the NeedsReview tier on unmodified systems.
const OS_DEFAULT_SENSITIVE_PREFIXES: &[&str] = &[
    // ca-certificates, p11-kit-trust
    "/etc/pki/ca-trust/",
    // centos-stream-release, redhat-release
    "/etc/pki/rpm-gpg/",
    // fwupd
    "/etc/pki/fwupd/",
    "/etc/pki/fwupd-metadata/",
    // nss-softokn, nss-tools
    "/etc/pki/nssdb/",
    // openssl
    "/etc/ssl/",
    // pam
    "/etc/security/",
    // openssh-server, openssh-clients
    "/etc/ssh/ssh_config.d/",
    "/etc/ssh/sshd_config.d/",
];

/// Exact paths that are OS defaults in sensitive directories.
const OS_DEFAULT_SENSITIVE_EXACT: &[&str] = &["/etc/ssh/moduli", "/etc/ssh/ssh_config"];

/// Returns true when the path is a well-known OS default inside a sensitive
/// directory. These files should NOT be promoted from Informational to
/// NeedsReview by the sensitive-path overlay.
fn is_os_default_sensitive(path: &str) -> bool {
    OS_DEFAULT_SENSITIVE_PREFIXES
        .iter()
        .any(|p| path.starts_with(p))
        || OS_DEFAULT_SENSITIVE_EXACT.contains(&path)
}

pub fn compute_package_attention(snap: &InspectionSnapshot) -> Vec<RefinedPackage> {
    let rpm = match &snap.rpm {
        Some(r) => r,
        None => return Vec::new(),
    };

    let baseline_names: Option<Vec<String>> = snap
        .baseline
        .as_ref()
        .map(|b| b.packages.keys().cloned().collect());
    let baseline: Option<&[String]> = baseline_names.as_deref();

    // Build baseline_suppressed set for fast lookup
    let suppressed_set: std::collections::HashSet<&str> = rpm
        .baseline_suppressed
        .as_ref()
        .map(|v| v.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default();

    rpm.packages_added
        .iter()
        .map(|entry| {
            let canonical_id = format!("{}.{}", entry.name, entry.arch);

            if suppressed_set.contains(canonical_id.as_str()) {
                return RefinedPackage {
                    entry: entry.clone(),
                    attention: vec![AttentionTag {
                        level: AttentionLevel::Routine,
                        reason: AttentionReason::PackageBaselineMatch,
                        detail: None,
                    }],
                };
            }

            let tag = classify_package(entry, baseline, &rpm.version_changes);
            let mut tags = vec![tag];

            if is_sensitive_path(&entry.name) {
                let primary_level = tags[0].level;
                let should_promote = match primary_level {
                    AttentionLevel::Informational => true,
                    AttentionLevel::Routine => baseline.is_none(),
                    AttentionLevel::NeedsReview => false,
                };
                if should_promote {
                    tags.push(AttentionTag {
                        level: AttentionLevel::NeedsReview,
                        reason: AttentionReason::SensitivePath,
                        detail: Some(entry.name.clone()),
                    });
                }
            }

            RefinedPackage {
                entry: entry.clone(),
                attention: tags,
            }
        })
        .collect()
}

fn classify_package(
    entry: &PackageEntry,
    baseline: Option<&[String]>,
    version_changes: &[VersionChange],
) -> AttentionTag {
    // LocalInstall and NoRepo are always Tier 3, regardless of baseline or repo.
    match entry.state {
        PackageState::LocalInstall => {
            return AttentionTag {
                level: AttentionLevel::NeedsReview,
                reason: AttentionReason::PackageLocalInstall,
                detail: None,
            };
        }
        PackageState::NoRepo => {
            return AttentionTag {
                level: AttentionLevel::NeedsReview,
                reason: AttentionReason::PackageNoRepoSource,
                detail: None,
            };
        }
        _ => {}
    }

    // Empty source_repo means unknown provenance — always Tier 3.
    if entry.source_repo.is_empty() {
        return AttentionTag {
            level: AttentionLevel::NeedsReview,
            reason: AttentionReason::PackageNoRepoSource,
            detail: None,
        };
    }

    // Modified packages: check version change direction.
    // Upgrades are normal maintenance (Routine). Downgrades need review.
    if entry.state == PackageState::Modified {
        return match baseline {
            Some(_) => {
                let is_downgrade = version_changes.iter().any(|vc| {
                    vc.name == entry.name
                        && vc.arch == entry.arch
                        && vc.direction == VersionChangeDirection::Downgrade
                });
                if is_downgrade {
                    AttentionTag {
                        level: AttentionLevel::NeedsReview,
                        reason: AttentionReason::PackageVersionChanged,
                        detail: Some("Downgrade".to_string()),
                    }
                } else {
                    AttentionTag {
                        level: AttentionLevel::Routine,
                        reason: AttentionReason::PackageVersionChanged,
                        detail: Some("Upgrade".to_string()),
                    }
                }
            }
            None => AttentionTag {
                level: AttentionLevel::Informational,
                reason: AttentionReason::PackageProvenanceUnavailable,
                detail: None,
            },
        };
    }

    // Classify based on baseline availability and membership (Added/BaseImageOnly only).
    match baseline {
        Some(names)
            if names.iter().any(|n| {
                let entry_key = format!("{}.{}", entry.name, entry.arch);
                n == &entry_key
            }) =>
        {
            // In baseline with known repo — expected package, Tier 1.
            AttentionTag {
                level: AttentionLevel::Routine,
                reason: AttentionReason::PackageBaselineMatch,
                detail: None,
            }
        }
        Some(_) => {
            // Not in baseline but has a known repo — user-added, Tier 1.
            AttentionTag {
                level: AttentionLevel::Routine,
                reason: AttentionReason::PackageUserAdded,
                detail: None,
            }
        }
        None => {
            // No baseline available — can't determine provenance, Tier 2.
            AttentionTag {
                level: AttentionLevel::Informational,
                reason: AttentionReason::PackageProvenanceUnavailable,
                detail: None,
            }
        }
    }
}

pub fn compute_config_attention(snap: &InspectionSnapshot) -> Vec<RefinedConfig> {
    let config = match &snap.config {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut configs: Vec<RefinedConfig> = config
        .files
        .iter()
        .map(|entry| {
            let tag = match entry.kind {
                ConfigFileKind::RpmOwnedDefault => AttentionTag {
                    level: AttentionLevel::Routine,
                    reason: AttentionReason::ConfigDefault,
                    detail: None,
                },
                ConfigFileKind::BaselineMatch => AttentionTag {
                    level: AttentionLevel::Routine,
                    reason: AttentionReason::ConfigBaselineMatch,
                    detail: None,
                },
                ConfigFileKind::Unowned => AttentionTag {
                    level: AttentionLevel::Informational,
                    reason: AttentionReason::ConfigUnowned,
                    detail: None,
                },
                ConfigFileKind::RpmOwnedModified => AttentionTag {
                    level: AttentionLevel::NeedsReview,
                    reason: AttentionReason::ConfigModified,
                    detail: None,
                },
                ConfigFileKind::Orphaned => AttentionTag {
                    level: AttentionLevel::Informational,
                    reason: AttentionReason::ConfigOrphaned,
                    detail: None,
                },
            };

            let mut tags = vec![tag];
            // Sensitive path overlay: promote Tier 2 -> Tier 3.
            // Tier 1 is NOT promoted (base image ships these files).
            // OS-default files in sensitive directories are also NOT promoted —
            // they are stock package contents, not meaningful user customizations.
            if is_sensitive_path(&entry.path)
                && tags[0].level == AttentionLevel::Informational
                && !is_os_default_sensitive(&entry.path)
            {
                tags.push(AttentionTag {
                    level: AttentionLevel::NeedsReview,
                    reason: AttentionReason::SensitivePath,
                    detail: Some(entry.path.clone()),
                });
            }

            RefinedConfig {
                entry: entry.clone(),
                attention: tags,
            }
        })
        .collect();

    // Surface unresolved redaction hints as needs-review tags on matching
    // config files. Applies to PartiallyRedacted and SensitiveRetained.
    if let Some(
        RedactionState::PartiallyRedacted {
            ref unresolved_hints,
            ..
        }
        | RedactionState::SensitiveRetained {
            ref unresolved_hints,
            ..
        },
    ) = snap.redaction_state
    {
        for hint in unresolved_hints {
            if let Some(cfg) = configs.iter_mut().find(|c| c.entry.path == hint.path) {
                cfg.attention.push(AttentionTag {
                    level: AttentionLevel::NeedsReview,
                    reason: AttentionReason::Custom("unresolved redaction hint".into()),
                    detail: Some(hint.reason.clone()),
                });
            }
        }
    }

    configs
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
    use inspectah_core::types::rpm::{RpmSection, VersionChange, VersionChangeDirection};

    /// Helper: build a minimal PackageEntry with the given state and source_repo.
    fn pkg(name: &str, state: PackageState, source_repo: &str) -> PackageEntry {
        PackageEntry {
            name: name.to_string(),
            arch: "x86_64".to_string(),
            state,
            source_repo: source_repo.to_string(),
            ..Default::default()
        }
    }

    /// Helper: build a VersionChange with the given direction.
    fn vc(name: &str, direction: VersionChangeDirection) -> VersionChange {
        VersionChange {
            name: name.to_string(),
            arch: "x86_64".to_string(),
            direction,
            ..Default::default()
        }
    }

    /// Helper: build a VersionChange with a specific architecture.
    fn vc_arch(name: &str, arch: &str, direction: VersionChangeDirection) -> VersionChange {
        VersionChange {
            name: name.to_string(),
            arch: arch.to_string(),
            direction,
            ..Default::default()
        }
    }

    /// Helper: build a PackageEntry with a specific architecture.
    fn pkg_arch(name: &str, arch: &str, state: PackageState, source_repo: &str) -> PackageEntry {
        PackageEntry {
            name: name.to_string(),
            arch: arch.to_string(),
            state,
            source_repo: source_repo.to_string(),
            ..Default::default()
        }
    }

    /// Helper: build a snapshot with baseline via snap.baseline (Phase 6) and packages_added.
    fn snap_with_baseline(
        baseline_names: Option<Vec<String>>,
        packages: Vec<PackageEntry>,
    ) -> InspectionSnapshot {
        snap_with_baseline_and_vc(baseline_names, packages, vec![])
    }

    /// Helper: build a snapshot with baseline, packages, and version changes.
    fn snap_with_baseline_and_vc(
        baseline_names: Option<Vec<String>>,
        packages: Vec<PackageEntry>,
        version_changes: Vec<VersionChange>,
    ) -> InspectionSnapshot {
        let baseline = baseline_names.map(|names| {
            let pkgs = names
                .into_iter()
                .map(|n| {
                    let key = format!("{}.x86_64", n);
                    let entry = BaselinePackageEntry {
                        name: n,
                        epoch: Some("0".to_string()),
                        version: "1.0".to_string(),
                        release: "1.el9".to_string(),
                        arch: "x86_64".to_string(),
                    };
                    (key, entry)
                })
                .collect();
            BaselineData {
                image_digest: "sha256:test".to_string(),
                packages: pkgs,
                extracted_at: "2026-01-01T00:00:00Z".to_string(),
            }
        });
        InspectionSnapshot {
            schema_version: 14,
            rpm: Some(RpmSection {
                packages_added: packages,
                version_changes,
                ..Default::default()
            }),
            baseline,
            ..Default::default()
        }
    }

    // -----------------------------------------------------------------------
    // Verified mode: baseline present
    // -----------------------------------------------------------------------

    #[test]
    fn verified_added_in_baseline_is_routine_baseline_match() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into()]),
            vec![pkg("glibc", PackageState::Added, "rhel-9-baseos")],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::Routine);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageBaselineMatch
        );
    }

    #[test]
    fn verified_added_not_in_baseline_recognized_repo_is_routine_user_added() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into()]),
            vec![pkg("httpd", PackageState::Added, "rhel-9-appstream")],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::Routine);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageUserAdded
        );
    }

    #[test]
    fn verified_added_no_repo_is_needs_review() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into()]),
            vec![pkg("mystery", PackageState::Added, "")],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageNoRepoSource
        );
    }

    #[test]
    fn verified_modified_upgrade_is_routine() {
        // Upgrades are normal maintenance — Routine, not NeedsReview.
        let snap = snap_with_baseline_and_vc(
            Some(vec!["kernel".into()]),
            vec![pkg("kernel", PackageState::Modified, "rhel-9-baseos")],
            vec![vc("kernel", VersionChangeDirection::Upgrade)],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::Routine);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageVersionChanged
        );
    }

    #[test]
    fn verified_modified_downgrade_is_needs_review() {
        // Downgrades are unusual — NeedsReview.
        let snap = snap_with_baseline_and_vc(
            Some(vec!["kernel".into()]),
            vec![pkg("kernel", PackageState::Modified, "rhel-9-baseos")],
            vec![vc("kernel", VersionChangeDirection::Downgrade)],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageVersionChanged
        );
    }

    #[test]
    fn verified_modified_no_version_change_entry_defaults_to_routine() {
        // Modified with no matching VersionChange entry (no downgrade found) — Routine.
        let snap = snap_with_baseline(
            Some(vec!["kernel".into()]),
            vec![pkg("kernel", PackageState::Modified, "rhel-9-baseos")],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::Routine);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageVersionChanged
        );
    }

    #[test]
    fn verified_modified_not_in_baseline_upgrade_is_routine() {
        let snap = snap_with_baseline_and_vc(
            Some(vec!["glibc".into()]),
            vec![pkg("kernel", PackageState::Modified, "rhel-9-baseos")],
            vec![vc("kernel", VersionChangeDirection::Upgrade)],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::Routine);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageVersionChanged
        );
    }

    #[test]
    fn verified_modified_no_repo_is_needs_review_no_repo_source() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into()]),
            vec![pkg("kernel", PackageState::Modified, "")],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageNoRepoSource
        );
    }

    #[test]
    fn verified_local_install_is_needs_review() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into()]),
            vec![pkg("custom-tool", PackageState::LocalInstall, "")],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageLocalInstall
        );
    }

    #[test]
    fn verified_no_repo_state_is_needs_review() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into()]),
            vec![pkg("orphan-pkg", PackageState::NoRepo, "some-repo")],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageNoRepoSource
        );
    }

    #[test]
    fn snap_baseline_field_drives_verified_mode() {
        // Verify that compute_package_attention reads snap.baseline (Phase 6),
        // not rpm.baseline_package_names (Go compat).
        use std::collections::HashMap;
        let mut pkgs = HashMap::new();
        pkgs.insert(
            "glibc.x86_64".to_string(),
            BaselinePackageEntry {
                name: "glibc".to_string(),
                epoch: Some("0".to_string()),
                version: "2.34".to_string(),
                release: "83.el9".to_string(),
                arch: "x86_64".to_string(),
            },
        );
        let snap = InspectionSnapshot {
            schema_version: 14,
            rpm: Some(RpmSection {
                packages_added: vec![pkg("glibc", PackageState::Added, "rhel-9-baseos")],
                // baseline_package_names NOT set — only snap.baseline
                ..Default::default()
            }),
            baseline: Some(BaselineData {
                image_digest: "sha256:abc".to_string(),
                packages: pkgs,
                extracted_at: "2026-01-01T00:00:00Z".to_string(),
            }),
            ..Default::default()
        };
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        // Should be verified mode (Routine/BaselineMatch), not degraded
        assert_eq!(result[0].attention[0].level, AttentionLevel::Routine);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageBaselineMatch
        );
    }

    // -----------------------------------------------------------------------
    // Degraded mode: no baseline
    // -----------------------------------------------------------------------

    #[test]
    fn degraded_added_is_informational_provenance_unavailable() {
        let snap = snap_with_baseline(
            None,
            vec![pkg("httpd", PackageState::Added, "rhel-9-appstream")],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::Informational);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageProvenanceUnavailable
        );
    }

    #[test]
    fn degraded_local_install_still_needs_review() {
        // LocalInstall is always Tier 3 regardless of baseline.
        let snap = snap_with_baseline(
            None,
            vec![pkg("custom-tool", PackageState::LocalInstall, "")],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageLocalInstall
        );
    }

    #[test]
    fn degraded_no_repo_state_still_needs_review() {
        let snap = snap_with_baseline(None, vec![pkg("orphan", PackageState::NoRepo, "some-repo")]);
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageNoRepoSource
        );
    }

    #[test]
    fn degraded_empty_source_repo_is_needs_review() {
        let snap = snap_with_baseline(None, vec![pkg("mystery", PackageState::Added, "")]);
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageNoRepoSource
        );
    }

    // -----------------------------------------------------------------------
    // Multiple packages in one snapshot
    // -----------------------------------------------------------------------

    #[test]
    fn verified_mixed_packages_classification() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into(), "bash".into()]),
            vec![
                pkg("glibc", PackageState::Added, "rhel-9-baseos"), // baseline match -> Routine
                pkg("httpd", PackageState::Added, "rhel-9-appstream"), // user-added -> Routine
                pkg("custom", PackageState::LocalInstall, ""),      // local install -> NeedsReview
                pkg("unknown", PackageState::Added, ""),            // no repo -> NeedsReview
            ],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 4);

        assert_eq!(result[0].attention[0].level, AttentionLevel::Routine);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageBaselineMatch
        );

        assert_eq!(result[1].attention[0].level, AttentionLevel::Routine);
        assert_eq!(
            result[1].attention[0].reason,
            AttentionReason::PackageUserAdded
        );

        assert_eq!(result[2].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(
            result[2].attention[0].reason,
            AttentionReason::PackageLocalInstall
        );

        assert_eq!(result[3].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(
            result[3].attention[0].reason,
            AttentionReason::PackageNoRepoSource
        );
    }

    // -----------------------------------------------------------------------
    // Multiarch: version direction must respect architecture
    // -----------------------------------------------------------------------

    #[test]
    fn multiarch_downgrade_only_affects_matching_arch() {
        // openssl.x86_64 upgraded, openssl.i686 downgraded.
        // Each arch should get its own correct attention level.
        let snap = snap_with_baseline_and_vc(
            Some(vec!["openssl".into()]),
            vec![
                pkg_arch("openssl", "x86_64", PackageState::Modified, "rhel-9-baseos"),
                pkg_arch("openssl", "i686", PackageState::Modified, "rhel-9-baseos"),
            ],
            vec![
                vc_arch("openssl", "x86_64", VersionChangeDirection::Upgrade),
                vc_arch("openssl", "i686", VersionChangeDirection::Downgrade),
            ],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 2);

        // x86_64 was upgraded — should be Routine
        assert_eq!(result[0].attention[0].level, AttentionLevel::Routine);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::PackageVersionChanged
        );
        assert_eq!(result[0].attention[0].detail.as_deref(), Some("Upgrade"));

        // i686 was downgraded — should be NeedsReview
        assert_eq!(result[1].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(
            result[1].attention[0].reason,
            AttentionReason::PackageVersionChanged
        );
        assert_eq!(result[1].attention[0].detail.as_deref(), Some("Downgrade"));
    }

    // -----------------------------------------------------------------------
    // Config attention: OS-default sensitive path suppression
    // -----------------------------------------------------------------------

    use inspectah_core::types::config::{ConfigCategory, ConfigFileEntry, ConfigSection};

    fn config_entry(path: &str, kind: ConfigFileKind) -> ConfigFileEntry {
        ConfigFileEntry {
            path: path.to_string(),
            kind,
            category: ConfigCategory::default(),
            include: true,
            ..Default::default()
        }
    }

    #[test]
    fn os_default_pki_rpm_gpg_stays_informational() {
        let snap = InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![config_entry(
                    "/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial",
                    ConfigFileKind::Unowned,
                )],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = compute_config_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention.len(), 1, "no SensitivePath promotion");
        assert_eq!(result[0].attention[0].level, AttentionLevel::Informational);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::ConfigUnowned
        );
    }

    #[test]
    fn os_default_security_pam_stays_informational() {
        let snap = InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![config_entry(
                    "/etc/security/limits.conf",
                    ConfigFileKind::Unowned,
                )],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = compute_config_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention.len(), 1, "no SensitivePath promotion");
        assert_eq!(result[0].attention[0].level, AttentionLevel::Informational);
    }

    #[test]
    fn os_default_ssl_stays_informational() {
        let snap = InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![config_entry(
                    "/etc/ssl/openssl.cnf",
                    ConfigFileKind::Unowned,
                )],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = compute_config_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention.len(), 1, "no SensitivePath promotion");
        assert_eq!(result[0].attention[0].level, AttentionLevel::Informational);
    }

    #[test]
    fn os_default_ssh_moduli_stays_informational() {
        let snap = InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![config_entry("/etc/ssh/moduli", ConfigFileKind::Unowned)],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = compute_config_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention.len(), 1, "no SensitivePath promotion");
        assert_eq!(result[0].attention[0].level, AttentionLevel::Informational);
    }

    #[test]
    fn non_default_sensitive_path_still_promoted() {
        // /etc/shadow is sensitive but NOT in the OS-default list — should promote.
        let snap = InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![config_entry("/etc/shadow", ConfigFileKind::Unowned)],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = compute_config_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].attention.len(),
            2,
            "SensitivePath promotion applied"
        );
        assert_eq!(result[0].attention[0].level, AttentionLevel::Informational);
        assert_eq!(result[0].attention[1].level, AttentionLevel::NeedsReview);
        assert_eq!(
            result[0].attention[1].reason,
            AttentionReason::SensitivePath
        );
    }

    #[test]
    fn rpm_modified_in_sensitive_path_still_needs_review() {
        // RpmOwnedModified is already NeedsReview — not affected by the overlay.
        let snap = InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![config_entry(
                    "/etc/ssh/sshd_config",
                    ConfigFileKind::RpmOwnedModified,
                )],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = compute_config_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(
            result[0].attention[0].reason,
            AttentionReason::ConfigModified
        );
    }

    #[test]
    fn unknown_pki_subdir_still_promoted() {
        // A file in /etc/pki/ that is NOT in a known OS-default subdir should
        // still be promoted. /etc/pki/tls/custom.pem is not under any allowlisted prefix.
        let snap = InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![config_entry(
                    "/etc/pki/tls/custom.pem",
                    ConfigFileKind::Unowned,
                )],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = compute_config_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].attention.len(),
            2,
            "SensitivePath promotion applied"
        );
        assert_eq!(result[0].attention[1].level, AttentionLevel::NeedsReview);
    }

    // -----------------------------------------------------------------------
    // Baseline-suppressed attention gating
    // -----------------------------------------------------------------------

    #[test]
    fn test_baseline_suppressed_package_gets_routine_not_needs_review() {
        let mut snap = InspectionSnapshot::default();
        let mut rpm = RpmSection::default();
        rpm.packages_added = vec![PackageEntry {
            name: "bash".into(),
            arch: "x86_64".into(),
            version: "5.2.26".into(),
            release: "3.el9".into(),
            epoch: String::new(),
            state: PackageState::Modified,
            include: true,
            source_repo: "baseos".into(),
            ..Default::default()
        }];
        rpm.version_changes = vec![VersionChange {
            name: "bash".into(),
            arch: "x86_64".into(),
            host_version: "5.2.26-3.el9".into(),
            base_version: "5.2.26-4.el9".into(),
            host_epoch: String::new(),
            base_epoch: String::new(),
            direction: VersionChangeDirection::Downgrade,
        }];
        rpm.baseline_suppressed = Some(vec!["bash.x86_64".into()]);
        snap.rpm = Some(rpm);

        let result = compute_package_attention(&snap);
        let bash = result.iter().find(|p| p.entry.name == "bash").unwrap();
        assert_eq!(bash.attention[0].level, AttentionLevel::Routine);
        assert_eq!(
            bash.attention[0].reason,
            AttentionReason::PackageBaselineMatch
        );
    }

    #[test]
    fn test_non_suppressed_downgrade_still_gets_needs_review() {
        let mut snap = InspectionSnapshot::default();
        let mut rpm = RpmSection::default();
        rpm.packages_added = vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            version: "2.4.57".into(),
            release: "4.el9".into(),
            epoch: String::new(),
            state: PackageState::Modified,
            include: true,
            source_repo: "appstream".into(),
            ..Default::default()
        }];
        rpm.version_changes = vec![VersionChange {
            name: "httpd".into(),
            arch: "x86_64".into(),
            host_version: "2.4.57-4.el9".into(),
            base_version: "2.4.57-5.el9".into(),
            host_epoch: String::new(),
            base_epoch: String::new(),
            direction: VersionChangeDirection::Downgrade,
        }];
        rpm.baseline_suppressed = Some(Vec::new());
        snap.rpm = Some(rpm);
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });

        let result = compute_package_attention(&snap);
        let httpd = result.iter().find(|p| p.entry.name == "httpd").unwrap();
        assert_eq!(httpd.attention[0].level, AttentionLevel::NeedsReview);
    }

    // -----------------------------------------------------------------------
    // SensitiveRetained: unresolved hints surface as NeedsReview
    // -----------------------------------------------------------------------

    #[test]
    fn sensitive_retained_surfaces_unresolved_hints() {
        use inspectah_core::types::redaction::RedactionHint;

        let mut snap = InspectionSnapshot::default();
        snap.redaction_state = Some(RedactionState::SensitiveRetained {
            redacted_by: "inspectah 0.8.0".into(),
            config_hash: "abc".into(),
            unresolved_count: 1,
            unresolved_hints: vec![RedactionHint {
                path: "/etc/httpd/conf/httpd.conf".into(),
                reason: "possible credential".into(),
                confidence: None,
            }],
        });
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                include: true,
                ..Default::default()
            }],
        });

        let result = compute_config_attention(&snap);
        let config_attention = &result[0].attention;
        assert!(
            config_attention
                .iter()
                .any(|a| a.level == AttentionLevel::NeedsReview
                    && matches!(a.reason, AttentionReason::Custom(_))),
            "SensitiveRetained with unresolved hints must surface NeedsReview"
        );
    }
}
