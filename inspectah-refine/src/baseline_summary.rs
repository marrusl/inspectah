use serde::{Deserialize, Serialize};

use inspectah_core::snapshot::InspectionSnapshot;

use crate::types::{RefinedPackage, TriageBucket, TriageReason};

/// Summary of baseline resolution for the web UI.
///
/// Counts reflect the **classification result** (attention reasons), NOT
/// mutable `include` booleans. They are stable across user include/exclude
/// operations — they tell the UI "how the system was classified," not
/// "what the user has triaged so far."
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineSummary {
    pub image_ref: String,
    pub image_digest: String,
    pub strategy: String,
    pub baseline_count: usize,
    pub user_added_count: usize,
    pub review_count: usize,
}

/// Derive a `BaselineSummary` from the snapshot and classified packages.
///
/// Returns `None` when `target_image` or `baseline` is absent — the UI
/// uses `Option<BaselineSummary>` to decide whether to show the baseline
/// verification banner.
pub fn derive_baseline_summary(
    snap: &InspectionSnapshot,
    packages: &[RefinedPackage],
) -> Option<BaselineSummary> {
    let target = snap.target_image.as_ref()?;
    let baseline = snap.baseline.as_ref()?;

    let baseline_count = packages
        .iter()
        .filter(|p| p.triage.primary_reason == TriageReason::PackageBaselineMatch)
        .count();
    let user_added_count = packages
        .iter()
        .filter(|p| p.triage.primary_reason == TriageReason::PackageUserAdded)
        .count();
    let review_count = packages
        .iter()
        .filter(|p| p.triage.bucket() == TriageBucket::Investigate)
        .count();

    Some(BaselineSummary {
        image_ref: target.image_ref.clone(),
        image_digest: baseline.image_digest.clone(),
        strategy: serde_json::to_string(&target.strategy)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string(),
        baseline_count,
        user_added_count,
        review_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::baseline::{
        BaselineData, BaselinePackageEntry, ResolutionStrategy, TargetImageIdentity,
    };
    use inspectah_core::snapshot::InspectionSnapshot;
    use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
    use std::collections::HashMap;

    use crate::classify::classify_packages;

    fn snapshot_with_baseline() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(TargetImageIdentity {
            image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.6".into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        let mut pkgs = HashMap::new();
        pkgs.insert(
            "bash".to_string(),
            BaselinePackageEntry {
                name: "bash".into(),
                epoch: None,
                version: "5.2.26".into(),
                release: "4.el9".into(),
                arch: "x86_64".into(),
            },
        );
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:abc123".into(),
            packages: pkgs,
            extracted_at: "2026-05-17T00:00:00Z".into(),
        });
        snap.rpm = Some(RpmSection {
            baseline_package_names: Some(vec!["bash".into()]),
            packages_added: vec![
                PackageEntry {
                    name: "bash".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    include: true,
                    source_repo: "baseos".into(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "httpd".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    include: true,
                    source_repo: "appstream".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        snap
    }

    #[test]
    fn test_derive_baseline_summary_basic() {
        let snap = snapshot_with_baseline();
        let packages = classify_packages(&snap);
        let summary = derive_baseline_summary(&snap, &packages);
        assert!(summary.is_some(), "summary should be present with baseline");
        let s = summary.unwrap();
        assert_eq!(s.image_ref, "registry.redhat.io/rhel9/rhel-bootc:9.6");
        assert_eq!(s.image_digest, "sha256:abc123");
        assert_eq!(s.strategy, "os-release");
    }

    #[test]
    fn test_derive_baseline_summary_none_without_target_image() {
        let mut snap = snapshot_with_baseline();
        snap.target_image = None;
        let packages = classify_packages(&snap);
        let summary = derive_baseline_summary(&snap, &packages);
        assert!(summary.is_none(), "no target_image -> None");
    }

    #[test]
    fn test_derive_baseline_summary_none_without_baseline() {
        let mut snap = snapshot_with_baseline();
        snap.baseline = None;
        let packages = classify_packages(&snap);
        let summary = derive_baseline_summary(&snap, &packages);
        assert!(summary.is_none(), "no baseline -> None");
    }

    #[test]
    fn test_counts_stable_across_include_exclude() {
        use crate::session::RefineSession;
        use crate::types::RefinementOp;

        let snap = snapshot_with_baseline();
        let mut session = RefineSession::new(snap);

        // Get initial counts
        let summary1 = session.baseline_summary();
        assert!(summary1.is_some());
        let s1 = summary1.unwrap();

        // Exclude a package
        let op = RefinementOp::SetInclude {
            item_id: crate::types::ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        };
        session.apply(op).unwrap();

        // Counts must be identical — they reflect classification, not triage
        let summary2 = session.baseline_summary();
        assert!(summary2.is_some());
        let s2 = summary2.unwrap();
        assert_eq!(
            s1.baseline_count, s2.baseline_count,
            "baseline_count must be stable"
        );
        assert_eq!(
            s1.user_added_count, s2.user_added_count,
            "user_added_count must be stable"
        );
        assert_eq!(
            s1.review_count, s2.review_count,
            "review_count must be stable"
        );
    }
}
