use std::collections::{BTreeMap, BTreeSet};

use inspectah_core::types::rpm::VersionChange;
use inspectah_core::types::users::UserGroupDecision;

use crate::classify::{classify_containers, classify_services, classify_sysctls, classify_tuned};
use crate::repo_index::RepoIndex;
use crate::session::RefineSession;

use super::types::{DecisionProjection, RepoGroup};

/// Build a [`DecisionProjection`] from the current session state.
///
/// Calls classify_* functions on the projected snapshot and reads
/// repo groups, version changes, users/groups, sensitivity, and
/// baseline summary from the session's public API.
pub fn project_decisions(session: &RefineSession) -> DecisionProjection {
    let snap = session.snapshot_projected();

    // Classify services → tuple of (states, dropins)
    let (service_states, service_dropins) = classify_services(&snap);

    // Classify containers → tuple of (quadlets, flatpaks)
    let (quadlets, flatpaks) = classify_containers(&snap);

    // Classify kernel/boot
    let sysctls = classify_sysctls(&snap);
    let tuned = classify_tuned(&snap);

    // Version changes from RPM section
    let version_changes: Vec<VersionChange> = snap
        .rpm
        .as_ref()
        .map(|r| r.version_changes.clone())
        .unwrap_or_default();

    // Users/groups from projected snapshot
    let users_groups: Vec<UserGroupDecision> = snap
        .users_groups
        .map(|ug| ug.users)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    // Repo groups — ported from handlers.rs build_repo_groups
    let repo_groups = build_repo_groups(session);

    // Sensitivity and baseline summary
    let is_sensitive = session.is_sensitive();
    let baseline_summary = session.baseline_summary();

    DecisionProjection {
        service_states,
        service_dropins,
        quadlets,
        flatpaks,
        sysctls,
        tuned,
        repo_groups,
        version_changes,
        users_groups,
        is_sensitive,
        baseline_summary,
    }
}

/// Build repo groups from the session's repo index and current view.
///
/// Ported from `inspectah-web/src/handlers.rs` `build_repo_groups`.
/// Produces `Vec<RepoGroup>` (projection type) instead of `Vec<RepoGroupInfo>`.
fn build_repo_groups(session: &RefineSession) -> Vec<RepoGroup> {
    let view = session.view();
    let repo_index = session.repo_index();
    let changes = session.pending_changes();
    let repos_excluded = changes.repos_excluded();
    let excluded: BTreeSet<&str> = repos_excluded.iter().map(|s| s.as_str()).collect();

    // Count visible packages per source_repo (lowercased for consistency
    // with RepoIndex, which normalizes section IDs to lowercase).
    let mut repo_counts: BTreeMap<String, usize> = BTreeMap::new();
    for pkg in &view.packages {
        let section = pkg.entry.source_repo.to_lowercase();
        *repo_counts.entry(section).or_insert(0) += 1;
    }

    // Also include repos known to the index but not visible (0-count)
    for section_id in repo_index.packages_by_repo.keys() {
        repo_counts.entry(section_id.clone()).or_insert(0);
    }

    let mut groups: Vec<RepoGroup> = repo_counts
        .into_iter()
        .map(|(section_id, package_count)| {
            let provenance = if section_id.is_empty() {
                crate::types::RepoProvenance::Unknown
            } else {
                repo_index.provenance(&section_id)
            };
            let is_distro = if section_id.is_empty() {
                false
            } else {
                RepoIndex::is_distro_repo(&section_id)
            };
            let tier = RepoIndex::repo_tier(&section_id);
            let enabled = !excluded.contains(section_id.as_str());
            RepoGroup {
                section_id,
                provenance,
                is_distro,
                tier,
                package_count,
                enabled,
            }
        })
        .collect();

    // Sort: distro repos first, then by section_id
    groups.sort_by(|a, b| {
        b.is_distro
            .cmp(&a.is_distro)
            .then_with(|| a.section_id.cmp(&b.section_id))
    });

    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::snapshot::InspectionSnapshot;
    use inspectah_core::types::rpm::{RpmSection, VersionChangeDirection};
    use inspectah_core::types::services::{
        PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
    };

    /// Snapshot with one service state change.
    fn snapshot_with_service() -> InspectionSnapshot {
        InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection::default()),
            services: Some(ServiceSection {
                state_changes: vec![ServiceStateChange {
                    unit: "httpd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    include: true,
                    owning_package: Some("httpd".into()),
                    fleet: None,
                    attention_reason: None,
                }],
                enabled_units: vec!["httpd.service".into()],
                disabled_units: vec![],
                drop_ins: vec![],
                preset_matched_units: vec![],
            }),
            ..Default::default()
        }
    }

    #[test]
    fn classified_services_appear_in_projection() {
        let session = RefineSession::new(snapshot_with_service());
        let proj = project_decisions(&session);

        assert_eq!(proj.service_states.len(), 1);
        assert_eq!(proj.service_states[0].entry.unit, "httpd.service");
    }

    #[test]
    fn empty_snapshot_produces_empty_projection() {
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection::default()),
            ..Default::default()
        };
        let session = RefineSession::new(snap);
        let proj = project_decisions(&session);

        assert!(proj.service_states.is_empty());
        assert!(proj.service_dropins.is_empty());
        assert!(proj.quadlets.is_empty());
        assert!(proj.flatpaks.is_empty());
        assert!(proj.sysctls.is_empty());
        assert!(proj.tuned.is_empty());
        assert!(proj.version_changes.is_empty());
        assert!(proj.users_groups.is_empty());
    }

    #[test]
    fn version_changes_pass_through_from_rpm() {
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                version_changes: vec![VersionChange {
                    name: "bash".into(),
                    arch: "x86_64".into(),
                    host_version: "5.2.15-3.el9".into(),
                    base_version: "5.1.8-6.el9".into(),
                    host_epoch: "0".into(),
                    base_epoch: "0".into(),
                    direction: VersionChangeDirection::Upgrade,
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let session = RefineSession::new(snap);
        let proj = project_decisions(&session);

        assert_eq!(proj.version_changes.len(), 1);
        assert_eq!(proj.version_changes[0].name, "bash");
        assert_eq!(proj.version_changes[0].direction, VersionChangeDirection::Upgrade);
    }
}
