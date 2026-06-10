//! Pretty receipt renderer — arrival-order output with typed receipt lines.
//!
//! Prints each inspector's result as it finishes (arrival order), not in
//! display order.  In verbose mode, sub-step and probe child lines are
//! buffered per inspector and flushed atomically with the parent line.
//!
//! Uses the shared [`receipt`] data model so output cannot drift from
//! [`FlatRenderer`].

use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;

use inspectah_core::types::completeness::InspectorId;
use inspectah_core::types::progress::{
    InspectorOutcome, MetricKind, ProbeOutcome, ProgressEvent, StepOutcome,
};

use super::display;
use super::receipt::{InspectorState, ReceiptLine, ScanFinalize, TypedCounts};

// ── ANSI helpers ────────────────────────────────────────────────────

const RESET: &str = "\x1b[0m";

/// Wrap `text` in ANSI color codes when color is enabled.
fn colored(text: &str, code: &str, use_color: bool) -> String {
    if use_color {
        format!("{code}{text}{RESET}")
    } else {
        text.to_string()
    }
}

// ── Column widths ──────────────────────────────────────────────────

/// Fixed column width for inspector name (left-aligned).
const NAME_WIDTH: usize = 28;

// ── Per-inspector tracking ─────────────────────────────────────────

/// Tracks state accumulated for a single inspector during the scan.
struct InspectorTracker {
    started_at: Instant,
    /// All metrics received.  RPM sends PackagesFound and ReposMapped;
    /// other inspectors send at most one.  Stored as a vec because
    /// `MetricKind` does not implement `Hash`.
    metrics: Vec<(MetricKind, usize)>,
    /// Probes with results (Non-RPM ecosystem counting).
    probe_results: Vec<(String, usize)>,
    /// Total probes started (for "none found" detection).
    probes_started: usize,
    /// Verbose-mode child lines (sub-steps, probes), buffered for
    /// atomic flush with the parent line.
    child_lines: Vec<String>,
}

impl InspectorTracker {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            metrics: Vec::new(),
            probe_results: Vec::new(),
            probes_started: 0,
            child_lines: Vec::new(),
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

// ── Inner state ────────────────────────────────────────────────────

struct PrettyState {
    writer: Box<dyn Write + Send>,
    use_color: bool,
    verbose: bool,
    /// Per-inspector tracking, keyed by InspectorId.
    trackers: HashMap<InspectorId, InspectorTracker>,
    /// Display order slice — used for total count and name lookup only.
    display_order: &'static [(InspectorId, &'static str)],
    /// Built receipt lines, stored for later use by finalize (T5).
    receipt_lines: Vec<ReceiptLine>,
}

// ── Public API ─────────────────────────────────────────────────────

/// Pretty-mode receipt renderer — arrival-order output with Unicode
/// symbols, optional ANSI color, and typed receipt lines.
///
/// Thread-safe via internal [`Mutex`].
pub struct PrettyRenderer {
    inner: Mutex<PrettyState>,
}

impl PrettyRenderer {
    /// Create a new pretty renderer.
    ///
    /// `display_order` is `active_display_order(has_subscription)` —
    /// used for total count and name lookup, not for output ordering.
    pub fn new(
        writer: Box<dyn Write + Send>,
        use_color: bool,
        verbose: bool,
        display_order: &'static [(InspectorId, &'static str)],
    ) -> Self {
        Self {
            inner: Mutex::new(PrettyState {
                writer,
                use_color,
                verbose,
                trackers: HashMap::new(),
                display_order,
                receipt_lines: Vec::new(),
            }),
        }
    }

    /// Handle a progress event.
    pub fn handle(&self, event: ProgressEvent) {
        let mut state = self.inner.lock().expect("PrettyRenderer lock poisoned");
        match event {
            ProgressEvent::InspectorStarted(id) => {
                state.trackers.insert(id, InspectorTracker::new());
            }
            ProgressEvent::InspectorFinished { id, outcome } => {
                let line = state.build_receipt_line(id, &outcome);
                state.print_receipt_line(&line);
                state.receipt_lines.push(line);
                state.trackers.remove(&id);
            }
            ProgressEvent::StepStarted { inspector, step } => {
                if state.verbose
                    && let Some(tracker) = state.trackers.get_mut(&inspector)
                {
                    let name = display::step_name(&step);
                    tracker.child_lines.push(format!("      \u{25b8} {name}"));
                }
            }
            ProgressEvent::StepFinished {
                inspector,
                step,
                outcome,
            } => {
                if state.verbose {
                    let use_color = state.use_color;
                    if let Some(tracker) = state.trackers.get_mut(&inspector) {
                        let name = display::step_name(&step);
                        // Find the last metric received (most recent).
                        let step_metric = tracker.metrics.last().map(|(k, v)| (k.clone(), *v));
                        let (symbol, suffix) = format_step_line(&outcome, &step_metric, use_color);
                        tracker
                            .child_lines
                            .push(format!("      {symbol} {name:<24} {suffix}"));
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
            ProgressEvent::ProbeStarted { inspector, probe } => {
                let verbose = state.verbose;
                if let Some(tracker) = state.trackers.get_mut(&inspector) {
                    tracker.probes_started += 1;
                    if verbose {
                        let name = display::probe_name(&probe);
                        tracker.child_lines.push(format!("      \u{25b8} {name}"));
                    }
                }
            }
            ProgressEvent::ProbeFinished {
                inspector,
                probe,
                outcome,
            } => {
                let verbose = state.verbose;
                let use_color = state.use_color;
                if let Some(tracker) = state.trackers.get_mut(&inspector) {
                    if let ProbeOutcome::Found { count } = outcome {
                        let name = display::probe_name(&probe);
                        tracker.probe_results.push((name.to_string(), count));
                    }
                    if verbose {
                        let name = display::probe_name(&probe);
                        let (symbol, suffix) = format_probe_line(&outcome, use_color);
                        tracker
                            .child_lines
                            .push(format!("      {symbol} {name:<24} {suffix}"));
                    }
                }
            }
        }
    }

    /// Finalize rendering.  Stub for T3 — T5 adds the real summary/footer logic.
    pub fn finalize(&self, _scan: &ScanFinalize) {
        // T5 will implement summary and footer printing.
    }

    /// Access built receipt lines (for T5 summary computation).
    pub fn receipt_lines(&self) -> Vec<ReceiptLine> {
        let state = self.inner.lock().expect("PrettyRenderer lock poisoned");
        state.receipt_lines.clone()
    }
}

// ── State helpers ──────────────────────────────────────────────────

impl PrettyState {
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

    /// Print a receipt line (and any child lines in verbose mode) to the writer.
    fn print_receipt_line(&mut self, line: &ReceiptLine) {
        let use_color = self.use_color;
        let name = lookup_name(self.display_order, line.id);

        // Build the formatted line.
        let symbol = colored(line.state.symbol(), line.state.color_code(), use_color);
        let suffix = format_suffix(line);
        let _ = writeln!(self.writer, "  {symbol} {name:<NAME_WIDTH$} {suffix}");

        // Print receipt sub_lines (e.g., Non-RPM ecosystem breakdown).
        for sub in &line.sub_lines {
            let _ = writeln!(self.writer, "      {sub}");
        }

        // In verbose mode, print buffered child lines atomically with the parent.
        if self.verbose
            && let Some(tracker) = self.trackers.get(&line.id)
        {
            for child in &tracker.child_lines {
                let _ = writeln!(self.writer, "{child}");
            }
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

/// Format the right-hand suffix for a receipt line.
fn format_suffix(line: &ReceiptLine) -> String {
    match &line.state {
        InspectorState::Success | InspectorState::Degraded => {
            let mut parts = Vec::new();
            if let Some(ref metric) = line.metric {
                parts.push(metric.clone());
            }
            if let Some(ref reason) = line.reason {
                parts.push(format!("({reason})"));
            }
            if parts.is_empty() {
                "done".to_string()
            } else {
                parts.join(" ")
            }
        }
        InspectorState::Skipped => match &line.reason {
            Some(reason) => format!("skipped ({reason})"),
            None => "skipped".to_string(),
        },
        InspectorState::Failed => match &line.reason {
            Some(reason) => format!("failed: {reason}"),
            None => "failed".to_string(),
        },
        InspectorState::Interrupted => "interrupted".to_string(),
    }
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
    // Prioritize the "interesting" metric kinds.
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
        return vec![parts.join(" \u{00b7} ")];
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

/// Format a verbose step child line.
fn format_step_line(
    outcome: &StepOutcome,
    _metric: &Option<(MetricKind, usize)>,
    use_color: bool,
) -> (String, String) {
    match outcome {
        StepOutcome::Complete => {
            let sym = colored("\u{2713}", "\x1b[32m", use_color);
            (sym, "done".to_string())
        }
        StepOutcome::Skipped { reason } => {
            let sym = colored("\u{25cb}", "\x1b[2m", use_color);
            (sym, format!("skipped ({reason})"))
        }
        StepOutcome::Degraded { reason } => {
            let sym = colored("\u{26a0}", "\x1b[33m", use_color);
            (sym, format!("degraded: {reason}"))
        }
        StepOutcome::Failed { reason } => {
            let sym = colored("\u{2717}", "\x1b[31m", use_color);
            (sym, format!("failed: {reason}"))
        }
        StepOutcome::Interrupted => {
            let sym = colored("\u{25b3}", "\x1b[33m", use_color);
            (sym, "interrupted".to_string())
        }
    }
}

/// Format a verbose probe child line.
fn format_probe_line(outcome: &ProbeOutcome, use_color: bool) -> (String, String) {
    match outcome {
        ProbeOutcome::Found { count } => {
            let sym = colored("\u{2713}", "\x1b[32m", use_color);
            (sym, format!("{count} found"))
        }
        ProbeOutcome::Empty => {
            let sym = colored("\u{2013}", "\x1b[2m", use_color);
            (sym, "none".to_string())
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────

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

    /// Create a PrettyRenderer backed by a shared buffer.
    fn test_renderer(
        use_color: bool,
        verbose: bool,
        has_subscription: bool,
    ) -> (PrettyRenderer, Arc<Mutex<Vec<u8>>>) {
        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let writer = SharedWriter(Arc::clone(&buf));
        let order = display::active_display_order(has_subscription);
        let renderer = PrettyRenderer::new(Box::new(writer), use_color, verbose, order);
        (renderer, buf)
    }

    fn output_text(buf: &Arc<Mutex<Vec<u8>>>) -> String {
        String::from_utf8(buf.lock().expect("test lock").clone()).expect("valid utf8")
    }

    // ── Helper: feed a full inspector lifecycle ────────────────────

    /// Feed a simple inspector: started → finished(Complete), no metrics.
    fn feed_simple_complete(r: &PrettyRenderer, id: InspectorId) {
        r.handle(ProgressEvent::InspectorStarted(id));
        r.handle(ProgressEvent::InspectorFinished {
            id,
            outcome: InspectorOutcome::Complete,
        });
    }

    /// Feed RPM with packages + repos metrics.
    fn feed_rpm(r: &PrettyRenderer, packages: usize, repos: usize) {
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

    /// Feed RPM with steps for verbose mode.
    fn feed_rpm_with_steps(r: &PrettyRenderer, packages: usize, repos: usize) {
        r.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        r.handle(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
        });
        r.handle(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::PackagesFound,
            value: packages,
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
            value: repos,
        });
        r.handle(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::ResolvingSourceRepos,
            outcome: StepOutcome::Complete,
        });
        r.handle(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::ClassifyingPackages,
        });
        r.handle(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::ClassifyingPackages,
            outcome: StepOutcome::Complete,
        });
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });
    }

    /// Feed Non-RPM with probes.
    fn feed_nonrpm_probes(r: &PrettyRenderer, probes: &[(&ProbeId, Option<usize>)]) {
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

    /// Feed a metric-bearing inspector.
    fn feed_with_metric(r: &PrettyRenderer, id: InspectorId, kind: MetricKind, value: usize) {
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

    // ── Snapshot tests ─────────────────────────────────────────────

    #[test]
    fn pretty_normal_11_inspectors() {
        let (r, buf) = test_renderer(false, false, false);

        feed_rpm(&r, 613, 6);
        feed_with_metric(&r, InspectorId::Services, MetricKind::UnitsFound, 42);
        feed_simple_complete(&r, InspectorId::Storage);
        feed_simple_complete(&r, InspectorId::KernelBoot);
        feed_simple_complete(&r, InspectorId::Network);
        feed_with_metric(&r, InspectorId::Containers, MetricKind::ContainersFound, 3);
        feed_simple_complete(&r, InspectorId::UsersGroups);
        feed_with_metric(&r, InspectorId::ScheduledTasks, MetricKind::TimersFound, 5);
        feed_with_metric(&r, InspectorId::Config, MetricKind::ConfigsModified, 37);
        feed_simple_complete(&r, InspectorId::Selinux);
        feed_nonrpm_probes(
            &r,
            &[
                (&ProbeId::PipPackages, Some(23)),
                (&ProbeId::NpmPackages, Some(69)),
                (&ProbeId::GemPackages, None),
            ],
        );

        insta::assert_snapshot!(output_text(&buf));
    }

    #[test]
    fn pretty_with_subscription() {
        let (r, buf) = test_renderer(false, false, true);

        feed_rpm(&r, 613, 6);
        feed_with_metric(&r, InspectorId::Services, MetricKind::UnitsFound, 42);
        feed_simple_complete(&r, InspectorId::Storage);
        feed_simple_complete(&r, InspectorId::KernelBoot);
        feed_simple_complete(&r, InspectorId::Network);
        feed_with_metric(&r, InspectorId::Containers, MetricKind::ContainersFound, 3);
        feed_simple_complete(&r, InspectorId::UsersGroups);
        feed_with_metric(&r, InspectorId::ScheduledTasks, MetricKind::TimersFound, 5);
        feed_with_metric(&r, InspectorId::Config, MetricKind::ConfigsModified, 37);
        feed_simple_complete(&r, InspectorId::Selinux);
        feed_nonrpm_probes(
            &r,
            &[
                (&ProbeId::PipPackages, Some(23)),
                (&ProbeId::NpmPackages, Some(69)),
            ],
        );
        feed_simple_complete(&r, InspectorId::Subscription);

        insta::assert_snapshot!(output_text(&buf));
    }

    #[test]
    fn pretty_with_failures() {
        let (r, buf) = test_renderer(false, false, false);

        feed_rpm(&r, 613, 6);
        feed_with_metric(&r, InspectorId::Services, MetricKind::UnitsFound, 42);
        feed_simple_complete(&r, InspectorId::Storage);
        feed_simple_complete(&r, InspectorId::KernelBoot);
        feed_simple_complete(&r, InspectorId::Network);

        // Failed
        r.handle(ProgressEvent::InspectorStarted(InspectorId::Containers));
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Containers,
            outcome: InspectorOutcome::Failed {
                reason: "podman not found".to_string(),
            },
        });

        feed_simple_complete(&r, InspectorId::UsersGroups);
        feed_simple_complete(&r, InspectorId::ScheduledTasks);

        // Degraded
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

        feed_simple_complete(&r, InspectorId::NonRpmSoftware);

        insta::assert_snapshot!(output_text(&buf));
    }

    #[test]
    fn pretty_with_interrupted() {
        let (r, buf) = test_renderer(false, false, false);

        feed_rpm(&r, 613, 6);
        feed_with_metric(&r, InspectorId::Services, MetricKind::UnitsFound, 42);
        feed_simple_complete(&r, InspectorId::Storage);
        feed_simple_complete(&r, InspectorId::KernelBoot);
        feed_simple_complete(&r, InspectorId::Network);

        // Remaining 6 interrupted
        for id in [
            InspectorId::Containers,
            InspectorId::UsersGroups,
            InspectorId::ScheduledTasks,
            InspectorId::Config,
            InspectorId::Selinux,
            InspectorId::NonRpmSoftware,
        ] {
            r.handle(ProgressEvent::InspectorStarted(id));
            r.handle(ProgressEvent::InspectorFinished {
                id,
                outcome: InspectorOutcome::Interrupted,
            });
        }

        insta::assert_snapshot!(output_text(&buf));
    }

    #[test]
    fn pretty_nonrpm_sub_lines() {
        let (r, buf) = test_renderer(false, false, false);

        feed_nonrpm_probes(
            &r,
            &[
                (&ProbeId::PipPackages, Some(23)),
                (&ProbeId::NpmPackages, Some(69)),
                (&ProbeId::GemPackages, Some(5)),
                (&ProbeId::ElfBinaries, None),
                (&ProbeId::GitRepos, Some(12)),
            ],
        );

        let text = output_text(&buf);
        // Verify sub_lines contain ecosystem breakdown.
        assert!(
            text.contains("pip packages 23"),
            "expected pip ecosystem in output: {text}"
        );
        insta::assert_snapshot!(text);
    }

    #[test]
    fn pretty_verbose_rpm_substeps() {
        let (r, buf) = test_renderer(false, true, false);

        feed_rpm_with_steps(&r, 847, 8);

        insta::assert_snapshot!(output_text(&buf));
    }

    #[test]
    fn pretty_no_color() {
        let (r, buf) = test_renderer(false, false, false);

        feed_rpm(&r, 613, 6);

        // Skipped
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Selinux,
            outcome: InspectorOutcome::Skipped {
                reason: "disabled".to_string(),
            },
        });

        // Failed
        r.handle(ProgressEvent::InspectorStarted(InspectorId::Containers));
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Containers,
            outcome: InspectorOutcome::Failed {
                reason: "podman not found".to_string(),
            },
        });

        let text = output_text(&buf);
        // Verify no ANSI escape codes
        assert!(
            !text.contains("\x1b["),
            "found ANSI escape in no-color mode: {text}"
        );
        insta::assert_snapshot!(text);
    }

    #[test]
    fn pretty_arrival_order() {
        // Events arrive out of display order: Network finishes before RPM.
        let (r, buf) = test_renderer(false, false, false);

        // Network finishes first (fast).
        feed_simple_complete(&r, InspectorId::Network);
        // Services finishes second.
        feed_with_metric(&r, InspectorId::Services, MetricKind::UnitsFound, 42);
        // RPM finishes last (slow).
        feed_rpm(&r, 613, 6);

        let text = output_text(&buf);
        let lines: Vec<&str> = text.lines().collect();
        // Output order should be: Network, Services, RPM (arrival order).
        assert!(
            lines[0].contains("Network"),
            "first line should be Network: {}",
            lines[0]
        );
        assert!(
            lines[1].contains("Services"),
            "second line should be Services: {}",
            lines[1]
        );
        assert!(
            lines[2].contains("RPM"),
            "third line should be RPM: {}",
            lines[2]
        );
        insta::assert_snapshot!(text);
    }

    #[test]
    fn pretty_skipped_without_start() {
        // Inspector that is inapplicable — Skipped without InspectorStarted.
        let (r, buf) = test_renderer(false, false, false);

        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Selinux,
            outcome: InspectorOutcome::Skipped {
                reason: "disabled".to_string(),
            },
        });

        let text = output_text(&buf);
        assert!(
            text.contains("\u{25cb}"),
            "expected ○ symbol for skipped: {text}"
        );
        assert!(
            text.contains("skipped (disabled)"),
            "expected skip reason: {text}"
        );
        insta::assert_snapshot!(text);
    }

    #[test]
    fn pretty_verbose_atomic_parent_child() {
        // Verbose mode: parent + child lines should print together.
        let (r, buf) = test_renderer(false, true, false);

        // Services with a step, then Network simple.
        r.handle(ProgressEvent::InspectorStarted(InspectorId::Services));
        r.handle(ProgressEvent::StepStarted {
            inspector: InspectorId::Services,
            step: StepId::QueryingPackages,
        });
        r.handle(ProgressEvent::StepFinished {
            inspector: InspectorId::Services,
            step: StepId::QueryingPackages,
            outcome: StepOutcome::Complete,
        });
        r.handle(ProgressEvent::Metric {
            inspector: InspectorId::Services,
            kind: MetricKind::UnitsFound,
            value: 42,
        });
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Services,
            outcome: InspectorOutcome::Complete,
        });

        // Network simple (no steps).
        feed_simple_complete(&r, InspectorId::Network);

        let text = output_text(&buf);
        let lines: Vec<&str> = text.lines().collect();

        // Services parent line should be first.
        assert!(
            lines[0].contains("Services"),
            "expected Services parent first: {}",
            lines[0]
        );
        // Child lines should be indented and appear between Services and Network.
        let services_idx = lines.iter().position(|l| l.contains("Services")).unwrap();
        let network_idx = lines.iter().position(|l| l.contains("Network")).unwrap();
        // Children between parent and next inspector.
        assert!(
            network_idx > services_idx + 1,
            "expected child lines between Services and Network"
        );
        // Child lines should be indented with 6 spaces.
        for i in (services_idx + 1)..network_idx {
            assert!(
                lines[i].starts_with("      "),
                "child line not indented: {}",
                lines[i]
            );
        }
        insta::assert_snapshot!(text);
    }

    // ── Non-snapshot unit tests ────────────────────────────────────

    #[test]
    fn receipt_lines_accumulate() {
        let (r, _buf) = test_renderer(false, false, false);
        feed_rpm(&r, 100, 3);
        feed_simple_complete(&r, InspectorId::Services);
        let lines = r.receipt_lines();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].id, InspectorId::Rpm);
        assert_eq!(lines[1].id, InspectorId::Services);
    }

    #[test]
    fn typed_counts_populated() {
        let (r, _buf) = test_renderer(false, false, false);
        feed_with_metric(&r, InspectorId::Config, MetricKind::ConfigsModified, 37);
        let lines = r.receipt_lines();
        assert_eq!(lines[0].typed_counts.configs_modified, Some(37));
    }

    #[test]
    fn typed_counts_nonrpm_probes() {
        let (r, _buf) = test_renderer(false, false, false);
        feed_nonrpm_probes(
            &r,
            &[
                (&ProbeId::PipPackages, Some(23)),
                (&ProbeId::NpmPackages, Some(69)),
            ],
        );
        let lines = r.receipt_lines();
        assert_eq!(lines[0].typed_counts.pip_packages, Some(23));
        assert_eq!(lines[0].typed_counts.npm_packages, Some(69));
        assert_eq!(lines[0].typed_counts.gem_packages, None);
    }

    #[test]
    fn finalize_stub_does_not_panic() {
        let (r, _buf) = test_renderer(false, false, false);
        use crate::progress::receipt::ScanEndState;
        use std::path::PathBuf;
        use std::time::Duration;
        let scan = ScanFinalize {
            elapsed: Duration::from_secs(5),
            end_state: ScanEndState::Completed {
                path: PathBuf::from("/tmp/test.tar.gz"),
                sensitivity: None,
            },
            version_changes: None,
        };
        r.finalize(&scan);
    }
}
