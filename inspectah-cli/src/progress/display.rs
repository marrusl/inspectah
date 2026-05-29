//! Shared display-order table and name mapping for scan inspectors.
//!
//! All progress renderers use these constants to present inspectors
//! in a consistent, user-friendly order regardless of the order
//! results arrive from parallel execution.

use inspectah_core::types::completeness::InspectorId;
use inspectah_core::types::progress::{MetricKind, ProbeId, StepId};

/// Fixed display order for the scan checklist.
///
/// Matches `scan.rs` inspector registration order. Only the 11
/// inspectors that produce [`SectionData`] appear here — phase-2-only
/// IDs (`Hardware`, `Ostree`, `OsRelease`) are excluded.
pub const DISPLAY_ORDER: &[(InspectorId, &str)] = &[
    (InspectorId::Rpm, "RPM packages"),
    (InspectorId::Services, "Services"),
    (InspectorId::Storage, "Storage"),
    (InspectorId::KernelBoot, "Kernel & boot"),
    (InspectorId::Network, "Network"),
    (InspectorId::Containers, "Containers"),
    (InspectorId::UsersGroups, "Users & groups"),
    (InspectorId::ScheduledTasks, "Scheduled tasks"),
    (InspectorId::Config, "Config files"),
    (InspectorId::Selinux, "SELinux"),
    (InspectorId::NonRpmSoftware, "Non-RPM packages"),
];

/// Get the 1-based display position for an inspector.
///
/// Returns `0` for inspectors not in the display order table
/// (e.g. phase-2-only IDs).
pub fn display_position(id: InspectorId) -> usize {
    DISPLAY_ORDER
        .iter()
        .position(|(oid, _)| *oid == id)
        .map(|p| p + 1)
        .unwrap_or(0)
}

/// Get the human-readable display name for an inspector.
///
/// Returns `"Unknown"` for inspectors not in the display order table.
pub fn display_name(id: InspectorId) -> &'static str {
    DISPLAY_ORDER
        .iter()
        .find(|(oid, _)| *oid == id)
        .map(|(_, name)| *name)
        .unwrap_or("Unknown")
}

/// Get the human-readable display name for a step.
pub fn step_name(id: &StepId) -> &'static str {
    match id {
        StepId::QueryingPackages => "Querying installed packages",
        StepId::ClassifyingPackages => "Classifying packages",
        StepId::ResolvingSourceRepos => "Resolving source repositories",
        StepId::ResolvingDepTree => "Resolving dependency tree",
        StepId::VerifyingIntegrity => "Verifying package integrity",
        StepId::MappingFileOwnership => "Mapping file ownership",
        StepId::ApplyingRpmVerification => "Applying RPM verification results",
        StepId::WalkingFilesystem => "Walking filesystem",
        StepId::ClassifyingConfigs => "Classifying configs",
    }
}

/// Get the human-readable display name for a probe.
pub fn probe_name(id: &ProbeId) -> &'static str {
    match id {
        ProbeId::ElfBinaries => "ELF binaries",
        ProbeId::PythonVenvs => "Python virtualenvs",
        ProbeId::PipPackages => "pip packages",
        ProbeId::NpmPackages => "npm packages",
        ProbeId::GemPackages => "gem packages",
        ProbeId::EnvFiles => ".env files",
        ProbeId::GitRepos => "git repos",
    }
}

/// Format a metric value with the spec-defined label for its kind.
///
/// Each `MetricKind` maps to a specific phrasing rather than the
/// generic "N found" used before this fix.
pub fn metric_label(kind: &MetricKind, value: usize) -> String {
    match kind {
        MetricKind::PackagesFound => format!("{value} found"),
        MetricKind::ReposMapped => {
            if value == 1 {
                "1 repo mapped".to_string()
            } else {
                format!("{value} repos mapped")
            }
        }
        MetricKind::ConfigsModified => format!("{value} modified"),
        MetricKind::UnitsFound => {
            if value == 1 {
                "1 unit".to_string()
            } else {
                format!("{value} units")
            }
        }
        MetricKind::ContainersFound => format!("{value} found"),
        MetricKind::TimersFound => {
            if value == 1 {
                "1 timer".to_string()
            } else {
                format!("{value} timers")
            }
        }
    }
}

/// Minimum elapsed seconds before showing a timer on completion lines.
pub const TIMER_THRESHOLD_SECS: f64 = 3.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_order_has_11_entries() {
        assert_eq!(DISPLAY_ORDER.len(), 11);
    }

    #[test]
    fn display_position_known() {
        assert_eq!(display_position(InspectorId::Rpm), 1);
        assert_eq!(display_position(InspectorId::NonRpmSoftware), 11);
    }

    #[test]
    fn display_position_unknown() {
        assert_eq!(display_position(InspectorId::Hardware), 0);
    }

    #[test]
    fn display_name_known() {
        assert_eq!(display_name(InspectorId::Config), "Config files");
    }

    #[test]
    fn display_name_unknown() {
        assert_eq!(display_name(InspectorId::Ostree), "Unknown");
    }

    #[test]
    fn step_name_coverage() {
        // Ensure every StepId maps to a non-empty string.
        let steps = [
            StepId::QueryingPackages,
            StepId::ClassifyingPackages,
            StepId::ResolvingSourceRepos,
            StepId::ResolvingDepTree,
            StepId::VerifyingIntegrity,
            StepId::MappingFileOwnership,
            StepId::ApplyingRpmVerification,
            StepId::WalkingFilesystem,
            StepId::ClassifyingConfigs,
        ];
        for s in &steps {
            assert!(!step_name(s).is_empty());
        }
    }

    #[test]
    fn probe_name_coverage() {
        let probes = [
            ProbeId::ElfBinaries,
            ProbeId::PythonVenvs,
            ProbeId::PipPackages,
            ProbeId::NpmPackages,
            ProbeId::GemPackages,
            ProbeId::EnvFiles,
            ProbeId::GitRepos,
        ];
        for p in &probes {
            assert!(!probe_name(p).is_empty());
        }
    }

    #[test]
    fn metric_label_specific_wording() {
        assert_eq!(metric_label(&MetricKind::PackagesFound, 847), "847 found");
        assert_eq!(metric_label(&MetricKind::ReposMapped, 8), "8 repos mapped");
        assert_eq!(metric_label(&MetricKind::ReposMapped, 1), "1 repo mapped");
        assert_eq!(
            metric_label(&MetricKind::ConfigsModified, 12),
            "12 modified"
        );
        assert_eq!(metric_label(&MetricKind::UnitsFound, 4), "4 units");
        assert_eq!(metric_label(&MetricKind::UnitsFound, 1), "1 unit");
        assert_eq!(metric_label(&MetricKind::ContainersFound, 3), "3 found");
        assert_eq!(metric_label(&MetricKind::TimersFound, 2), "2 timers");
        assert_eq!(metric_label(&MetricKind::TimersFound, 1), "1 timer");
    }
}
