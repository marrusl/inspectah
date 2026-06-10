//! Shared data model for scan receipt output.
//!
//! Both `PrettyRenderer` and `FlatRenderer` consume these types
//! so their output cannot drift.

use std::path::PathBuf;
use std::time::Duration;

use inspectah_core::types::completeness::InspectorId;

/// Inspector completion state for receipt rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectorState {
    Success,
    Degraded,
    Skipped,
    Failed,
    Interrupted,
}

/// One inspector's receipt line.
#[derive(Debug, Clone)]
pub struct ReceiptLine {
    pub id: InspectorId,
    pub state: InspectorState,
    /// Summary metric (e.g., "613 packages, 6 repos"). None -> "done".
    pub metric: Option<String>,
    /// Reason string for non-success states.
    pub reason: Option<String>,
    /// Child lines (Non-RPM breakdown, verbose sub-steps). Plain text, no symbols.
    pub sub_lines: Vec<String>,
    /// Authoritative typed counts for summary aggregation.
    /// Renderers use `metric` for display; `ScanSummary::build()` uses
    /// these counts to construct hotspot lines without parsing strings.
    pub typed_counts: TypedCounts,
}

/// Authoritative numeric counts per inspector -- source of truth for
/// summary aggregation. Populated from `MetricKind` events during
/// collection, not parsed from the formatted `metric` string.
#[derive(Debug, Clone, Default)]
pub struct TypedCounts {
    pub configs_modified: Option<usize>,
    pub pip_packages: Option<usize>,
    pub npm_packages: Option<usize>,
    pub gem_packages: Option<usize>,
    pub git_repos: Option<usize>,
    // Extensible: add fields as new inspectors emit counts
}

/// Version change summary -- typed, not stringly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionChangeSummary {
    pub total: usize,
    pub target_newer: usize,
    pub host_newer: usize,
}

/// One hotspot line in the findings summary.
#[derive(Debug, Clone)]
pub struct HotspotLine {
    pub segments: Vec<HotspotSegment>,
}

/// A single segment within a hotspot line (typed, not a raw string).
#[derive(Debug, Clone)]
pub struct HotspotSegment {
    pub count: usize,
    pub label: &'static str, // e.g., "modified configs", "pip packages"
}

impl HotspotSegment {
    pub fn format(&self) -> String {
        format!("{} {}", self.count, self.label)
    }
}

impl HotspotLine {
    /// Format segments joined by " . " (pretty) or ", " (flat).
    pub fn format_pretty(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.format())
            .collect::<Vec<_>>()
            .join(" \u{00b7} ")
    }

    pub fn format_flat(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.format())
            .collect::<Vec<_>>()
            .join(", ")
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

/// Non-success tally for the timing line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NonSuccessTally {
    pub failed: usize,
    pub degraded: usize,
    pub skipped: usize,
    pub interrupted: usize,
}

impl NonSuccessTally {
    pub fn is_empty(&self) -> bool {
        self.failed == 0 && self.degraded == 0 && self.skipped == 0 && self.interrupted == 0
    }

    /// Format as parenthetical: "(1 failed, 2 degraded)"
    pub fn format(&self) -> String {
        let mut parts = Vec::new();
        if self.failed > 0 {
            parts.push(format!("{} failed", self.failed));
        }
        if self.degraded > 0 {
            parts.push(format!("{} degraded", self.degraded));
        }
        if self.skipped > 0 {
            parts.push(format!("{} skipped", self.skipped));
        }
        if self.interrupted > 0 {
            parts.push(format!("{} interrupted", self.interrupted));
        }
        format!("({})", parts.join(", "))
    }
}

/// Scan end state -- mutually exclusive outcomes.
/// Illegal combinations (e.g., interrupted with a report path) are
/// unrepresentable.
#[derive(Debug, Clone)]
pub enum ScanEndState {
    /// Normal scan: tarball written to disk.
    Completed {
        path: PathBuf,
        sensitivity: Option<String>,
    },
    /// `--inspect-only` with explicit `--output <path>`.
    InspectOnly { path: PathBuf },
    /// `--inspect-only` without `--output` -- JSON went to stdout.
    InspectOnlyStdout,
    /// Tarball or inspect-only write failed.
    WriteFailure { error: String },
    /// SIGINT interrupted the scan before all inspectors finished.
    Interrupted { completed: usize, total: usize },
}

/// Wrapper passed to `finalize()` -- shared fields + end state.
#[derive(Debug, Clone)]
pub struct ScanFinalize {
    pub elapsed: Duration,
    pub end_state: ScanEndState,
    pub version_changes: Option<VersionChangeSummary>,
}

/// Aggregate scan summary -- computed from receipt lines.
#[derive(Debug, Clone)]
pub struct ScanSummary {
    pub version_changes: Option<VersionChangeSummary>,
    pub hotspots: Vec<HotspotLine>,
    pub non_success_tally: NonSuccessTally,
}

impl ScanSummary {
    /// Build summary from receipt lines and version change data.
    pub fn build(lines: &[ReceiptLine], version_changes: Option<VersionChangeSummary>) -> Self {
        let mut tally = NonSuccessTally::default();
        for line in lines {
            match line.state {
                InspectorState::Failed => tally.failed += 1,
                InspectorState::Degraded => tally.degraded += 1,
                InspectorState::Skipped => tally.skipped += 1,
                InspectorState::Interrupted => tally.interrupted += 1,
                InspectorState::Success => {}
            }
        }

        // Aggregate across ALL lines first, then emit in fixed order.
        // This makes hotspot output deterministic regardless of inspector
        // arrival order.
        let mut total_configs = 0usize;
        let mut total_pip = 0usize;
        let mut total_npm = 0usize;
        let mut total_gem = 0usize;
        let mut total_git = 0usize;

        for line in lines {
            let tc = &line.typed_counts;
            if let Some(c) = tc.configs_modified {
                total_configs += c;
            }
            if let Some(c) = tc.pip_packages {
                total_pip += c;
            }
            if let Some(c) = tc.npm_packages {
                total_npm += c;
            }
            if let Some(c) = tc.gem_packages {
                total_gem += c;
            }
            if let Some(c) = tc.git_repos {
                total_git += c;
            }
        }

        // Emit in fixed order: configs → pip → npm → gem → git.
        let mut hotspot_segments = Vec::new();
        if total_configs > 0 {
            hotspot_segments.push(HotspotSegment {
                count: total_configs,
                label: "modified configs",
            });
        }
        if total_pip > 0 {
            hotspot_segments.push(HotspotSegment {
                count: total_pip,
                label: "pip packages",
            });
        }
        if total_npm > 0 {
            hotspot_segments.push(HotspotSegment {
                count: total_npm,
                label: "npm packages",
            });
        }
        if total_gem > 0 {
            hotspot_segments.push(HotspotSegment {
                count: total_gem,
                label: "gem packages",
            });
        }
        if total_git > 0 {
            hotspot_segments.push(HotspotSegment {
                count: total_git,
                label: "git repos",
            });
        }

        let hotspots = if hotspot_segments.is_empty() {
            Vec::new()
        } else {
            vec![HotspotLine {
                segments: hotspot_segments,
            }]
        };

        Self {
            version_changes,
            hotspots,
            non_success_tally: tally,
        }
    }

    pub fn has_content(&self) -> bool {
        self.version_changes.is_some() || !self.hotspots.is_empty()
    }
}

impl InspectorState {
    /// Pretty-mode symbol.
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Success => "\u{2713}",
            Self::Degraded => "\u{26a0}",
            Self::Skipped => "\u{25cb}",
            Self::Failed => "\u{2717}",
            Self::Interrupted => "\u{25b3}",
        }
    }

    /// Flat-mode text equivalent.
    pub fn flat_label(&self) -> &'static str {
        match self {
            Self::Success => "ok",
            Self::Degraded => "WARN",
            Self::Skipped => "skip",
            Self::Failed => "FAIL",
            Self::Interrupted => "INT",
        }
    }

    /// ANSI color code.
    pub fn color_code(&self) -> &'static str {
        match self {
            Self::Success => "\x1b[32m",
            Self::Degraded => "\x1b[33m",
            Self::Skipped => "\x1b[2m",
            Self::Failed => "\x1b[31m",
            Self::Interrupted => "\x1b[33m",
        }
    }
}

impl VersionChangeSummary {
    pub fn format(&self) -> String {
        format!(
            "{} version changes ({} target-newer, {} host-newer)",
            self.total, self.target_newer, self.host_newer
        )
    }
}

// No string-parsing helpers needed -- TypedCounts provides authoritative data.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tally_is_empty() {
        assert!(NonSuccessTally::default().is_empty());
    }

    #[test]
    fn tally_format_single() {
        let t = NonSuccessTally {
            failed: 1,
            ..Default::default()
        };
        assert_eq!(t.format(), "(1 failed)");
    }

    #[test]
    fn tally_format_multiple_preserves_order() {
        let t = NonSuccessTally {
            failed: 1,
            degraded: 2,
            skipped: 0,
            interrupted: 3,
        };
        assert_eq!(t.format(), "(1 failed, 2 degraded, 3 interrupted)");
    }

    #[test]
    fn state_symbols() {
        assert_eq!(InspectorState::Success.symbol(), "\u{2713}");
        assert_eq!(InspectorState::Failed.symbol(), "\u{2717}");
        assert_eq!(InspectorState::Interrupted.symbol(), "\u{25b3}");
    }

    #[test]
    fn state_flat_labels() {
        assert_eq!(InspectorState::Success.flat_label(), "ok");
        assert_eq!(InspectorState::Failed.flat_label(), "FAIL");
        assert_eq!(InspectorState::Interrupted.flat_label(), "INT");
    }

    #[test]
    fn version_change_summary_format() {
        let v = VersionChangeSummary {
            total: 58,
            target_newer: 54,
            host_newer: 4,
        };
        assert_eq!(
            v.format(),
            "58 version changes (54 target-newer, 4 host-newer)"
        );
    }

    #[test]
    fn hotspot_line_pretty_format() {
        let line = HotspotLine {
            segments: vec![
                HotspotSegment {
                    count: 37,
                    label: "modified configs",
                },
                HotspotSegment {
                    count: 23,
                    label: "pip packages",
                },
            ],
        };
        assert_eq!(
            line.format_pretty(),
            "37 modified configs \u{00b7} 23 pip packages"
        );
    }

    #[test]
    fn hotspot_line_flat_format() {
        let line = HotspotLine {
            segments: vec![
                HotspotSegment {
                    count: 37,
                    label: "modified configs",
                },
                HotspotSegment {
                    count: 23,
                    label: "pip packages",
                },
            ],
        };
        assert_eq!(line.format_flat(), "37 modified configs, 23 pip packages");
    }

    #[test]
    fn summary_omits_zero_version_changes() {
        let summary = ScanSummary::build(&[], None);
        assert!(summary.version_changes.is_none());
        assert!(!summary.has_content());
    }

    #[test]
    fn summary_counts_non_success() {
        let lines = vec![
            ReceiptLine {
                id: InspectorId::Rpm,
                state: InspectorState::Success,
                metric: None,
                reason: None,
                sub_lines: vec![],
                typed_counts: TypedCounts::default(),
            },
            ReceiptLine {
                id: InspectorId::Containers,
                state: InspectorState::Failed,
                metric: None,
                reason: Some("podman not found".into()),
                sub_lines: vec![],
                typed_counts: TypedCounts::default(),
            },
            ReceiptLine {
                id: InspectorId::Config,
                state: InspectorState::Degraded,
                metric: Some("37 modified".into()),
                reason: Some("rpm verify timed out".into()),
                sub_lines: vec![],
                typed_counts: TypedCounts::default(),
            },
        ];
        let summary = ScanSummary::build(&lines, None);
        assert_eq!(summary.non_success_tally.failed, 1);
        assert_eq!(summary.non_success_tally.degraded, 1);
    }

    #[test]
    fn summary_uses_typed_counts_not_strings() {
        let lines = vec![
            ReceiptLine {
                id: InspectorId::Config,
                state: InspectorState::Success,
                metric: Some("37 modified".into()),
                reason: None,
                sub_lines: vec![],
                typed_counts: TypedCounts {
                    configs_modified: Some(37),
                    ..Default::default()
                },
            },
            ReceiptLine {
                id: InspectorId::NonRpmSoftware,
                state: InspectorState::Success,
                metric: Some("2 ecosystems".into()),
                reason: None,
                sub_lines: vec!["pip 23 \u{00b7} npm 69".into()],
                typed_counts: TypedCounts {
                    pip_packages: Some(23),
                    npm_packages: Some(69),
                    ..Default::default()
                },
            },
        ];
        let summary = ScanSummary::build(&lines, None);
        assert_eq!(summary.hotspots.len(), 1);
        assert_eq!(summary.hotspots[0].segments.len(), 3);
        assert_eq!(summary.hotspots[0].segments[0].count, 37);
        assert_eq!(summary.hotspots[0].segments[1].count, 23);
        assert_eq!(summary.hotspots[0].segments[2].count, 69);
    }

    #[test]
    fn summary_deterministic_order_regardless_of_line_order() {
        // Even when NonRpmSoftware arrives before Config, the hotspot
        // order is always: configs → pip → npm → gem → git.
        let lines_nonrpm_first = vec![
            ReceiptLine {
                id: InspectorId::NonRpmSoftware,
                state: InspectorState::Success,
                metric: Some("2 ecosystems".into()),
                reason: None,
                sub_lines: vec![],
                typed_counts: TypedCounts {
                    pip_packages: Some(23),
                    npm_packages: Some(69),
                    ..Default::default()
                },
            },
            ReceiptLine {
                id: InspectorId::Config,
                state: InspectorState::Success,
                metric: Some("37 modified".into()),
                reason: None,
                sub_lines: vec![],
                typed_counts: TypedCounts {
                    configs_modified: Some(37),
                    ..Default::default()
                },
            },
        ];
        let lines_config_first = vec![
            ReceiptLine {
                id: InspectorId::Config,
                state: InspectorState::Success,
                metric: Some("37 modified".into()),
                reason: None,
                sub_lines: vec![],
                typed_counts: TypedCounts {
                    configs_modified: Some(37),
                    ..Default::default()
                },
            },
            ReceiptLine {
                id: InspectorId::NonRpmSoftware,
                state: InspectorState::Success,
                metric: Some("2 ecosystems".into()),
                reason: None,
                sub_lines: vec![],
                typed_counts: TypedCounts {
                    pip_packages: Some(23),
                    npm_packages: Some(69),
                    ..Default::default()
                },
            },
        ];

        let summary_a = ScanSummary::build(&lines_nonrpm_first, None);
        let summary_b = ScanSummary::build(&lines_config_first, None);

        // Both should produce identical hotspot segments in the same order.
        assert_eq!(summary_a.hotspots.len(), 1);
        assert_eq!(summary_b.hotspots.len(), 1);
        let segs_a = &summary_a.hotspots[0].segments;
        let segs_b = &summary_b.hotspots[0].segments;
        assert_eq!(segs_a.len(), segs_b.len());
        for (a, b) in segs_a.iter().zip(segs_b.iter()) {
            assert_eq!(a.count, b.count);
            assert_eq!(a.label, b.label);
        }
        // Verify fixed order: configs first.
        assert_eq!(segs_a[0].label, "modified configs");
        assert_eq!(segs_a[1].label, "pip packages");
        assert_eq!(segs_a[2].label, "npm packages");
    }

    #[test]
    fn scan_end_state_exhaustive() {
        // Compile-time proof: all variants are matchable.
        let states = vec![
            ScanEndState::Completed {
                path: PathBuf::from("/tmp/test"),
                sensitivity: None,
            },
            ScanEndState::InspectOnly {
                path: PathBuf::from("/tmp/test"),
            },
            ScanEndState::InspectOnlyStdout,
            ScanEndState::WriteFailure {
                error: "disk full".into(),
            },
            ScanEndState::Interrupted {
                completed: 5,
                total: 11,
            },
        ];
        for state in &states {
            match state {
                ScanEndState::Completed { .. } => {}
                ScanEndState::InspectOnly { .. } => {}
                ScanEndState::InspectOnlyStdout => {}
                ScanEndState::WriteFailure { .. } => {}
                ScanEndState::Interrupted { .. } => {}
            }
        }
    }
}
