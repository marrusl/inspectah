//! Pretty receipt renderer — arrival-order output with typed receipt lines.
//!
//! Prints each inspector's result as it finishes (arrival order), not in
//! display order.  In verbose mode, sub-step and probe child lines are
//! buffered per inspector and flushed atomically with the parent line.
//!
//! A background tick thread drives a safety-valve spinner: when any
//! inspector exceeds [`SPINNER_THRESHOLD`], a braille animation appears
//! on the line below the last receipt.  The spinner transfers between
//! slow inspectors and is cleared when the final result arrives.
//!
//! Uses the shared [`receipt`] data model so output cannot drift from
//! [`FlatRenderer`].

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use inspectah_core::types::completeness::InspectorId;
use inspectah_core::types::progress::{
    InspectorOutcome, MetricKind, ProbeOutcome, ProgressEvent, StepOutcome,
};

use super::display;
use super::receipt::{InspectorState, ReceiptLine, ScanFinalize, ScanSummary, TypedCounts};

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

// ── Spinner constants ─────────────────────────────────────────────

/// Braille spinner frames.
const SPINNER: &[char] = &[
    '\u{280b}', '\u{2819}', '\u{2839}', '\u{2838}', '\u{283c}', '\u{2834}', '\u{2826}', '\u{2827}',
    '\u{2807}', '\u{280f}',
];

/// Elapsed time threshold — show spinner after this duration.
const SPINNER_THRESHOLD: Duration = Duration::from_millis(3500);

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

    /// Create a tracker with a custom start time (for testing).
    #[cfg(test)]
    fn with_started_at(started_at: Instant) -> Self {
        Self {
            started_at,
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
    use_color: bool,
    verbose: bool,
    /// Per-inspector tracking, keyed by InspectorId.
    trackers: HashMap<InspectorId, InspectorTracker>,
    /// Display order slice — used for total count and name lookup only.
    display_order: &'static [(InspectorId, &'static str)],
    /// Built receipt lines, stored for later use by finalize (T5).
    receipt_lines: Vec<ReceiptLine>,
    /// Which inspector currently owns the spinner line (if any).
    spinner_active: Option<InspectorId>,
    /// Frame counter for braille animation.
    spinner_frame: usize,
}

// ── Public API ─────────────────────────────────────────────────────

/// Pretty-mode receipt renderer — arrival-order output with Unicode
/// symbols, optional ANSI color, and typed receipt lines.
///
/// Thread-safe via `Arc<Mutex>` shared with a background tick thread
/// that drives spinner animation for slow inspectors.
pub struct PrettyRenderer {
    state: Arc<Mutex<PrettyState>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    stop_tick: Arc<AtomicBool>,
    tick_handle: Option<JoinHandle<()>>,
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
        let state = Arc::new(Mutex::new(PrettyState {
            use_color,
            verbose,
            trackers: HashMap::new(),
            display_order,
            receipt_lines: Vec::new(),
            spinner_active: None,
            spinner_frame: 0,
        }));
        let writer = Arc::new(Mutex::new(writer));
        let stop_tick = Arc::new(AtomicBool::new(false));

        // Spawn background tick thread for spinner animation.
        let tick_state = Arc::clone(&state);
        let tick_writer = Arc::clone(&writer);
        let tick_stop = Arc::clone(&stop_tick);
        let tick_handle = std::thread::spawn(move || {
            while !tick_stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
                if tick_stop.load(Ordering::Relaxed) {
                    break;
                }
                let mut st = tick_state.lock().expect("tick lock");
                let mut wr = tick_writer.lock().expect("tick writer lock");
                st.tick_spinner(&mut *wr);
            }
        });

        Self {
            state,
            writer,
            stop_tick,
            tick_handle: Some(tick_handle),
        }
    }

    /// Handle a progress event.
    pub fn handle(&self, event: ProgressEvent) {
        let mut state = self.state.lock().expect("PrettyRenderer lock poisoned");
        let mut writer = self.writer.lock().expect("PrettyRenderer writer lock");
        match event {
            ProgressEvent::InspectorStarted(id) => {
                state.trackers.insert(id, InspectorTracker::new());
            }
            ProgressEvent::InspectorFinished { id, outcome } => {
                // If this inspector owns the spinner, clear the spinner line.
                if state.spinner_active == Some(id) {
                    let _ = write!(writer, "\x1b[2K\r");
                    state.spinner_active = None;
                }

                let line = state.build_receipt_line(id, &outcome);
                state.print_receipt_line(&line, &mut *writer);
                state.receipt_lines.push(line);
                state.trackers.remove(&id);

                // Transfer spinner to the next slowest inspector (if any).
                state.maybe_start_spinner();
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

    /// Finalize rendering — summary block + typed footer.
    pub fn finalize(&self, scan: &ScanFinalize) {
        // Stop the tick thread.
        self.stop_tick.store(true, Ordering::Relaxed);

        let mut state = self.state.lock().expect("finalize lock");
        let mut writer = self.writer.lock().expect("finalize writer lock");

        // Clear any lingering spinner line.
        if state.spinner_active.is_some() {
            let _ = write!(writer, "\x1b[2K\r");
            state.spinner_active = None;
        }

        // Build summary from collected receipt lines.
        let summary = ScanSummary::build(&state.receipt_lines, scan.version_changes.clone());

        // Print summary block (separator + content) if there's anything to show.
        if summary.has_content() {
            let _ = writeln!(writer);
            let _ = writeln!(writer, "  \u{2504}\u{2504}\u{2504}");
            if let Some(ref vc) = summary.version_changes {
                let _ = writeln!(writer, "  {}", vc.format());
            }
            for hotspot in &summary.hotspots {
                let _ = writeln!(writer, "  {}", hotspot.format_pretty());
            }
        }

        // Footer zone.
        let _ = writeln!(writer);
        let secs = scan.elapsed.as_secs_f64();

        match &scan.end_state {
            super::receipt::ScanEndState::Completed { path, sensitivity } => {
                let tally = if summary.non_success_tally.is_empty() {
                    String::new()
                } else {
                    format!(" {}", summary.non_success_tally.format())
                };
                let _ = writeln!(writer, "  Inspected in {secs:.1}s{tally}");
                let _ = writeln!(writer, "  Report: {}", path.display());
                let _ = writeln!(writer, "  To review: inspectah refine {}", path.display());
                if let Some(notice) = sensitivity {
                    for line in notice.lines() {
                        let _ = writeln!(writer, "  {line}");
                    }
                }
            }
            super::receipt::ScanEndState::InspectOnly { path } => {
                let tally = if summary.non_success_tally.is_empty() {
                    String::new()
                } else {
                    format!(" {}", summary.non_success_tally.format())
                };
                let _ = writeln!(writer, "  Inspected in {secs:.1}s{tally}");
                let _ = writeln!(writer, "  Output: {}", path.display());
            }
            super::receipt::ScanEndState::InspectOnlyStdout => {
                let tally = if summary.non_success_tally.is_empty() {
                    String::new()
                } else {
                    format!(" {}", summary.non_success_tally.format())
                };
                let _ = writeln!(writer, "  Inspected in {secs:.1}s{tally}");
            }
            super::receipt::ScanEndState::WriteFailure { error } => {
                let tally = if summary.non_success_tally.is_empty() {
                    String::new()
                } else {
                    format!(" {}", summary.non_success_tally.format())
                };
                let _ = writeln!(writer, "  Inspected in {secs:.1}s{tally}");
                let _ = writeln!(writer, "  Error: {error}");
            }
            super::receipt::ScanEndState::Interrupted { completed, total } => {
                let _ = writeln!(
                    writer,
                    "  Interrupted after {secs:.1}s ({completed} of {total} inspectors completed)"
                );
            }
        }
    }

    /// Cancel rendering (SIGINT path). Stops the tick thread without
    /// printing summary/footer — leaves the terminal as-is.
    pub fn cancel(&self) {
        self.stop_tick.store(true, Ordering::Relaxed);
    }

    /// Access built receipt lines (for T5 summary computation).
    pub fn receipt_lines(&self) -> Vec<ReceiptLine> {
        let state = self.state.lock().expect("PrettyRenderer lock poisoned");
        state.receipt_lines.clone()
    }
}

impl Drop for PrettyRenderer {
    fn drop(&mut self) {
        self.stop_tick.store(true, Ordering::Relaxed);
        if let Some(handle) = self.tick_handle.take() {
            let _ = handle.join();
        }
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
    fn print_receipt_line(&mut self, line: &ReceiptLine, writer: &mut dyn Write) {
        let use_color = self.use_color;
        let name = lookup_name(self.display_order, line.id);

        // Build the formatted line.
        let symbol = colored(line.state.symbol(), line.state.color_code(), use_color);
        let suffix = format_suffix(line);
        let _ = writeln!(writer, "  {symbol} {name:<NAME_WIDTH$} {suffix}");

        // Print receipt sub_lines (e.g., Non-RPM ecosystem breakdown).
        for sub in &line.sub_lines {
            let _ = writeln!(writer, "      {sub}");
        }

        // In verbose mode, print buffered child lines atomically with the parent.
        if self.verbose
            && let Some(tracker) = self.trackers.get(&line.id)
        {
            for child in &tracker.child_lines {
                let _ = writeln!(writer, "{child}");
            }
        }
    }

    /// Find the longest-running inspector that exceeds `SPINNER_THRESHOLD`.
    fn find_slowest_inspector(&self) -> Option<InspectorId> {
        let now = Instant::now();
        self.trackers
            .iter()
            .filter(|(_, t)| now.duration_since(t.started_at) >= SPINNER_THRESHOLD)
            .max_by_key(|(_, t)| now.duration_since(t.started_at))
            .map(|(id, _)| *id)
    }

    /// If no spinner is active and a slow inspector exists, activate spinner.
    fn maybe_start_spinner(&mut self) {
        if self.spinner_active.is_none()
            && let Some(id) = self.find_slowest_inspector()
        {
            self.spinner_active = Some(id);
            self.spinner_frame = 0;
        }
    }

    /// Called by the tick thread — advance spinner frame and redraw.
    fn tick_spinner(&mut self, writer: &mut dyn Write) {
        // If no spinner active, check if one should start.
        if self.spinner_active.is_none() {
            self.maybe_start_spinner();
        }

        if let Some(id) = self.spinner_active {
            if let Some(tracker) = self.trackers.get(&id) {
                let frame = SPINNER[self.spinner_frame % SPINNER.len()];
                let name = lookup_name(self.display_order, id);
                let elapsed = tracker.started_at.elapsed().as_secs_f64();
                let _ = write!(
                    writer,
                    "\x1b[2K\r  {frame} {name:<NAME_WIDTH$} ({elapsed:.1}s)"
                );
                let _ = writer.flush();
                self.spinner_frame += 1;
            } else {
                // Inspector was removed (finished) — clear.
                self.spinner_active = None;
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
    fn finalize_does_not_panic() {
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

    // ── Summary + footer snapshot tests (T5) ──────────────────────────

    /// Helper: feed a typical scan and finalize, returning only the
    /// summary + footer portion (everything after the receipt lines).
    fn finalize_output(
        r: &PrettyRenderer,
        buf: &Arc<Mutex<Vec<u8>>>,
        scan: &ScanFinalize,
    ) -> String {
        // Capture the buffer length before finalize to isolate footer output.
        let pre_len = buf.lock().expect("test lock").len();
        r.finalize(scan);
        let full = buf.lock().expect("test lock").clone();
        String::from_utf8(full[pre_len..].to_vec()).expect("valid utf8")
    }

    #[test]
    fn pretty_summary_with_version_changes() {
        use crate::progress::receipt::{ScanEndState, VersionChangeSummary};
        use std::path::PathBuf;

        let (r, buf) = test_renderer(false, false, false);

        // Feed inspectors that produce hotspot counts.
        feed_with_metric(&r, InspectorId::Config, MetricKind::ConfigsModified, 37);
        feed_nonrpm_probes(
            &r,
            &[
                (&ProbeId::PipPackages, Some(23)),
                (&ProbeId::NpmPackages, Some(69)),
            ],
        );

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(4.2),
            end_state: ScanEndState::Completed {
                path: PathBuf::from("/tmp/report.tar.gz"),
                sensitivity: None,
            },
            version_changes: Some(VersionChangeSummary {
                total: 58,
                target_newer: 54,
                host_newer: 4,
            }),
        };

        insta::assert_snapshot!(finalize_output(&r, &buf, &scan));
    }

    #[test]
    fn pretty_summary_clean_host() {
        use crate::progress::receipt::ScanEndState;
        use std::path::PathBuf;

        let (r, buf) = test_renderer(false, false, false);

        // All inspectors succeed with no hotspot-bearing metrics.
        feed_simple_complete(&r, InspectorId::Network);
        feed_simple_complete(&r, InspectorId::Storage);

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(1.3),
            end_state: ScanEndState::Completed {
                path: PathBuf::from("/tmp/report.tar.gz"),
                sensitivity: None,
            },
            version_changes: None,
        };

        insta::assert_snapshot!(finalize_output(&r, &buf, &scan));
    }

    #[test]
    fn pretty_footer_completed() {
        use crate::progress::receipt::ScanEndState;
        use std::path::PathBuf;

        let (r, buf) = test_renderer(false, false, false);
        feed_simple_complete(&r, InspectorId::Network);

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(2.5),
            end_state: ScanEndState::Completed {
                path: PathBuf::from("/tmp/host-report.tar.gz"),
                sensitivity: None,
            },
            version_changes: None,
        };

        insta::assert_snapshot!(finalize_output(&r, &buf, &scan));
    }

    #[test]
    fn pretty_footer_inspect_only() {
        use crate::progress::receipt::ScanEndState;
        use std::path::PathBuf;

        let (r, buf) = test_renderer(false, false, false);
        feed_simple_complete(&r, InspectorId::Network);

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(3.7),
            end_state: ScanEndState::InspectOnly {
                path: PathBuf::from("/tmp/inspect.json"),
            },
            version_changes: None,
        };

        insta::assert_snapshot!(finalize_output(&r, &buf, &scan));
    }

    #[test]
    fn pretty_footer_inspect_only_stdout() {
        use crate::progress::receipt::ScanEndState;

        let (r, buf) = test_renderer(false, false, false);
        feed_simple_complete(&r, InspectorId::Network);

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(1.0),
            end_state: ScanEndState::InspectOnlyStdout,
            version_changes: None,
        };

        insta::assert_snapshot!(finalize_output(&r, &buf, &scan));
    }

    #[test]
    fn pretty_footer_write_failure() {
        use crate::progress::receipt::ScanEndState;

        let (r, buf) = test_renderer(false, false, false);
        feed_simple_complete(&r, InspectorId::Network);

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(2.0),
            end_state: ScanEndState::WriteFailure {
                error: "Permission denied: /opt/report.tar.gz".to_string(),
            },
            version_changes: None,
        };

        insta::assert_snapshot!(finalize_output(&r, &buf, &scan));
    }

    #[test]
    fn pretty_footer_interrupted() {
        use crate::progress::receipt::ScanEndState;

        let (r, buf) = test_renderer(false, false, false);
        feed_simple_complete(&r, InspectorId::Network);

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(1.8),
            end_state: ScanEndState::Interrupted {
                completed: 5,
                total: 11,
            },
            version_changes: None,
        };

        insta::assert_snapshot!(finalize_output(&r, &buf, &scan));
    }

    #[test]
    fn pretty_footer_completed_with_sensitivity() {
        use crate::progress::receipt::ScanEndState;
        use std::path::PathBuf;

        let (r, buf) = test_renderer(false, false, false);
        feed_simple_complete(&r, InspectorId::Network);

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(3.0),
            end_state: ScanEndState::Completed {
                path: PathBuf::from("/tmp/report.tar.gz"),
                sensitivity: Some(
                    "Note: Report may contain sensitive data.\nReview before sharing.".to_string(),
                ),
            },
            version_changes: None,
        };

        insta::assert_snapshot!(finalize_output(&r, &buf, &scan));
    }

    #[test]
    fn pretty_footer_non_success_tally() {
        use crate::progress::receipt::ScanEndState;
        use std::path::PathBuf;

        let (r, buf) = test_renderer(false, false, false);

        // One failed, one degraded — tally should appear on timing line.
        r.handle(ProgressEvent::InspectorStarted(InspectorId::Containers));
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Containers,
            outcome: InspectorOutcome::Failed {
                reason: "podman not found".to_string(),
            },
        });
        r.handle(ProgressEvent::InspectorStarted(InspectorId::Config));
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Config,
            outcome: InspectorOutcome::Degraded {
                reason: "rpm verify timed out".to_string(),
            },
        });

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(4.5),
            end_state: ScanEndState::Completed {
                path: PathBuf::from("/tmp/report.tar.gz"),
                sensitivity: None,
            },
            version_changes: None,
        };

        insta::assert_snapshot!(finalize_output(&r, &buf, &scan));
    }

    // ── Spinner state tests ───────────────────────────────────────────

    /// Helper: build a PrettyState with injected trackers for spinner
    /// state testing without real-time delays.
    fn test_state(trackers: Vec<(InspectorId, InspectorTracker)>) -> PrettyState {
        PrettyState {
            use_color: false,
            verbose: false,
            trackers: trackers.into_iter().collect(),
            display_order: display::active_display_order(false),
            receipt_lines: Vec::new(),
            spinner_active: None,
            spinner_frame: 0,
        }
    }

    #[test]
    fn spinner_triggers_after_threshold() {
        // An inspector that started 5s ago exceeds the 3.5s threshold.
        let old_start = Instant::now() - Duration::from_secs(5);
        let mut state = test_state(vec![(
            InspectorId::Rpm,
            InspectorTracker::with_started_at(old_start),
        )]);

        assert!(
            state.spinner_active.is_none(),
            "spinner should start inactive"
        );

        state.maybe_start_spinner();

        assert_eq!(
            state.spinner_active,
            Some(InspectorId::Rpm),
            "spinner should activate for slow inspector"
        );
    }

    #[test]
    fn spinner_replaced_by_result() {
        // Spinner is active for RPM; RPM finishes → spinner clears,
        // final output has no spinner residue (no ANSI clear sequences
        // in the receipt line output).
        let old_start = Instant::now() - Duration::from_secs(5);
        let mut state = test_state(vec![(
            InspectorId::Rpm,
            InspectorTracker::with_started_at(old_start),
        )]);
        state.spinner_active = Some(InspectorId::Rpm);
        state.spinner_frame = 3;

        // Simulate InspectorFinished: clear spinner, print result.
        let mut output = Vec::new();

        // Clear spinner line (as handle() does).
        let _ = write!(output, "\x1b[2K\r");
        state.spinner_active = None;

        // Build and print the receipt.
        let line = state.build_receipt_line(InspectorId::Rpm, &InspectorOutcome::Complete);
        state.print_receipt_line(&line, &mut output);
        state.trackers.remove(&InspectorId::Rpm);

        // No spinner should remain active.
        assert!(state.spinner_active.is_none(), "spinner should be cleared");

        // The receipt output should contain the result, not spinner frames.
        let text = String::from_utf8(output).expect("valid utf8");
        assert!(text.contains("RPM"), "output should contain receipt line");
        // No braille spinner characters in the final output.
        for &frame in SPINNER {
            assert!(
                !text.contains(frame),
                "output should not contain spinner frame '{frame}'"
            );
        }
    }

    #[test]
    fn spinner_interrupted_by_other_completion() {
        // RPM is slow (spinner active). Services finishes → spinner
        // is NOT cleared (Services doesn't own it), result prints,
        // spinner stays on RPM.
        let old_rpm = Instant::now() - Duration::from_secs(5);
        let recent_svc = Instant::now() - Duration::from_millis(500);
        let mut state = test_state(vec![
            (InspectorId::Rpm, InspectorTracker::with_started_at(old_rpm)),
            (
                InspectorId::Services,
                InspectorTracker::with_started_at(recent_svc),
            ),
        ]);
        state.spinner_active = Some(InspectorId::Rpm);

        // Services finishes — doesn't own spinner, so no clear.
        // (In handle(), the clear only happens when spinner_active == id.)
        let finishing_id = InspectorId::Services;
        let owns_spinner = state.spinner_active == Some(finishing_id);
        assert!(!owns_spinner, "Services should not own the spinner");

        // Spinner stays on RPM.
        assert_eq!(
            state.spinner_active,
            Some(InspectorId::Rpm),
            "spinner should remain on RPM"
        );

        // Clean up Services tracker.
        state.trackers.remove(&InspectorId::Services);

        // RPM still has the spinner.
        assert_eq!(
            state.spinner_active,
            Some(InspectorId::Rpm),
            "spinner should still be on RPM after Services finishes"
        );
    }

    #[test]
    fn spinner_transfers_to_next_slow() {
        // Both RPM and Config are slow.  RPM finishes → spinner
        // transfers to Config.
        let old_rpm = Instant::now() - Duration::from_secs(6);
        let old_cfg = Instant::now() - Duration::from_secs(4);
        let mut state = test_state(vec![
            (InspectorId::Rpm, InspectorTracker::with_started_at(old_rpm)),
            (
                InspectorId::Config,
                InspectorTracker::with_started_at(old_cfg),
            ),
        ]);
        state.spinner_active = Some(InspectorId::Rpm);

        // RPM finishes: clear spinner, remove tracker, transfer.
        state.spinner_active = None;
        state.trackers.remove(&InspectorId::Rpm);
        state.maybe_start_spinner();

        assert_eq!(
            state.spinner_active,
            Some(InspectorId::Config),
            "spinner should transfer to the next slow inspector (Config)"
        );
    }

    // ── End-to-end snapshot tests ──────────────────────────────────
    //
    // These test the complete output from receipt lines through
    // finalize (summary + footer) for each spec scenario.

    /// Helper: feed all 11 inspectors, finalize, and return full output.
    fn e2e_full_output(
        r: &PrettyRenderer,
        buf: &Arc<Mutex<Vec<u8>>>,
        scan: &ScanFinalize,
    ) -> String {
        r.finalize(scan);
        output_text(buf)
    }

    #[test]
    fn e2e_pretty_fast_scan() {
        // All inspectors complete quickly in roughly display order.
        use crate::progress::receipt::{ScanEndState, VersionChangeSummary};
        use std::path::PathBuf;

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

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(12.3),
            end_state: ScanEndState::Completed {
                path: PathBuf::from("/tmp/inspectah-report.tar.gz"),
                sensitivity: None,
            },
            version_changes: Some(VersionChangeSummary {
                total: 58,
                target_newer: 54,
                host_newer: 4,
            }),
        };

        insta::assert_snapshot!(e2e_full_output(&r, &buf, &scan));
    }

    #[test]
    fn e2e_pretty_slow_rpm_scan() {
        // Fast inspectors print first, RPM prints after long delay.
        use crate::progress::receipt::{ScanEndState, VersionChangeSummary};
        use std::path::PathBuf;

        let (r, buf) = test_renderer(false, false, false);

        // Wave 1: fast inspectors arrive first.
        feed_with_metric(&r, InspectorId::Services, MetricKind::UnitsFound, 27);
        feed_simple_complete(&r, InspectorId::Storage);
        feed_simple_complete(&r, InspectorId::KernelBoot);
        feed_simple_complete(&r, InspectorId::Network);
        feed_with_metric(&r, InspectorId::Containers, MetricKind::ContainersFound, 1);

        // RPM finally arrives (slow).
        feed_rpm(&r, 847, 8);

        // Wave 2: remaining inspectors.
        feed_simple_complete(&r, InspectorId::UsersGroups);
        feed_with_metric(&r, InspectorId::ScheduledTasks, MetricKind::TimersFound, 2);
        feed_with_metric(&r, InspectorId::Config, MetricKind::ConfigsModified, 12);
        feed_simple_complete(&r, InspectorId::Selinux);
        feed_nonrpm_probes(
            &r,
            &[
                (&ProbeId::PipPackages, Some(5)),
                (&ProbeId::NpmPackages, None),
            ],
        );

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(45.7),
            end_state: ScanEndState::Completed {
                path: PathBuf::from("/tmp/inspectah-report.tar.gz"),
                sensitivity: None,
            },
            version_changes: Some(VersionChangeSummary {
                total: 120,
                target_newer: 115,
                host_newer: 5,
            }),
        };

        insta::assert_snapshot!(e2e_full_output(&r, &buf, &scan));
    }

    #[test]
    fn e2e_pretty_failures_degradation() {
        // Mix of success, degraded, and failed inspectors.
        use crate::progress::receipt::ScanEndState;
        use std::path::PathBuf;

        let (r, buf) = test_renderer(false, false, false);

        // RPM degraded (rpm verify timed out).
        r.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        r.handle(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::PackagesFound,
            value: 613,
        });
        r.handle(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::ReposMapped,
            value: 6,
        });
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Degraded {
                reason: "rpm verify timed out".to_string(),
            },
        });

        feed_with_metric(&r, InspectorId::Services, MetricKind::UnitsFound, 42);
        feed_simple_complete(&r, InspectorId::Storage);
        feed_simple_complete(&r, InspectorId::KernelBoot);

        // Network failed.
        r.handle(ProgressEvent::InspectorStarted(InspectorId::Network));
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Network,
            outcome: InspectorOutcome::Failed {
                reason: "timeout connecting to NetworkManager".to_string(),
            },
        });

        feed_with_metric(&r, InspectorId::Containers, MetricKind::ContainersFound, 3);
        feed_simple_complete(&r, InspectorId::UsersGroups);
        feed_with_metric(&r, InspectorId::ScheduledTasks, MetricKind::TimersFound, 5);
        feed_with_metric(&r, InspectorId::Config, MetricKind::ConfigsModified, 37);

        // SELinux skipped.
        r.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Selinux,
            outcome: InspectorOutcome::Skipped {
                reason: "disabled".to_string(),
            },
        });

        feed_nonrpm_probes(
            &r,
            &[
                (&ProbeId::PipPackages, Some(23)),
                (&ProbeId::NpmPackages, Some(69)),
            ],
        );

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(18.5),
            end_state: ScanEndState::Completed {
                path: PathBuf::from("/tmp/inspectah-report.tar.gz"),
                sensitivity: None,
            },
            version_changes: None,
        };

        insta::assert_snapshot!(e2e_full_output(&r, &buf, &scan));
    }

    #[test]
    fn e2e_pretty_clean_host() {
        // All inspectors succeed with minimal findings — no summary block.
        use crate::progress::receipt::ScanEndState;
        use std::path::PathBuf;

        let (r, buf) = test_renderer(false, false, false);

        feed_rpm(&r, 200, 3);
        feed_with_metric(&r, InspectorId::Services, MetricKind::UnitsFound, 15);
        feed_simple_complete(&r, InspectorId::Storage);
        feed_simple_complete(&r, InspectorId::KernelBoot);
        feed_simple_complete(&r, InspectorId::Network);
        feed_simple_complete(&r, InspectorId::Containers);
        feed_simple_complete(&r, InspectorId::UsersGroups);
        feed_simple_complete(&r, InspectorId::ScheduledTasks);
        feed_simple_complete(&r, InspectorId::Config);
        feed_simple_complete(&r, InspectorId::Selinux);
        feed_simple_complete(&r, InspectorId::NonRpmSoftware);

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(3.1),
            end_state: ScanEndState::Completed {
                path: PathBuf::from("/tmp/inspectah-report.tar.gz"),
                sensitivity: None,
            },
            version_changes: None,
        };

        insta::assert_snapshot!(e2e_full_output(&r, &buf, &scan));
    }

    #[test]
    fn e2e_pretty_interrupted() {
        // 5 inspectors complete, 6 interrupted via SIGINT reconciliation.
        use crate::progress::receipt::ScanEndState;

        let (r, buf) = test_renderer(false, false, false);

        // 5 complete.
        feed_rpm(&r, 613, 6);
        feed_with_metric(&r, InspectorId::Services, MetricKind::UnitsFound, 42);
        feed_simple_complete(&r, InspectorId::Storage);
        feed_simple_complete(&r, InspectorId::KernelBoot);
        feed_simple_complete(&r, InspectorId::Network);

        // 6 interrupted (SIGINT reconciliation synthesizes these).
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

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(6.8),
            end_state: ScanEndState::Interrupted {
                completed: 5,
                total: 11,
            },
            version_changes: None,
        };

        insta::assert_snapshot!(e2e_full_output(&r, &buf, &scan));
    }

    #[test]
    fn e2e_pretty_subscription() {
        // 12 inspectors including Subscription.
        use crate::progress::receipt::{ScanEndState, VersionChangeSummary};
        use std::path::PathBuf;

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
                (&ProbeId::GemPackages, None),
            ],
        );
        feed_simple_complete(&r, InspectorId::Subscription);

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(14.2),
            end_state: ScanEndState::Completed {
                path: PathBuf::from("/tmp/inspectah-report.tar.gz"),
                sensitivity: None,
            },
            version_changes: Some(VersionChangeSummary {
                total: 58,
                target_newer: 54,
                host_newer: 4,
            }),
        };

        insta::assert_snapshot!(e2e_full_output(&r, &buf, &scan));
    }

    #[test]
    fn e2e_pretty_inspect_only_stdout() {
        // Inspect-only without --output: no path in footer.
        use crate::progress::receipt::ScanEndState;

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
            ],
        );

        let scan = ScanFinalize {
            elapsed: Duration::from_secs_f64(10.5),
            end_state: ScanEndState::InspectOnlyStdout,
            version_changes: None,
        };

        insta::assert_snapshot!(e2e_full_output(&r, &buf, &scan));
    }
}
