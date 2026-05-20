pub mod manifest;
pub mod merge;
pub mod validate;

use std::collections::BTreeMap;

use crate::baseline::{ResolutionStrategy, TargetImageIdentity};
use crate::snapshot::{InspectionSnapshot, SCHEMA_VERSION};
use crate::types::completeness::{Completeness, InspectorId};
use crate::types::fleet::FleetSnapshotMeta;

use self::manifest::FleetManifest;
use self::merge::{
    merge_config_sections, merge_container_sections, merge_kernelboot_sections,
    merge_network_sections, merge_nonrpm_sections, merge_rpm_sections,
    merge_scheduled_sections, merge_selinux_sections, merge_service_sections,
    merge_storage_sections, merge_usersgroups_sections,
};
use self::validate::{extract_hostname, FleetValidationError, FleetWarning};

/// Merge multiple host snapshots into a single fleet-aggregate snapshot.
///
/// Validates inputs first — returns hard errors if validation fails.
/// On success, returns the merged snapshot and any non-fatal warnings.
pub fn merge_snapshots(
    snapshots: Vec<InspectionSnapshot>,
    manifest: Option<&FleetManifest>,
) -> Result<(InspectionSnapshot, Vec<FleetWarning>), Vec<FleetValidationError>> {
    let validation = validate::validate_snapshots(&snapshots);
    if !validation.errors.is_empty() {
        return Err(validation.errors);
    }

    let total = snapshots.len();

    // CANONICAL HOST ORDERING: sort snapshots by hostname FIRST, then
    // derive the hostnames vec. All downstream code uses index into
    // this sorted vec as host_idx. This is the SINGLE source of truth
    // for host ordering.
    let mut indexed: Vec<(String, InspectionSnapshot)> = snapshots
        .into_iter()
        .map(|s| (extract_hostname(&s), s))
        .collect();
    indexed.sort_by(|(a, _), (b, _)| a.cmp(b));
    let hostnames: Vec<String> = indexed.iter().map(|(h, _)| h.clone()).collect();
    let sorted_snapshots: Vec<InspectionSnapshot> =
        indexed.into_iter().map(|(_, s)| s).collect();

    let section_host_counts = compute_section_host_counts(&sorted_snapshots);

    // Snapshot-level field merging
    let (target_image, baseline_provisional) =
        select_target_image(&sorted_snapshots, manifest);
    let baseline = select_baseline(&sorted_snapshots, &target_image);
    let completeness = merge_completeness(&sorted_snapshots);

    let fleet_meta = FleetSnapshotMeta {
        label: manifest
            .and_then(|m| m.label.clone())
            .unwrap_or_else(|| "fleet".into()),
        host_count: total,
        hostnames: hostnames.clone(),
        merged_at: chrono::Utc::now().to_rfc3339(),
        baseline_provisional,
        section_host_counts,
    };

    // Extract section Option vecs for adapters (consumed by value)
    let rpm_sections: Vec<Option<_>> =
        sorted_snapshots.iter().map(|s| s.rpm.clone()).collect();
    let config_sections: Vec<Option<_>> =
        sorted_snapshots.iter().map(|s| s.config.clone()).collect();
    let service_sections: Vec<Option<_>> =
        sorted_snapshots.iter().map(|s| s.services.clone()).collect();
    let network_sections: Vec<Option<_>> =
        sorted_snapshots.iter().map(|s| s.network.clone()).collect();
    let storage_sections: Vec<Option<_>> =
        sorted_snapshots.iter().map(|s| s.storage.clone()).collect();
    let scheduled_sections: Vec<Option<_>> = sorted_snapshots
        .iter()
        .map(|s| s.scheduled_tasks.clone())
        .collect();
    let container_sections: Vec<Option<_>> =
        sorted_snapshots.iter().map(|s| s.containers.clone()).collect();
    let nonrpm_sections: Vec<Option<_>> = sorted_snapshots
        .iter()
        .map(|s| s.non_rpm_software.clone())
        .collect();
    let kernelboot_sections: Vec<Option<_>> =
        sorted_snapshots.iter().map(|s| s.kernel_boot.clone()).collect();
    let selinux_sections: Vec<Option<_>> =
        sorted_snapshots.iter().map(|s| s.selinux.clone()).collect();
    let usergroup_sections: Vec<Option<_>> =
        sorted_snapshots.iter().map(|s| s.users_groups.clone()).collect();

    let mut merged = InspectionSnapshot::new();
    merged.schema_version = SCHEMA_VERSION;
    merged.fleet_meta = Some(fleet_meta);
    merged.target_image = target_image;
    merged.baseline = baseline;
    merged.no_baseline = merged.baseline.is_none();
    merged.completeness = completeness;
    merged.redaction_state = None;
    merged.sensitive_snapshot = sorted_snapshots.iter().any(|s| s.sensitive_snapshot);
    merged.preserved_credentials = sorted_snapshots.iter().any(|s| s.preserved_credentials);
    merged.preserved_ssh_keys = sorted_snapshots.iter().any(|s| s.preserved_ssh_keys);
    // os_release from first host (already sorted by hostname)
    merged.os_release = sorted_snapshots
        .first()
        .and_then(|s| s.os_release.clone());

    // Merge each section via adapters
    merged.rpm = merge_rpm_sections(rpm_sections, total, &hostnames);
    merged.config = merge_config_sections(config_sections, total, &hostnames);
    merged.services = merge_service_sections(service_sections, total, &hostnames);
    merged.network = merge_network_sections(network_sections, total, &hostnames);
    merged.storage = merge_storage_sections(storage_sections, total, &hostnames);
    merged.scheduled_tasks = merge_scheduled_sections(scheduled_sections, total, &hostnames);
    merged.containers = merge_container_sections(container_sections, total, &hostnames);
    merged.non_rpm_software = merge_nonrpm_sections(nonrpm_sections, total, &hostnames);
    merged.kernel_boot = merge_kernelboot_sections(kernelboot_sections, total, &hostnames);
    merged.selinux = merge_selinux_sections(selinux_sections, total, &hostnames);
    merged.users_groups = merge_usersgroups_sections(usergroup_sections, total, &hostnames);

    Ok((merged, validation.warnings))
}

/// Count how many hosts have each section present.
fn compute_section_host_counts(snapshots: &[InspectionSnapshot]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();

    let section_checkers: &[(&str, fn(&InspectionSnapshot) -> bool)] = &[
        ("rpm", |s| s.rpm.is_some()),
        ("config", |s| s.config.is_some()),
        ("services", |s| s.services.is_some()),
        ("network", |s| s.network.is_some()),
        ("storage", |s| s.storage.is_some()),
        ("scheduled_tasks", |s| s.scheduled_tasks.is_some()),
        ("containers", |s| s.containers.is_some()),
        ("non_rpm_software", |s| s.non_rpm_software.is_some()),
        ("kernel_boot", |s| s.kernel_boot.is_some()),
        ("selinux", |s| s.selinux.is_some()),
        ("users_groups", |s| s.users_groups.is_some()),
    ];

    for (name, checker) in section_checkers {
        let count = snapshots.iter().filter(|s| checker(s)).count();
        if count > 0 {
            counts.insert((*name).to_string(), count);
        }
    }

    counts
}

/// Select the target image for the merged snapshot.
///
/// If the manifest provides a baseline override, use it with `CliOverride` strategy.
/// Otherwise, find the most-common `target_image` across inputs.
/// Ties broken by lexicographic `image_ref`.
///
/// Returns `(selected_target_image, baseline_provisional)`.
/// `baseline_provisional` is true when the baseline was auto-selected
/// from conflicting inputs (i.e., not all hosts agreed and no manifest override).
fn select_target_image(
    snapshots: &[InspectionSnapshot],
    manifest: Option<&FleetManifest>,
) -> (Option<TargetImageIdentity>, bool) {
    // Manifest override takes precedence
    if let Some(m) = manifest {
        if let Some(ref override_ref) = m.baseline {
            return (
                Some(TargetImageIdentity {
                    image_ref: override_ref.clone(),
                    strategy: ResolutionStrategy::CliOverride,
                }),
                false, // explicit override is not provisional
            );
        }
    }

    // Collect target images with counts
    let mut image_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for snap in snapshots {
        if let Some(ref ti) = snap.target_image {
            *image_counts.entry(ti.image_ref.clone()).or_insert(0) += 1;
        }
    }

    if image_counts.is_empty() {
        return (None, false);
    }

    // Find max count, break ties by lexicographic order
    let mut candidates: Vec<(String, usize)> = image_counts.into_iter().collect();
    candidates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let selected_ref = &candidates[0].0;
    let baseline_provisional = candidates.len() > 1;

    // Find the first snapshot with this target image to get the strategy
    let strategy = snapshots
        .iter()
        .find(|s| {
            s.target_image
                .as_ref()
                .map(|ti| &ti.image_ref == selected_ref)
                .unwrap_or(false)
        })
        .and_then(|s| s.target_image.as_ref())
        .map(|ti| ti.strategy.clone())
        .unwrap_or(ResolutionStrategy::OsRelease);

    (
        Some(TargetImageIdentity {
            image_ref: selected_ref.clone(),
            strategy,
        }),
        baseline_provisional,
    )
}

/// Select baseline data from the input whose target_image matches the selected one.
///
/// Uses the first match (sorted by hostname, since inputs are pre-sorted).
fn select_baseline(
    snapshots: &[InspectionSnapshot],
    selected_target: &Option<TargetImageIdentity>,
) -> Option<crate::baseline::BaselineData> {
    let target = selected_target.as_ref()?;

    snapshots
        .iter()
        .find(|s| {
            s.target_image
                .as_ref()
                .map(|ti| ti.image_ref == target.image_ref)
                .unwrap_or(false)
                && s.baseline.is_some()
        })
        .and_then(|s| s.baseline.clone())
}

/// Merge completeness across all snapshots.
///
/// - All Complete -> Complete
/// - Any Incomplete -> Incomplete (union failed + degraded sections)
/// - Any Partial (none Incomplete) -> Partial (union degraded sections)
fn merge_completeness(snapshots: &[InspectionSnapshot]) -> Completeness {
    let mut has_incomplete = false;
    let mut has_partial = false;
    let mut all_failed: Vec<InspectorId> = Vec::new();
    let mut all_degraded: Vec<InspectorId> = Vec::new();
    let mut reasons: Vec<String> = Vec::new();

    for snap in snapshots {
        match &snap.completeness {
            Completeness::Complete => {}
            Completeness::Partial {
                degraded_sections,
                reason,
            } => {
                has_partial = true;
                for id in degraded_sections {
                    if !all_degraded.contains(id) {
                        all_degraded.push(*id);
                    }
                }
                if !reason.is_empty() && !reasons.contains(reason) {
                    reasons.push(reason.clone());
                }
            }
            Completeness::Incomplete {
                failed_sections,
                degraded_sections,
                reason,
            } => {
                has_incomplete = true;
                for id in failed_sections {
                    if !all_failed.contains(id) {
                        all_failed.push(*id);
                    }
                }
                for id in degraded_sections {
                    if !all_degraded.contains(id) {
                        all_degraded.push(*id);
                    }
                }
                if !reason.is_empty() && !reasons.contains(reason) {
                    reasons.push(reason.clone());
                }
            }
        }
    }

    if has_incomplete {
        let host_count = snapshots
            .iter()
            .filter(|s| matches!(s.completeness, Completeness::Incomplete { .. }))
            .count();
        let merged_reason = format!(
            "{} host(s) incomplete: {}",
            host_count,
            reasons.join("; ")
        );
        Completeness::Incomplete {
            failed_sections: all_failed,
            degraded_sections: all_degraded,
            reason: merged_reason,
        }
    } else if has_partial {
        let host_count = snapshots
            .iter()
            .filter(|s| matches!(s.completeness, Completeness::Partial { .. }))
            .count();
        let merged_reason = format!(
            "{} host(s) partial: {}",
            host_count,
            reasons.join("; ")
        );
        Completeness::Partial {
            degraded_sections: all_degraded,
            reason: merged_reason,
        }
    } else {
        Completeness::Complete
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_section_host_counts_empty() {
        let snaps = vec![InspectionSnapshot::new(), InspectionSnapshot::new()];
        let counts = compute_section_host_counts(&snaps);
        assert!(counts.is_empty());
    }

    #[test]
    fn test_compute_section_host_counts_mixed() {
        let mut s1 = InspectionSnapshot::new();
        s1.rpm = Some(crate::types::rpm::RpmSection::default());
        s1.config = Some(crate::types::config::ConfigSection::default());

        let mut s2 = InspectionSnapshot::new();
        s2.rpm = Some(crate::types::rpm::RpmSection::default());

        let counts = compute_section_host_counts(&[s1, s2]);
        assert_eq!(counts.get("rpm"), Some(&2));
        assert_eq!(counts.get("config"), Some(&1));
        assert!(counts.get("services").is_none());
    }

    #[test]
    fn test_select_target_image_manifest_override() {
        let snaps = vec![InspectionSnapshot::new()];
        let manifest = FleetManifest {
            label: None,
            baseline: Some("registry.example.com/rhel:9.4".into()),
            sources: vec![],
        };
        let (ti, provisional) = select_target_image(&snaps, Some(&manifest));
        assert!(!provisional);
        let ti = ti.unwrap();
        assert_eq!(ti.image_ref, "registry.example.com/rhel:9.4");
        assert_eq!(ti.strategy, ResolutionStrategy::CliOverride);
    }

    #[test]
    fn test_select_target_image_most_common() {
        let mut s1 = InspectionSnapshot::new();
        s1.target_image = Some(TargetImageIdentity {
            image_ref: "quay.io/rhel:9.4".into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        let mut s2 = InspectionSnapshot::new();
        s2.target_image = Some(TargetImageIdentity {
            image_ref: "quay.io/rhel:9.4".into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        let mut s3 = InspectionSnapshot::new();
        s3.target_image = Some(TargetImageIdentity {
            image_ref: "quay.io/rhel:9.3".into(),
            strategy: ResolutionStrategy::OsRelease,
        });

        let (ti, provisional) = select_target_image(&[s1, s2, s3], None);
        assert!(provisional);
        let ti = ti.unwrap();
        assert_eq!(ti.image_ref, "quay.io/rhel:9.4");
    }

    #[test]
    fn test_select_target_image_tie_break_lexicographic() {
        let mut s1 = InspectionSnapshot::new();
        s1.target_image = Some(TargetImageIdentity {
            image_ref: "b-image:1".into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        let mut s2 = InspectionSnapshot::new();
        s2.target_image = Some(TargetImageIdentity {
            image_ref: "a-image:1".into(),
            strategy: ResolutionStrategy::OsRelease,
        });

        let (ti, provisional) = select_target_image(&[s1, s2], None);
        assert!(provisional);
        // Equal counts, lexicographic winner is "a-image:1"
        assert_eq!(ti.unwrap().image_ref, "a-image:1");
    }

    #[test]
    fn test_select_target_image_none_when_no_targets() {
        let snaps = vec![InspectionSnapshot::new(), InspectionSnapshot::new()];
        let (ti, provisional) = select_target_image(&snaps, None);
        assert!(ti.is_none());
        assert!(!provisional);
    }

    #[test]
    fn test_select_baseline_matching() {
        let mut s1 = InspectionSnapshot::new();
        s1.target_image = Some(TargetImageIdentity {
            image_ref: "quay.io/rhel:9.4".into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        s1.baseline = Some(crate::baseline::BaselineData {
            image_digest: "sha256:abc".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });

        let target = Some(TargetImageIdentity {
            image_ref: "quay.io/rhel:9.4".into(),
            strategy: ResolutionStrategy::OsRelease,
        });

        let baseline = select_baseline(&[s1], &target);
        assert!(baseline.is_some());
        assert_eq!(baseline.unwrap().image_digest, "sha256:abc");
    }

    #[test]
    fn test_select_baseline_no_match() {
        let mut s1 = InspectionSnapshot::new();
        s1.target_image = Some(TargetImageIdentity {
            image_ref: "quay.io/rhel:9.3".into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        s1.baseline = Some(crate::baseline::BaselineData {
            image_digest: "sha256:abc".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });

        let target = Some(TargetImageIdentity {
            image_ref: "quay.io/rhel:9.4".into(),
            strategy: ResolutionStrategy::OsRelease,
        });

        let baseline = select_baseline(&[s1], &target);
        assert!(baseline.is_none());
    }

    #[test]
    fn test_merge_completeness_all_complete() {
        let s1 = InspectionSnapshot::new();
        let s2 = InspectionSnapshot::new();
        let result = merge_completeness(&[s1, s2]);
        assert_eq!(result, Completeness::Complete);
    }

    #[test]
    fn test_merge_completeness_with_partial() {
        let mut s1 = InspectionSnapshot::new();
        s1.completeness = Completeness::Partial {
            degraded_sections: vec![InspectorId::Rpm],
            reason: "rpm timeout".into(),
        };
        let s2 = InspectionSnapshot::new();
        let result = merge_completeness(&[s1, s2]);
        match result {
            Completeness::Partial {
                degraded_sections,
                reason,
            } => {
                assert!(degraded_sections.contains(&InspectorId::Rpm));
                assert!(reason.contains("1 host(s) partial"));
            }
            other => panic!("expected Partial, got {:?}", other),
        }
    }

    #[test]
    fn test_merge_completeness_incomplete_wins() {
        let mut s1 = InspectionSnapshot::new();
        s1.completeness = Completeness::Partial {
            degraded_sections: vec![InspectorId::Config],
            reason: "config degraded".into(),
        };
        let mut s2 = InspectionSnapshot::new();
        s2.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Rpm],
            degraded_sections: vec![InspectorId::Network],
            reason: "rpm failed".into(),
        };
        let result = merge_completeness(&[s1, s2]);
        match result {
            Completeness::Incomplete {
                failed_sections,
                degraded_sections,
                reason,
            } => {
                assert!(failed_sections.contains(&InspectorId::Rpm));
                // Union of all degraded across all inputs
                assert!(degraded_sections.contains(&InspectorId::Config));
                assert!(degraded_sections.contains(&InspectorId::Network));
                assert!(reason.contains("1 host(s) incomplete"));
            }
            other => panic!("expected Incomplete, got {:?}", other),
        }
    }

    #[test]
    fn test_merge_completeness_dedup_inspector_ids() {
        let mut s1 = InspectionSnapshot::new();
        s1.completeness = Completeness::Partial {
            degraded_sections: vec![InspectorId::Rpm, InspectorId::Config],
            reason: "some reason".into(),
        };
        let mut s2 = InspectionSnapshot::new();
        s2.completeness = Completeness::Partial {
            degraded_sections: vec![InspectorId::Rpm],
            reason: "some reason".into(),
        };
        let result = merge_completeness(&[s1, s2]);
        match result {
            Completeness::Partial {
                degraded_sections, ..
            } => {
                // Rpm should appear only once
                assert_eq!(
                    degraded_sections
                        .iter()
                        .filter(|id| **id == InspectorId::Rpm)
                        .count(),
                    1
                );
                assert_eq!(degraded_sections.len(), 2);
            }
            other => panic!("expected Partial, got {:?}", other),
        }
    }
}
