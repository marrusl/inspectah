//! Shared presentation helpers for baseline metadata.
//!
//! Used by CLI, README, and audit renderers. All functions are pure —
//! they take typed inputs and return formatted strings.

use inspectah_core::baseline::{BaselineData, ResolutionStrategy};
use inspectah_core::types::rpm::{VersionChange, VersionChangeDirection};

/// Human-readable label for a resolution strategy.
pub fn strategy_label(strategy: &ResolutionStrategy) -> &'static str {
    match strategy {
        ResolutionStrategy::CliOverride => "--base-image (user-specified)",
        ResolutionStrategy::UniversalBlue => "ublue image-info.json",
        ResolutionStrategy::BootcStatus => "bootc status (booted deployment)",
        ResolutionStrategy::FedoraAtomicDesktop => "fedora-atomic-desktop image-info.json",
        ResolutionStrategy::OsRelease => "os-release (auto-detected)",
    }
}

/// Summarize version comparison results.
///
/// Takes `Option<&[VersionChange]>` to distinguish three states:
/// - `None` — comparison data unavailable (RPM section absent or degraded)
/// - `Some(&[])` — comparison ran, zero differences found
/// - `Some(&[...])` — comparison ran, differences found
///
/// `shared_count` is the number of packages present in both host and baseline.
/// This is NOT `baseline.packages.len()` (which includes baseline-only packages).
pub fn version_comparison_summary(
    version_changes: Option<&[VersionChange]>,
    shared_count: usize,
) -> String {
    match version_changes {
        None => "comparison data unavailable".to_string(),
        Some([]) => format!("all {shared_count} shared packages at same version"),
        Some(vcs) => {
            let upgrades = vcs
                .iter()
                .filter(|vc| vc.direction == VersionChangeDirection::Upgrade)
                .count();
            let downgrades = vcs.len() - upgrades;
            let detail = match (upgrades, downgrades) {
                (_, 0) => "all target-newer".to_string(),
                (0, _) => "all host-newer".to_string(),
                (u, d) => format!("{u} target-newer, {d} host-newer"),
            };
            format!(
                "{} shared packages with version changes ({})",
                vcs.len(),
                detail
            )
        }
    }
}

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::completeness::{Completeness, InspectorId};

/// Compute shared package count: baseline packages present on both host and baseline.
///
/// shared = total baseline packages - baseline-only packages.
/// Returns `None` when inputs don't allow a meaningful count.
pub fn shared_package_count(
    baseline: &BaselineData,
    rpm: &inspectah_core::types::rpm::RpmSection,
) -> usize {
    let total = baseline.packages.len();
    let baseline_only = rpm.base_image_only.len();
    total.saturating_sub(baseline_only)
}

/// Check whether RPM comparison data is trustworthy.
///
/// Returns `false` if the RPM inspector is degraded, failed, or if the RPM
/// section is absent. Uses the same completeness-check pattern as
/// `containerfile.rs:is_degraded`.
pub fn is_rpm_comparison_available(snap: &InspectionSnapshot) -> bool {
    if snap.rpm.is_none() {
        return false;
    }
    match &snap.completeness {
        Completeness::Partial {
            degraded_sections, ..
        } => !degraded_sections.contains(&InspectorId::Rpm),
        Completeness::Incomplete {
            failed_sections,
            degraded_sections,
            ..
        } => {
            !failed_sections.contains(&InspectorId::Rpm)
                && !degraded_sections.contains(&InspectorId::Rpm)
        }
        Completeness::Complete => true,
    }
}

/// Build version changes Option for the comparison summary.
///
/// Returns:
/// - `None` when RPM data is absent, degraded, or failed
/// - `Some(&[])` when comparison ran and found zero differences
/// - `Some(&vcs)` when differences exist
pub fn version_changes_for_display(snap: &InspectionSnapshot) -> Option<&[VersionChange]> {
    if !is_rpm_comparison_available(snap) {
        return None;
    }
    snap.rpm.as_ref().map(|r| r.version_changes.as_slice())
}

/// Build the baseline section lines for README and audit.
///
/// Returns an empty vec when `target_image` is absent (unknown state).
pub fn baseline_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let ti = match &snap.target_image {
        Some(ti) => ti,
        None => return vec![],
    };

    let mut lines = vec![
        "## Baseline comparison".into(),
        String::new(),
        "| | |".into(),
        "|---|---|".into(),
        format!("| Target image | {} |", ti.image_ref),
        format!("| Resolution | {} |", strategy_label(&ti.strategy)),
    ];

    match &snap.baseline {
        Some(bl) => {
            lines.push(format!("| Image digest | {} |", bl.image_digest));
            lines.push(format!("| Baseline extracted | {} |", bl.extracted_at));
            lines.push(format!("| Baseline packages | {} |", bl.packages.len()));

            let vc_display = version_changes_for_display(snap);
            let shared_count = match (snap.rpm.as_ref(), &snap.baseline) {
                (Some(rpm), Some(bl)) if is_rpm_comparison_available(snap) => {
                    shared_package_count(bl, rpm)
                }
                _ => 0,
            };
            lines.push(format!(
                "| Version changes | {} |",
                version_comparison_summary(vc_display, shared_count)
            ));
        }
        None => {
            if snap.no_baseline {
                lines.push("| Baseline | skipped (--no-baseline) |".into());
            } else {
                lines.push("| Baseline | unavailable |".into());
            }
        }
    }

    lines.push(String::new());
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::baseline::{BaselinePackageEntry, TargetImageIdentity};
    use inspectah_core::types::rpm::RpmSection;
    use std::collections::HashMap;

    #[test]
    fn strategy_label_all_variants() {
        assert_eq!(
            strategy_label(&ResolutionStrategy::CliOverride),
            "--base-image (user-specified)"
        );
        assert_eq!(
            strategy_label(&ResolutionStrategy::UniversalBlue),
            "ublue image-info.json"
        );
        assert_eq!(
            strategy_label(&ResolutionStrategy::BootcStatus),
            "bootc status (booted deployment)"
        );
        assert_eq!(
            strategy_label(&ResolutionStrategy::FedoraAtomicDesktop),
            "fedora-atomic-desktop image-info.json"
        );
        assert_eq!(
            strategy_label(&ResolutionStrategy::OsRelease),
            "os-release (auto-detected)"
        );
    }

    fn make_vc(direction: VersionChangeDirection) -> VersionChange {
        VersionChange {
            name: "test-pkg".into(),
            direction,
            ..Default::default()
        }
    }

    #[test]
    fn version_comparison_unavailable() {
        assert_eq!(
            version_comparison_summary(None, 447),
            "comparison data unavailable"
        );
    }

    #[test]
    fn version_comparison_zero_changes() {
        assert_eq!(
            version_comparison_summary(Some(&[]), 447),
            "all 447 shared packages at same version"
        );
    }

    #[test]
    fn version_comparison_all_upgrades() {
        let vcs = vec![
            make_vc(VersionChangeDirection::Upgrade),
            make_vc(VersionChangeDirection::Upgrade),
        ];
        let s = version_comparison_summary(Some(&vcs), 447);
        assert!(s.contains("2 shared packages with version changes"));
        assert!(s.contains("all target-newer"));
    }

    #[test]
    fn version_comparison_all_downgrades() {
        let vcs = vec![make_vc(VersionChangeDirection::Downgrade)];
        let s = version_comparison_summary(Some(&vcs), 447);
        assert!(s.contains("1 shared packages with version changes"));
        assert!(s.contains("all host-newer"));
    }

    #[test]
    fn version_comparison_mixed() {
        let vcs = vec![
            make_vc(VersionChangeDirection::Upgrade),
            make_vc(VersionChangeDirection::Downgrade),
        ];
        let s = version_comparison_summary(Some(&vcs), 447);
        assert!(s.contains("2 shared packages with version changes"));
        assert!(s.contains("1 target-newer, 1 host-newer"));
    }

    fn test_target_image() -> TargetImageIdentity {
        TargetImageIdentity {
            image_ref: "quay.io/centos-bootc/centos-bootc:stream9".into(),
            strategy: ResolutionStrategy::OsRelease,
        }
    }

    fn test_baseline() -> BaselineData {
        let mut packages = HashMap::new();
        for i in 0..447 {
            packages.insert(
                format!("pkg-{i}"),
                BaselinePackageEntry {
                    name: format!("pkg-{i}"),
                    epoch: Some("0".into()),
                    version: "1.0".into(),
                    release: "1.el9".into(),
                    arch: "x86_64".into(),
                },
            );
        }
        BaselineData {
            image_digest: "sha256:abc123def456".into(),
            packages,
            extracted_at: "2026-05-18T14:32:00Z".into(),
        }
    }

    #[test]
    fn section_lines_full_state() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = Some(test_baseline());
        snap.rpm = Some(RpmSection {
            version_changes: vec![make_vc(VersionChangeDirection::Upgrade)],
            ..Default::default()
        });
        let lines = baseline_section_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("Baseline comparison")));
        assert!(lines.iter().any(|l| l.contains("centos-bootc:stream9")));
        assert!(
            lines
                .iter()
                .any(|l| l.contains("os-release (auto-detected)"))
        );
        assert!(lines.iter().any(|l| l.contains("sha256:abc123def456")));
        assert!(lines.iter().any(|l| l.contains("447")));
        assert!(
            lines
                .iter()
                .any(|l| l.contains("1 shared packages with version changes"))
        );
    }

    #[test]
    fn section_lines_degraded_no_baseline() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = None;
        snap.no_baseline = false;
        let lines = baseline_section_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("unavailable")));
    }

    #[test]
    fn section_lines_skipped_no_baseline() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = None;
        snap.no_baseline = true;
        let lines = baseline_section_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("skipped (--no-baseline)")));
    }

    #[test]
    fn section_lines_unknown_state() {
        let snap = InspectionSnapshot::new();
        let lines = baseline_section_lines(&snap);
        assert!(lines.is_empty());
    }

    #[test]
    fn section_lines_comparison_unavailable_rpm_degraded() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = Some(test_baseline());
        snap.rpm = Some(RpmSection::default());
        snap.completeness = Completeness::Partial {
            degraded_sections: vec![InspectorId::Rpm],
            reason: "rpm inspector degraded".into(),
        };
        let lines = baseline_section_lines(&snap);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("comparison data unavailable"))
        );
    }

    #[test]
    fn section_lines_comparison_unavailable_rpm_absent() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = Some(test_baseline());
        snap.rpm = None;
        let lines = baseline_section_lines(&snap);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("comparison data unavailable"))
        );
    }

    #[test]
    fn shared_package_count_excludes_baseline_only() {
        let bl = test_baseline(); // 447 packages
        let rpm = RpmSection {
            base_image_only: vec![inspectah_core::types::rpm::PackageEntry {
                name: "baseline-only-pkg".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(shared_package_count(&bl, &rpm), 446);
    }

    #[test]
    fn is_rpm_comparison_available_complete() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection::default());
        snap.completeness = Completeness::Complete;
        assert!(is_rpm_comparison_available(&snap));
    }

    #[test]
    fn is_rpm_comparison_available_degraded() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection::default());
        snap.completeness = Completeness::Partial {
            degraded_sections: vec![InspectorId::Rpm],
            reason: "degraded".into(),
        };
        assert!(!is_rpm_comparison_available(&snap));
    }

    #[test]
    fn is_rpm_comparison_available_absent() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = None;
        assert!(!is_rpm_comparison_available(&snap));
    }

    #[test]
    fn readme_audit_parity() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = Some(test_baseline());
        snap.rpm = Some(RpmSection {
            version_changes: vec![make_vc(VersionChangeDirection::Upgrade)],
            ..Default::default()
        });
        let readme = crate::render::readme::render_readme(&snap);
        let audit = crate::render::audit::render_audit(&snap);

        // Both must contain the same baseline metadata
        assert!(readme.contains("centos-bootc:stream9"));
        assert!(audit.contains("centos-bootc:stream9"));
        assert!(readme.contains("os-release (auto-detected)"));
        assert!(audit.contains("os-release (auto-detected)"));
        assert!(readme.contains("sha256:abc123def456"));
        assert!(audit.contains("sha256:abc123def456"));
        assert!(readme.contains("1 shared packages with version changes"));
        assert!(audit.contains("1 shared packages with version changes"));
    }
}
