use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::ConfigFileKind;
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::rpm::{PackageEntry, PackageState};
use crate::types::{AttentionLevel, AttentionReason, AttentionTag, RefinedConfig, RefinedPackage};

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

pub fn compute_package_attention(snap: &InspectionSnapshot) -> Vec<RefinedPackage> {
    let rpm = match &snap.rpm {
        Some(r) => r,
        None => return Vec::new(),
    };

    let baseline_names: Option<Vec<String>> = snap.baseline.as_ref().map(|b| {
        b.packages.keys().cloned().collect()
    });
    let baseline: Option<&[String]> = baseline_names.as_deref();

    rpm.packages_added
        .iter()
        .map(|entry| {
            let tag = classify_package(entry, baseline);
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

            RefinedPackage { entry: entry.clone(), attention: tags }
        })
        .collect()
}

fn classify_package(entry: &PackageEntry, baseline: Option<&[String]>) -> AttentionTag {
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

    // Modified packages ALWAYS need review, even if in baseline.
    if entry.state == PackageState::Modified {
        return match baseline {
            Some(_) => AttentionTag {
                level: AttentionLevel::NeedsReview,
                reason: AttentionReason::PackageVersionChanged,
                detail: None,
            },
            None => AttentionTag {
                level: AttentionLevel::Informational,
                reason: AttentionReason::PackageProvenanceUnavailable,
                detail: None,
            },
        };
    }

    // Classify based on baseline availability and membership (Added/BaseImageOnly only).
    match baseline {
        Some(names) if names.iter().any(|n| {
            let entry_key = format!("{}.{}", entry.name, entry.arch);
            n == &entry_key
        }) => {
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

    let mut configs: Vec<RefinedConfig> = config.files
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
            if is_sensitive_path(&entry.path) && tags[0].level == AttentionLevel::Informational {
                tags.push(AttentionTag {
                    level: AttentionLevel::NeedsReview,
                    reason: AttentionReason::SensitivePath,
                    detail: Some(entry.path.clone()),
                });
            }

            RefinedConfig { entry: entry.clone(), attention: tags }
        })
        .collect();

    // Surface unresolved redaction hints as needs-review tags on matching
    // config files. Only applies when the snapshot is PartiallyRedacted.
    if let Some(RedactionState::PartiallyRedacted { ref unresolved_hints, .. }) = snap.redaction_state {
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
    use inspectah_core::types::rpm::RpmSection;

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

    /// Helper: build a snapshot with baseline via snap.baseline (Phase 6) and packages_added.
    fn snap_with_baseline(
        baseline_names: Option<Vec<String>>,
        packages: Vec<PackageEntry>,
    ) -> InspectionSnapshot {
        let baseline = baseline_names.map(|names| {
            let pkgs = names.into_iter().map(|n| {
                let key = format!("{}.x86_64", n);
                let entry = BaselinePackageEntry {
                    name: n,
                    epoch: Some("0".to_string()),
                    version: "1.0".to_string(),
                    release: "1.el9".to_string(),
                    arch: "x86_64".to_string(),
                };
                (key, entry)
            }).collect();
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
        assert_eq!(result[0].attention[0].reason, AttentionReason::PackageBaselineMatch);
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
        assert_eq!(result[0].attention[0].reason, AttentionReason::PackageUserAdded);
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
        assert_eq!(result[0].attention[0].reason, AttentionReason::PackageNoRepoSource);
    }

    #[test]
    fn verified_modified_in_baseline_is_needs_review_version_changed() {
        // Modified packages ALWAYS need review, even when in baseline.
        let snap = snap_with_baseline(
            Some(vec!["kernel".into()]),
            vec![pkg("kernel", PackageState::Modified, "rhel-9-baseos")],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(result[0].attention[0].reason, AttentionReason::PackageVersionChanged);
    }

    #[test]
    fn verified_modified_not_in_baseline_recognized_repo_is_needs_review() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into()]),
            vec![pkg("kernel", PackageState::Modified, "rhel-9-baseos")],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(result[0].attention[0].reason, AttentionReason::PackageVersionChanged);
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
        assert_eq!(result[0].attention[0].reason, AttentionReason::PackageNoRepoSource);
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
        assert_eq!(result[0].attention[0].reason, AttentionReason::PackageLocalInstall);
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
        assert_eq!(result[0].attention[0].reason, AttentionReason::PackageNoRepoSource);
    }

    #[test]
    fn snap_baseline_field_drives_verified_mode() {
        // Verify that compute_package_attention reads snap.baseline (Phase 6),
        // not rpm.baseline_package_names (Go compat).
        use std::collections::HashMap;
        let mut pkgs = HashMap::new();
        pkgs.insert("glibc.x86_64".to_string(), BaselinePackageEntry {
            name: "glibc".to_string(),
            epoch: Some("0".to_string()),
            version: "2.34".to_string(),
            release: "83.el9".to_string(),
            arch: "x86_64".to_string(),
        });
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
        assert_eq!(result[0].attention[0].reason, AttentionReason::PackageBaselineMatch);
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
        assert_eq!(result[0].attention[0].reason, AttentionReason::PackageProvenanceUnavailable);
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
        assert_eq!(result[0].attention[0].reason, AttentionReason::PackageLocalInstall);
    }

    #[test]
    fn degraded_no_repo_state_still_needs_review() {
        let snap = snap_with_baseline(
            None,
            vec![pkg("orphan", PackageState::NoRepo, "some-repo")],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(result[0].attention[0].reason, AttentionReason::PackageNoRepoSource);
    }

    #[test]
    fn degraded_empty_source_repo_is_needs_review() {
        let snap = snap_with_baseline(
            None,
            vec![pkg("mystery", PackageState::Added, "")],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(result[0].attention[0].reason, AttentionReason::PackageNoRepoSource);
    }

    // -----------------------------------------------------------------------
    // Multiple packages in one snapshot
    // -----------------------------------------------------------------------

    #[test]
    fn verified_mixed_packages_classification() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into(), "bash".into()]),
            vec![
                pkg("glibc", PackageState::Added, "rhel-9-baseos"),   // baseline match -> Routine
                pkg("httpd", PackageState::Added, "rhel-9-appstream"), // user-added -> Routine
                pkg("custom", PackageState::LocalInstall, ""),         // local install -> NeedsReview
                pkg("unknown", PackageState::Added, ""),               // no repo -> NeedsReview
            ],
        );
        let result = compute_package_attention(&snap);
        assert_eq!(result.len(), 4);

        assert_eq!(result[0].attention[0].level, AttentionLevel::Routine);
        assert_eq!(result[0].attention[0].reason, AttentionReason::PackageBaselineMatch);

        assert_eq!(result[1].attention[0].level, AttentionLevel::Routine);
        assert_eq!(result[1].attention[0].reason, AttentionReason::PackageUserAdded);

        assert_eq!(result[2].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(result[2].attention[0].reason, AttentionReason::PackageLocalInstall);

        assert_eq!(result[3].attention[0].level, AttentionLevel::NeedsReview);
        assert_eq!(result[3].attention[0].reason, AttentionReason::PackageNoRepoSource);
    }
}
