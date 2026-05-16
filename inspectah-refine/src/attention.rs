use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::ConfigFileKind;
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::rpm::PackageState;
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

    rpm.packages_added
        .iter()
        .map(|entry| {
            let mut tags = Vec::new();
            match entry.state {
                PackageState::Added => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::NeedsReview,
                        reason: AttentionReason::PackageNotInBaseline,
                        detail: None,
                    });
                }
                PackageState::LocalInstall => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::NeedsReview,
                        reason: AttentionReason::PackageLocalInstall,
                        detail: None,
                    });
                }
                PackageState::Modified => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::Informational,
                        reason: AttentionReason::PackageStateChanged,
                        detail: None,
                    });
                }
                PackageState::NoRepo => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::Informational,
                        reason: AttentionReason::PackageNoRepo,
                        detail: None,
                    });
                }
                _ => {
                    tags.push(AttentionTag {
                        level: AttentionLevel::Routine,
                        reason: AttentionReason::PackageStateChanged,
                        detail: None,
                    });
                }
            }
            RefinedPackage { entry: entry.clone(), attention: tags }
        })
        .collect()
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
