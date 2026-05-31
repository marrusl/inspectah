// inspectah-web/src/adapter.rs
//
// Builds a `ViewResponse` from a `RefineSession` by reading session state
// and mapping domain types to presentation-layer DTOs. Produces JSON
// identical to the legacy `build_view_response` in handlers.rs.

use std::collections::BTreeSet;

use inspectah_core::types::rpm::VersionChangeDirection;
use inspectah_core::types::users::UserGroupDecision;
use inspectah_refine::classify::{
    classify_containers, classify_services, classify_sysctls, classify_tuned,
};
use inspectah_refine::repo_index::RepoIndex;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::RepoProvenance;

use crate::web_types::{
    DropInDecisionDto, FlatpakDecisionDto, QuadletDecisionDto, RepoGroupInfo,
    ServiceDecisionDto, SysctlDecisionDto, TunedDecisionDto, VersionChangeEntry,
    ViewResponse,
};

/// Build a complete [`ViewResponse`] from session state.
///
/// This is a pure mapping layer: it reads the session's view, decisions,
/// and baseline summary, then maps each domain type to its DTO counterpart.
/// The output serializes to JSON identical to the legacy handler path.
pub fn build_web_view(session: &RefineSession) -> ViewResponse {
    let view = session.view().clone();
    let repo_groups = build_repo_groups(session);
    let baseline_summary = session.baseline_summary();
    let version_changes = build_version_changes(session);
    let (service_states, service_dropins) = build_service_decisions(session);
    let (quadlets, flatpaks) = build_container_decisions(session);
    let sysctls = build_sysctl_decisions(session);
    let tuned = build_tuned_decisions(session);
    let users_groups_decisions = build_users_groups_decisions(session);
    let session_is_sensitive = session.is_sensitive();

    ViewResponse {
        view,
        repo_groups,
        baseline_summary,
        version_changes,
        service_states,
        service_dropins,
        quadlets,
        flatpaks,
        sysctls,
        tuned,
        users_groups_decisions,
        session_is_sensitive,
    }
}

// -- Version changes -------------------------------------------------------

fn build_version_changes(session: &RefineSession) -> Vec<VersionChangeEntry> {
    session
        .snapshot()
        .rpm
        .as_ref()
        .map(|rpm| {
            rpm.version_changes
                .iter()
                .map(|vc| {
                    let dir = match vc.direction {
                        VersionChangeDirection::Upgrade => "upgrade",
                        VersionChangeDirection::Downgrade => "downgrade",
                    };
                    VersionChangeEntry {
                        name: vc.name.clone(),
                        arch: vc.arch.clone(),
                        host_version: vc.host_version.clone(),
                        base_version: vc.base_version.clone(),
                        host_epoch: vc.host_epoch.clone(),
                        base_epoch: vc.base_epoch.clone(),
                        direction: dir.to_string(),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

// -- Repo groups -----------------------------------------------------------

fn build_repo_groups(session: &RefineSession) -> Vec<RepoGroupInfo> {
    let view = session.view();
    let repo_index = session.repo_index();
    let changes = session.pending_changes();
    let repos_excluded = changes.repos_excluded();
    let excluded: BTreeSet<&str> = repos_excluded.iter().map(|s| s.as_str()).collect();

    // Count visible packages per source_repo (lowercased for consistency
    // with RepoIndex, which normalizes section IDs to lowercase).
    let mut repo_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for pkg in &view.packages {
        let section = pkg.entry.source_repo.to_lowercase();
        *repo_counts.entry(section).or_insert(0) += 1;
    }

    // Also include repos known to the index but not visible (0-count).
    for section_id in repo_index.packages_by_repo.keys() {
        repo_counts.entry(section_id.clone()).or_insert(0);
    }

    let mut groups: Vec<RepoGroupInfo> = repo_counts
        .into_iter()
        .map(|(section_id, package_count)| {
            let provenance = if section_id.is_empty() {
                RepoProvenance::Unknown
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
            RepoGroupInfo {
                section_id,
                provenance,
                is_distro,
                tier,
                package_count,
                enabled,
            }
        })
        .collect();

    // Sort: distro repos first, then by section_id.
    groups.sort_by(|a, b| {
        b.is_distro
            .cmp(&a.is_distro)
            .then_with(|| a.section_id.cmp(&b.section_id))
    });

    groups
}

// -- Service decisions -----------------------------------------------------

fn build_service_decisions(
    session: &RefineSession,
) -> (Vec<ServiceDecisionDto>, Vec<DropInDecisionDto>) {
    let snap = session.snapshot_projected();
    let (states, dropins) = classify_services(&snap);

    let state_dtos: Vec<ServiceDecisionDto> = states
        .into_iter()
        .map(|s| ServiceDecisionDto {
            unit: s.entry.unit.clone(),
            triage: s.triage,
            include: s.entry.include,
            owning_package: s.entry.owning_package.clone(),
        })
        .collect();

    let dropin_dtos: Vec<DropInDecisionDto> = dropins
        .into_iter()
        .map(|d| DropInDecisionDto {
            unit: d.entry.unit.clone(),
            path: d.entry.path.clone(),
            triage: d.triage,
            include: d.entry.include,
        })
        .collect();

    (state_dtos, dropin_dtos)
}

// -- Container decisions ---------------------------------------------------

fn build_container_decisions(
    session: &RefineSession,
) -> (Vec<QuadletDecisionDto>, Vec<FlatpakDecisionDto>) {
    let snap = session.snapshot_projected();
    let (quadlets, flatpaks) = classify_containers(&snap);

    let quadlet_dtos: Vec<QuadletDecisionDto> = quadlets
        .into_iter()
        .map(|q| QuadletDecisionDto {
            path: q.entry.path.clone(),
            name: q.entry.name.clone(),
            image: q.entry.image.clone(),
            triage: q.triage,
            include: q.entry.include,
        })
        .collect();

    let flatpak_dtos: Vec<FlatpakDecisionDto> = flatpaks
        .into_iter()
        .map(|f| FlatpakDecisionDto {
            app_id: f.entry.app_id.clone(),
            remote: f.entry.remote.clone(),
            branch: f.entry.branch.clone(),
            triage: f.triage,
            include: f.entry.include,
            lifecycle: "first_boot".to_string(),
        })
        .collect();

    (quadlet_dtos, flatpak_dtos)
}

// -- Sysctl decisions ------------------------------------------------------

fn build_sysctl_decisions(session: &RefineSession) -> Vec<SysctlDecisionDto> {
    let snap = session.snapshot_projected();
    classify_sysctls(&snap)
        .into_iter()
        .map(|s| SysctlDecisionDto {
            key: s.entry.key.clone(),
            runtime: s.entry.runtime.clone(),
            default: s.entry.default.clone(),
            source: s.entry.source.clone(),
            triage: s.triage,
            include: s.entry.include,
        })
        .collect()
}

// -- Tuned decisions -------------------------------------------------------

fn build_tuned_decisions(session: &RefineSession) -> Vec<TunedDecisionDto> {
    let snap = session.snapshot_projected();
    let tuned_include = snap.kernel_boot.as_ref().is_none_or(|kb| kb.tuned_include);
    classify_tuned(&snap)
        .into_iter()
        .map(|t| TunedDecisionDto {
            active_profile: t.active_profile.clone(),
            custom_profiles: t.custom_profiles.clone(),
            triage: t.triage,
            include: tuned_include,
        })
        .collect()
}

// -- Users/groups decisions ------------------------------------------------

fn build_users_groups_decisions(session: &RefineSession) -> Vec<UserGroupDecision> {
    session
        .snapshot_projected()
        .users_groups
        .map(|ug| ug.users)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::snapshot::InspectionSnapshot;

    /// Verify that `build_web_view` produces the same JSON as the legacy
    /// `build_view_response` path in handlers.rs on an empty snapshot.
    #[test]
    fn adapter_matches_legacy_empty() {
        let snap = InspectionSnapshot::new();
        let session = RefineSession::new(snap);

        let adapter_response = build_web_view(&session);
        let legacy_response = crate::handlers::build_view_response_for_test(&session);

        let adapter_json =
            serde_json::to_value(&adapter_response).expect("serialize adapter response");
        let legacy_json =
            serde_json::to_value(&legacy_response).expect("serialize legacy response");

        assert_eq!(adapter_json, legacy_json, "adapter and legacy JSON must match");
    }

    /// Verify equivalence on a snapshot with RPM data (version changes, packages).
    #[test]
    fn adapter_matches_legacy_with_rpm() {
        use inspectah_core::types::rpm::{
            PackageEntry, PackageState, RpmSection, VersionChange, VersionChangeDirection,
        };

        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            }],
            version_changes: vec![VersionChange {
                name: "curl".into(),
                arch: "x86_64".into(),
                host_version: "8.6.0".into(),
                base_version: "8.5.0".into(),
                host_epoch: "0".into(),
                base_epoch: "0".into(),
                direction: VersionChangeDirection::Upgrade,
            }],
            ..Default::default()
        });
        let session = RefineSession::new(snap);

        let adapter_json =
            serde_json::to_value(build_web_view(&session)).expect("serialize adapter");
        let legacy_json =
            serde_json::to_value(crate::handlers::build_view_response_for_test(&session))
                .expect("serialize legacy");

        assert_eq!(adapter_json, legacy_json);
    }
}
