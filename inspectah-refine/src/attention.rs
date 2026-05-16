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

    let baseline: Option<&[String]> = rpm.baseline_package_names.as_deref();

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

    // Classify based on baseline availability and membership.
    match baseline {
        Some(names) if names.iter().any(|n| n == &entry.name) => {
            // In baseline with known repo — expected package, Tier 1.
            AttentionTag {
                level: AttentionLevel::Routine,
                reason: AttentionReason::PackageBaselineMatch,
                detail: None,
            }
        }
        Some(_) => {
            // Not in baseline but has a known repo — user-added or version-changed, Tier 2.
            let reason = match entry.state {
                PackageState::Modified => AttentionReason::PackageVersionChanged,
                _ => AttentionReason::PackageUserAdded,
            };
            AttentionTag { level: AttentionLevel::Informational, reason, detail: None }
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
            let mut tags = Vec::new();
            match entry.kind {
                ConfigFileKind::RpmOwnedModified => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::NeedsReview,
                        reason: AttentionReason::ConfigModified,
                        detail: None,
                    });
                }
                ConfigFileKind::Unowned => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::NeedsReview,
                        reason: AttentionReason::ConfigUnowned,
                        detail: None,
                    });
                }
                ConfigFileKind::Orphaned => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::Informational,
                        reason: AttentionReason::ConfigOrphaned,
                        detail: None,
                    });
                }
                ConfigFileKind::RpmOwnedDefault => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::Routine,
                        reason: AttentionReason::ConfigModified,
                        detail: None,
                    });
                }
                ConfigFileKind::BaselineMatch => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::Routine,
                        reason: AttentionReason::ConfigBaselineMatch,
                        detail: None,
                    });
                }
            }
            if is_sensitive_path(&entry.path) {
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
