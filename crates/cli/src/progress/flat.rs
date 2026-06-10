//! Flat (non-TTY) progress renderer.
//!
//! Writes sequential numbered lines to an arbitrary [`Write`] sink.
//! No ANSI escapes, no cursor manipulation, no color — safe for piped
//! output, CI logs, and `$TERM=dumb` environments.
//!
//! Uses the shared [`receipt`] data model so output cannot drift from
//! [`PrettyRenderer`].

use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;

use inspectah_core::types::completeness::InspectorId;
use inspectah_core::types::progress::{
    InspectorOutcome, MetricKind, ProbeOutcome, ProgressEvent, StepOutcome,
};

use super::display;
use super::receipt::{
    InspectorState, ReceiptLine, ScanEndState, ScanFinalize, ScanSummary, TypedCounts,
};

/// Flat-mode progress renderer for non-TTY output.
///
/// Thread-safe via internal [`Mutex`] — progress events may arrive
/// from parallel inspector threads (wave-2 concurrency).
pub struct FlatRenderer {
    inner: Mutex<FlatState>,
}

/// Per-inspector tracking during scan.
struct InspectorTracker {
    /// All metrics received.
    metrics: Vec<(MetricKind, usize)>,
    /// Probes with results (Non-RPM ecosystem counting).
    probe_results: Vec<(String, usize)>,
    /// Total probes started (for "none found" detection).
    probes_started: usize,
    /// Verbose-mode sub-step counter (resets per inspector).
    sub_step_count: usize,
}

impl InspectorTracker {
    fn new() -> Self {
        Self {
            metrics: Vec::new(),
            probe_results: Vec::new(),
            probes_started: 0,
            sub_step_count: 0,
        }
    }

    /// Get a metric value by kind.
    fn metric(&self, kind: &MetricKind) -> Option<usize> {
        self.metrics
            .iter()
            .find(|(k, _)| k == kind)
            .map(|(_, v)| *v)
    }

    /// Insert or update a metric.
    fn set_metric(&mut self, kind: MetricKind, value: usize) {
        if let Some(entry) = self.metrics.iter_mut().find(|(k, _)| *k == kind) {
            entry.1 = value;
        } else {
            self.metrics.push((kind, value));
        }
    }
}

struct FlatState {
    writer: Box<dyn Write + Send>,
    verbose: bool,
    /// Display order slice — used for total count and name lookup only.
    display_order: &'static [(InspectorId, &'static str)],
    /// Per-inspector tracking.
    trackers: HashMap<InspectorId, InspectorTracker>,
    /// Arrival-order completion counter (1-based, increments per finish).
    completion_count: usize,
    /// Built receipt lines, stored for finalize().
    receipt_lines: Vec<ReceiptLine>,
}

impl FlatRenderer {
    /// Create a new flat renderer writing to `writer`.
    ///
    /// `display_order` is `active_display_order(has_subscription)` —
    /// used for total count and name lookup, not for output ordering.
    pub fn new(
        writer: Box<dyn Write + Send>,
        verbose: bool,
        display_order: &'static [(InspectorId, &'static str)],
    ) -> Self {
        Self {
            inner: Mutex::new(FlatState {
                writer,
                verbose,
                display_order,
                trackers: HashMap::new(),
                completion_count: 0,
                receipt_lines: Vec::new(),
            }),
        }
    }

    /// Handle a progress event, writing output to the underlying writer.
    pub fn handle(&self, event: ProgressEvent) {
        let mut state = self.inner.lock().expect("FlatRenderer lock poisoned");
        match event {
            ProgressEvent::InspectorStarted(id) => {
                state.trackers.insert(id, InspectorTracker::new());
            }
            ProgressEvent::InspectorFinished { id, outcome } => {
                state.completion_count += 1;
                let n = state.completion_count;
                let total = state.display_order.len();

                let line = state.build_receipt_line(id, &outcome);

                // Print the receipt line.
                let name = lookup_name(state.display_order, id);
                let label = line.state.flat_label();
                let suffix = format_flat_suffix(&line);
                let _ = writeln!(
                    state.writer,
                    "[{n:02}/{total:02}] {name}... {label}{suffix}"
                );

                // In verbose mode, print buffered sub-step lines.
                if state.verbose
                    && let Some(tracker) = state.trackers.get(&id)
                {
                    // Clone probe results to release the borrow on state
                    // before writing (which needs &mut state.writer).
                    let probes: Vec<_> = tracker.probe_results.clone();
                    for (probe_idx, (probe_name, count)) in probes.iter().enumerate() {
                        let s = probe_idx + 1;
                        let _ = writeln!(
                            state.writer,
                            "  [{n:02}/{total:02}.{s}] {probe_name}... {count} found"
                        );
                    }
                }

                state.receipt_lines.push(line);
                state.trackers.remove(&id);
            }
            ProgressEvent::StepStarted { .. } => {
                // Flat mode: no output on step start.
            }
            ProgressEvent::StepFinished {
                inspector,
                step,
                outcome,
            } => {
                if state.verbose {
                    // Extract total before borrowing trackers mutably.
                    let total = state.display_order.len();
                    if let Some(tracker) = state.trackers.get_mut(&inspector) {
                        tracker.sub_step_count += 1;
                        let s = tracker.sub_step_count;
                        let name = display::step_name(&step);
                        let result = format_step_result(&outcome, tracker);
                        let _ = writeln!(state.writer, "  [??/{total:02}.{s}] {name}... {result}");
                    }
                }
            }
            ProgressEvent::Metric {
                inspector,
                kind,
                value,
            } => {
                if let Some(tracker) = state.trackers.get_mut(&inspector) {
                    tracker.set_metric(kind, value);
                }
            }
            ProgressEvent::ProbeStarted { inspector, .. } => {
                if let Some(tracker) = state.trackers.get_mut(&inspector) {
                    tracker.probes_started += 1;
                }
            }
            ProgressEvent::ProbeFinished {
                inspector,
                probe,
                outcome,
            } => {
                if let Some(tracker) = state.trackers.get_mut(&inspector)
                    && let ProbeOutcome::Found { count } = outcome
                {
                    let name = display::probe_name(&probe);
                    tracker.probe_results.push((name.to_string(), count));
                }
            }
        }
    }

    /// Finalize rendering — summary block + typed footer.
    pub fn finalize(&self, scan: &ScanFinalize) {
        let mut state = self.inner.lock().expect("finalize lock");

        // Build summary from collected receipt lines.
        let summary = ScanSummary::build(&state.receipt_lines, scan.version_changes.clone());

        // Print summary block if there's anything to show.
        if summary.has_content() {
            let _ = writeln!(state.writer);
            if let Some(ref vc) = summary.version_changes {
                let _ = writeln!(state.writer, "  {}", vc.format());
            }
            for hotspot in &summary.hotspots {
                let _ = writeln!(state.writer, "  {}", hotspot.format_flat());
            }
        }

        // Footer zone.
        let _ = writeln!(state.writer);
        let secs = scan.elapsed.as_secs_f64();

        match &scan.end_state {
            ScanEndState::Completed { path, sensitivity } => {
                let tally = if summary.non_success_tally.is_empty() {
                    String::new()
                } else {
                    format!(" {}", summary.non_success_tally.format())
                };
                let _ = writeln!(state.writer, "  Inspected in {secs:.1}s{tally}");
                let _ = writeln!(state.writer, "  Report: {}", path.display());
                let _ = writeln!(
                    state.writer,
                    "  To review: inspectah refine {}",
                    path.display()
                );
                if let Some(notice) = sensitivity {
                    for line in notice.lines() {
                        let _ = writeln!(state.writer, "  {line}");
                    }
                }
            }
            ScanEndState::InspectOnly { path } => {
                let tally = if summary.non_success_tally.is_empty() {
                    String::new()
                } else {
                    format!(" {}", summary.non_success_tally.format())
                };
                let _ = writeln!(state.writer, "  Inspected in {secs:.1}s{tally}");
                let _ = writeln!(state.writer, "  Output: {}", path.display());
            }
            ScanEndState::InspectOnlyStdout => {
                let tally = if summary.non_success_tally.is_empty() {
                    String::new()
                } else {
                    format!(" {}", summary.non_success_tally.format())
                };
                let _ = writeln!(state.writer, "  Inspected in {secs:.1}s{tally}");
            }
            ScanEndState::WriteFailure { error } => {
                let tally = if summary.non_success_tally.is_empty() {
                    String::new()
                } else {
                    format!(" {}", summary.non_success_tally.format())
                };
                let _ = writeln!(state.writer, "  Inspected in {secs:.1}s{tally}");
                let _ = writeln!(state.writer, "  Error: {error}");
            }
            ScanEndState::Interrupted { completed, total } => {
                let _ = writeln!(
                    state.writer,
                    "  Interrupted after {secs:.1}s ({completed} of {total} inspectors completed)"
                );
            }
        }
    }

    /// Access built receipt lines (for summary computation).
    pub fn receipt_lines(&self) -> Vec<ReceiptLine> {
        let state = self.inner.lock().expect("FlatRenderer lock poisoned");
        state.receipt_lines.clone()
    }
}

// ── State helpers ──────────────────────────────────────────────────

impl FlatState {
    /// Build a `ReceiptLine` from the accumulated tracker state.
    fn build_receipt_line(&self, id: InspectorId, outcome: &InspectorOutcome) -> ReceiptLine {
        let tracker = self.trackers.get(&id);

        let inspector_state = match outcome {
            InspectorOutcome::Complete => InspectorState::Success,
            InspectorOutcome::Degraded { .. } => InspectorState::Degraded,
            InspectorOutcome::Skipped { .. } => InspectorState::Skipped,
            InspectorOutcome::Failed { .. } => InspectorState::Failed,
            InspectorOutcome::Interrupted => InspectorState::Interrupted,
        };

        let reason = match outcome {
            InspectorOutcome::Degraded { reason } => Some(reason.clone()),
            InspectorOutcome::Skipped { reason } => Some(reason.clone()),
            InspectorOutcome::Failed { reason } => Some(reason.clone()),
            _ => None,
        };

        // Build typed counts from metrics.
        let typed_counts = tracker
            .map(|t| build_typed_counts(t, id))
            .unwrap_or_default();

        // Build metric string.
        let metric = tracker.and_then(|t| build_metric_string(t, id));

        // Build sub_lines.
        let sub_lines = tracker.map(|t| build_sub_lines(t, id)).unwrap_or_default();

        ReceiptLine {
            id,
            state: inspector_state,
            metric,
            reason,
            sub_lines,
            typed_counts,
        }
    }
}

// ── Formatting helpers ─────────────────────────────────────────────

/// Look up display name from the display-order slice.
fn lookup_name(
    display_order: &'static [(InspectorId, &'static str)],
    id: InspectorId,
) -> &'static str {
    display_order
        .iter()
        .find(|(oid, _)| *oid == id)
        .map(|(_, name)| *name)
        .unwrap_or("Unknown")
}

/// Format the right-hand suffix for a flat receipt line.
///
/// Returns the part after the status label: ` (metric)` and/or ` — reason`.
fn format_flat_suffix(line: &ReceiptLine) -> String {
    let mut parts = String::new();

    // Metric in parentheses.
    if let Some(ref metric) = line.metric {
        parts.push_str(&format!(" ({metric})"));
    } else if line.state == InspectorState::Success {
        parts.push_str(" (done)");
    }

    // Non-success reason after em dash.
    if let Some(ref reason) = line.reason {
        parts.push_str(&format!(" \u{2014} {reason}"));
    }

    parts
}

/// Build the metric display string from tracker state.
fn build_metric_string(tracker: &InspectorTracker, id: InspectorId) -> Option<String> {
    // Non-RPM inspector uses probe-based ecosystem counting.
    if id == InspectorId::NonRpmSoftware {
        let found_count = tracker.probe_results.len();
        if tracker.probes_started > 0 && found_count == 0 {
            return Some("none found".to_string());
        }
        if found_count == 1 {
            return Some("1 ecosystem".to_string());
        }
        if found_count > 1 {
            return Some(format!("{found_count} ecosystems"));
        }
        return None;
    }

    // RPM inspector combines PackagesFound + ReposMapped.
    if id == InspectorId::Rpm {
        let mut parts = Vec::new();
        if let Some(count) = tracker.metric(&MetricKind::PackagesFound) {
            parts.push(format!("{count} packages"));
        }
        if let Some(count) = tracker.metric(&MetricKind::ReposMapped) {
            if count == 1 {
                parts.push("1 repo".to_string());
            } else {
                parts.push(format!("{count} repos"));
            }
        }
        if parts.is_empty() {
            return None;
        }
        return Some(parts.join(", "));
    }

    // Other inspectors: use the last metric with spec label.
    for kind in &[
        MetricKind::ConfigsModified,
        MetricKind::UnitsFound,
        MetricKind::ContainersFound,
        MetricKind::TimersFound,
        MetricKind::PackagesFound,
        MetricKind::ReposMapped,
    ] {
        if let Some(value) = tracker.metric(kind) {
            return Some(display::metric_label(kind, value));
        }
    }

    None
}

/// Build sub_lines for Non-RPM ecosystem breakdown.
fn build_sub_lines(tracker: &InspectorTracker, id: InspectorId) -> Vec<String> {
    if id == InspectorId::NonRpmSoftware && !tracker.probe_results.is_empty() {
        let parts: Vec<String> = tracker
            .probe_results
            .iter()
            .map(|(name, count)| format!("{name} {count}"))
            .collect();
        return vec![parts.join(", ")];
    }
    Vec::new()
}

/// Build TypedCounts from tracker metrics and probes.
fn build_typed_counts(tracker: &InspectorTracker, id: InspectorId) -> TypedCounts {
    let mut tc = TypedCounts::default();

    if let Some(v) = tracker.metric(&MetricKind::ConfigsModified) {
        tc.configs_modified = Some(v);
    }

    // Non-RPM probes populate typed counts from probe results.
    if id == InspectorId::NonRpmSoftware {
        for (name, count) in &tracker.probe_results {
            match name.as_str() {
                "pip packages" => tc.pip_packages = Some(*count),
                "npm packages" => tc.npm_packages = Some(*count),
                "gem packages" => tc.gem_packages = Some(*count),
                "git repos" => tc.git_repos = Some(*count),
                _ => {}
            }
        }
    }

    tc
}

/// Format the result portion of a verbose step line.
fn format_step_result(outcome: &StepOutcome, tracker: &InspectorTracker) -> String {
    match outcome {
        StepOutcome::Complete => {
            // Use last metric if available.
            if let Some((kind, value)) = tracker.metrics.last() {
                display::metric_label(kind, *value)
            } else {
                "done".to_string()
            }
        }
        StepOutcome::Degraded { reason } => format!("degraded: {reason}"),
        StepOutcome::Failed { reason } => format!("failed: {reason}"),
        StepOutcome::Skipped { reason } => format!("skipped ({reason})"),
        StepOutcome::Interrupted => "interrupted".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::progress::{ProbeId, StepId};
    use std::sync::Arc;

    /// A `Write` adapter that writes into a shared `Vec<u8>`.
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("test lock").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn output_text(buf: &Arc<Mutex<Vec<u8>>>) -> String {
        String::from_utf8(buf.lock().expect("test lock").clone()).expect("valid utf8")
    }

    /// Create a FlatRenderer backed by a shared buffer.
    fn test_renderer(verbose: bool, has_subscription: bool) -> (FlatRenderer, Arc<Mutex<Vec<u8>>>) {
        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let writer = SharedWriter(Arc::clone(&buf));
        let order = display::active_display_order(has_subscription);
        let renderer = FlatRenderer::new(Box::new(writer), verbose, order);
        (renderer, buf)
    }

    // ── Feed helpers ──────────────────────────────────────────────────

    fn feed_simple_complete(r: &FlatRenderer, id: InspectorId) {
        r.handle(ProgressEvent::InspectorStarted(id));
        r.handle(ProgressEvent::InspectorFinished {
            id,
            outcome: InspectorOutcome::Complete,
        });
    }

    fn feed_rpm(r: &FlatRenderer, packages: usize, repos: usize) {
        r.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        r.handle(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::PackagesFound,
            value: packages,
        });
        r.handle(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::ReposMapped,
            value: repos,
        });
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });
    }

    fn feed_with_metric(r: &FlatRenderer, id: InspectorId, kind: MetricKind, value: usize) {
        r.handle(ProgressEvent::InspectorStarted(id));
        r.handle(ProgressEvent::Metric {
            inspector: id,
            kind,
            value,
        });
        r.handle(ProgressEvent::InspectorFinished {
            id,
            outcome: InspectorOutcome::Complete,
        });
    }

    fn feed_nonrpm_probes(r: &FlatRenderer, probes: &[(&ProbeId, Option<usize>)]) {
        r.handle(ProgressEvent::InspectorStarted(InspectorId::NonRpmSoftware));
        for (probe, result) in probes {
            r.handle(ProgressEvent::ProbeStarted {
                inspector: InspectorId::NonRpmSoftware,
                probe: (*probe).clone(),
            });
            let outcome = match result {
                Some(count) => ProbeOutcome::Found { count: *count },
                None => ProbeOutcome::Empty,
            };
            r.handle(ProgressEvent::ProbeFinished {
                inspector: InspectorId::NonRpmSoftware,
                probe: (*probe).clone(),
                outcome,
            });
        }
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::NonRpmSoftware,
            outcome: InspectorOutcome::Complete,
        });
    }

    /// Capture only the finalize output (summary + footer).
    fn finalize_output(r: &FlatRenderer, buf: &Arc<Mutex<Vec<u8>>>, scan: &ScanFinalize) -> String {
        let pre_len = buf.lock().expect("test lock").len();
        r.finalize(scan);
        let full = buf.lock().expect("test lock").clone();
        String::from_utf8(full[pre_len..].to_vec()).expect("valid utf8")
    }

    // ── Tests ─────────────────────────────────────────────────────────

    #[test]
    fn flat_normal_hides_substeps() {
        // Default verbosity — sub-steps and probes suppressed.
        let (r, buf) = test_renderer(false, false);

        r.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        r.handle(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
        });
        r.handle(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::PackagesFound,
            value: 613,
        });
        r.handle(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
            outcome: StepOutcome::Complete,
        });
        r.handle(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::ResolvingSourceRepos,
        });
        r.handle(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::ReposMapped,
            value: 6,
        });
        r.handle(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::ResolvingSourceRepos,
            outcome: StepOutcome::Complete,
        });
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });

        let text = output_text(&buf);
        // Should only have the parent line, no sub-step lines.
        assert_eq!(text.lines().count(), 1, "expected 1 line, got: {text}");
        assert!(
            text.contains("[01/11] RPM packages... ok (613 packages, 6 repos)"),
            "got: {text}"
        );
    }

    #[test]
    fn flat_verbose_shows_substeps() {
        // Verbose mode — sub-steps shown with dot notation.
        let (r, buf) = test_renderer(true, false);

        r.handle(ProgressEvent::InspectorStarted(InspectorId::NonRpmSoftware));
        r.handle(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PipPackages,
        });
        r.handle(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PipPackages,
            outcome: ProbeOutcome::Found { count: 23 },
        });
        r.handle(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::NpmPackages,
        });
        r.handle(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::NpmPackages,
            outcome: ProbeOutcome::Found { count: 69 },
        });
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::NonRpmSoftware,
            outcome: InspectorOutcome::Complete,
        });

        let text = output_text(&buf);
        let lines: Vec<&str> = text.lines().collect();
        // Parent line + 2 probe sub-lines.
        assert_eq!(lines.len(), 3, "expected 3 lines, got: {text}");
        assert!(
            lines[0].contains("[01/11] Non-RPM packages... ok (2 ecosystems)"),
            "parent: {}",
            lines[0]
        );
        assert!(
            lines[1].contains("[01/11.1] pip packages... 23 found"),
            "sub 1: {}",
            lines[1]
        );
        assert!(
            lines[2].contains("[01/11.2] npm packages... 69 found"),
            "sub 2: {}",
            lines[2]
        );
    }

    #[test]
    fn flat_dynamic_count_12() {
        // Subscription enabled → total is 12.
        let (r, buf) = test_renderer(false, true);

        feed_simple_complete(&r, InspectorId::Rpm);
        feed_simple_complete(&r, InspectorId::Subscription);

        let text = output_text(&buf);
        assert!(
            text.contains("[01/12]"),
            "first completion should be [01/12]: {text}"
        );
        assert!(
            text.contains("[02/12]"),
            "second completion should be [02/12]: {text}"
        );
    }

    #[test]
    fn flat_summary_block() {
        // Findings summary with ", " separators (not " · ").
        use std::path::PathBuf;
        use std::time::Duration;

        let (r, buf) = test_renderer(false, false);

        feed_with_metric(&r, InspectorId::Config, MetricKind::ConfigsModified, 37);
        feed_nonrpm_probes(
            &r,
            &[
                (&ProbeId::PipPackages, Some(23)),
                (&ProbeId::NpmPackages, Some(69)),
            ],
        );

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(12.3),
            end_state: ScanEndState::Completed {
                path: PathBuf::from("/tmp/inspectah-scan.tar.gz"),
                sensitivity: None,
            },
            version_changes: Some(super::super::receipt::VersionChangeSummary {
                total: 58,
                target_newer: 54,
                host_newer: 4,
            }),
        };

        let footer = finalize_output(&r, &buf, &scan);
        // Version changes line.
        assert!(
            footer.contains("58 version changes (54 target-newer, 4 host-newer)"),
            "missing version changes: {footer}"
        );
        // Hotspot line with ", " separator (not " · ").
        assert!(
            footer.contains("37 modified configs, 23 pip packages, 69 npm packages"),
            "missing hotspot with comma separators: {footer}"
        );
        // No " · " anywhere.
        assert!(
            !footer.contains(" \u{00b7} "),
            "found pretty-mode separator in flat output: {footer}"
        );
    }

    #[test]
    fn flat_footer_completed() {
        // Timing line + report path + refine hint.
        use std::path::PathBuf;
        use std::time::Duration;

        let (r, buf) = test_renderer(false, false);
        feed_simple_complete(&r, InspectorId::Network);

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(12.3),
            end_state: ScanEndState::Completed {
                path: PathBuf::from("/tmp/inspectah-scan.tar.gz"),
                sensitivity: None,
            },
            version_changes: None,
        };

        let footer = finalize_output(&r, &buf, &scan);
        assert!(
            footer.contains("Inspected in 12.3s"),
            "missing timing: {footer}"
        );
        assert!(
            footer.contains("Report: /tmp/inspectah-scan.tar.gz"),
            "missing report path: {footer}"
        );
        assert!(
            footer.contains("To review: inspectah refine /tmp/inspectah-scan.tar.gz"),
            "missing refine hint: {footer}"
        );
    }

    #[test]
    fn flat_footer_interrupted() {
        // Interrupted timing line.
        use std::time::Duration;

        let (r, buf) = test_renderer(false, false);
        feed_simple_complete(&r, InspectorId::Network);

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(3.7),
            end_state: ScanEndState::Interrupted {
                completed: 5,
                total: 11,
            },
            version_changes: None,
        };

        let footer = finalize_output(&r, &buf, &scan);
        assert!(
            footer.contains("Interrupted after 3.7s (5 of 11 inspectors completed)"),
            "missing interrupted footer: {footer}"
        );
    }

    #[test]
    fn flat_non_success_states() {
        // ok/FAIL/WARN/skip/INT text labels.
        let (r, buf) = test_renderer(false, false);

        // Success
        feed_simple_complete(&r, InspectorId::Rpm);

        // Failed
        r.handle(ProgressEvent::InspectorStarted(InspectorId::Containers));
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Containers,
            outcome: InspectorOutcome::Failed {
                reason: "podman not found".to_string(),
            },
        });

        // Degraded (WARN)
        r.handle(ProgressEvent::InspectorStarted(InspectorId::Config));
        r.handle(ProgressEvent::Metric {
            inspector: InspectorId::Config,
            kind: MetricKind::ConfigsModified,
            value: 37,
        });
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Config,
            outcome: InspectorOutcome::Degraded {
                reason: "rpm verify timed out".to_string(),
            },
        });

        // Skipped
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Selinux,
            outcome: InspectorOutcome::Skipped {
                reason: "disabled".to_string(),
            },
        });

        // Interrupted
        r.handle(ProgressEvent::InspectorStarted(InspectorId::Network));
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Network,
            outcome: InspectorOutcome::Interrupted,
        });

        let text = output_text(&buf);
        assert!(
            text.contains("RPM packages... ok"),
            "missing ok label: {text}"
        );
        assert!(
            text.contains("Containers... FAIL"),
            "missing FAIL label: {text}"
        );
        assert!(
            text.contains("Config files... WARN"),
            "missing WARN label: {text}"
        );
        assert!(
            text.contains("SELinux... skip"),
            "missing skip label: {text}"
        );
        assert!(text.contains("Network... INT"), "missing INT label: {text}");
        // Reason after em dash for non-success.
        assert!(
            text.contains("FAIL \u{2014} podman not found"),
            "missing FAIL reason: {text}"
        );
        assert!(
            text.contains("WARN (37 modified) \u{2014} rpm verify timed out"),
            "missing WARN reason: {text}"
        );
        assert!(
            text.contains("skip \u{2014} disabled"),
            "missing skip reason: {text}"
        );
    }

    #[test]
    fn flat_arrival_order() {
        // Fast inspectors print before slow RPM.
        let (r, buf) = test_renderer(false, false);

        // Services finishes first.
        feed_with_metric(&r, InspectorId::Services, MetricKind::UnitsFound, 27);
        // Network finishes second.
        feed_simple_complete(&r, InspectorId::Network);
        // RPM finishes last.
        feed_rpm(&r, 613, 6);

        let text = output_text(&buf);
        let lines: Vec<&str> = text.lines().collect();

        // Arrival order: Services → Network → RPM.
        assert!(
            lines[0].contains("[01/11]") && lines[0].contains("Services"),
            "first should be Services [01/11]: {}",
            lines[0]
        );
        assert!(
            lines[1].contains("[02/11]") && lines[1].contains("Network"),
            "second should be Network [02/11]: {}",
            lines[1]
        );
        assert!(
            lines[2].contains("[03/11]") && lines[2].contains("RPM"),
            "third should be RPM [03/11]: {}",
            lines[2]
        );
    }

    #[test]
    fn flat_skipped_without_start() {
        // Inapplicable inspector — Skipped without InspectorStarted.
        let (r, buf) = test_renderer(false, false);

        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Selinux,
            outcome: InspectorOutcome::Skipped {
                reason: "disabled".to_string(),
            },
        });

        let text = output_text(&buf);
        assert!(
            text.contains("[01/11] SELinux... skip"),
            "expected skip line: {text}"
        );
        assert!(text.contains("disabled"), "expected reason: {text}");
    }

    #[test]
    fn flat_completion_counter() {
        // [N/total] increments per arrival, not by display position.
        let (r, buf) = test_renderer(false, false);

        // Services finishes first (display position 2), gets counter 1.
        feed_with_metric(&r, InspectorId::Services, MetricKind::UnitsFound, 27);
        // Selinux finishes second (display position 10), gets counter 2.
        feed_simple_complete(&r, InspectorId::Selinux);
        // RPM finishes third (display position 1), gets counter 3.
        feed_rpm(&r, 613, 6);

        let text = output_text(&buf);
        let lines: Vec<&str> = text.lines().collect();

        // Services is [01/11] despite being position 2 in display order.
        assert!(
            lines[0].contains("[01/11]") && lines[0].contains("Services"),
            "Services should be [01/11]: {}",
            lines[0]
        );
        // Selinux is [02/11] despite being position 10.
        assert!(
            lines[1].contains("[02/11]") && lines[1].contains("SELinux"),
            "SELinux should be [02/11]: {}",
            lines[1]
        );
        // RPM is [03/11] despite being position 1.
        assert!(
            lines[2].contains("[03/11]") && lines[2].contains("RPM"),
            "RPM should be [03/11]: {}",
            lines[2]
        );
    }
}
