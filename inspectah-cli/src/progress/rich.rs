//! Rich (block-redraw) progress renderer.
//!
//! Redraws the full checklist in-place using cursor-up and line-clear
//! ANSI sequences.  A background tick thread (~100ms) drives spinner
//! animation and elapsed-time updates.  The state model is fully
//! testable without a terminal — [`ChecklistState::render_lines`]
//! produces plain `Vec<String>` output.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use inspectah_core::types::completeness::InspectorId;
use inspectah_core::types::progress::{
    InspectorOutcome, MetricKind, ProbeId, ProbeOutcome, ProgressEvent, StepId, StepOutcome,
};

use super::display;

// ── ANSI helpers ────────────────────────────────────────────────────

const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// Braille spinner frames.
const SPINNER: &[char] = &[
    '\u{280b}', '\u{2819}', '\u{2839}', '\u{2838}', '\u{283c}', '\u{2834}', '\u{2826}', '\u{2827}',
    '\u{2807}', '\u{280f}',
];

/// Elapsed time threshold — only show `(Ns)` after this duration.
const ELAPSED_THRESHOLD: Duration = Duration::from_millis(3500);

/// Wrap `symbol` in ANSI color codes when color is enabled.
fn colored(symbol: &str, code: &str, use_color: bool) -> String {
    if use_color {
        format!("{code}{symbol}{RESET}")
    } else {
        symbol.to_string()
    }
}

// ── State model (testable without a terminal) ───────────────────────

/// Row state for inspectors and sub-steps.
#[derive(Debug, Clone)]
enum RowState {
    /// Waiting to start.
    Pending,
    /// Currently running.
    Active,
    /// Finished successfully.
    Complete { elapsed: Duration },
    /// Skipped (not applicable).
    Skipped { reason: String },
    /// Partial success with degradation.
    Degraded { reason: String, elapsed: Duration },
    /// Hard failure.
    Failed { reason: String },
    /// Scan was interrupted.
    Interrupted,
}

/// A sub-step row (RPM / Config inspectors).
#[derive(Debug, Clone)]
struct SubStepRow {
    step: StepId,
    state: RowState,
    metric: Option<(MetricKind, usize)>,
}

/// Probe row state — Active or Found.  Empty probes are removed.
#[derive(Debug, Clone)]
enum ProbeRowState {
    /// Probe is running.
    Active,
    /// Probe found results.
    Found { count: usize },
}

/// A probe row (Non-RPM inspector).
#[derive(Debug, Clone)]
struct ProbeRow {
    probe: ProbeId,
    state: ProbeRowState,
}

/// One inspector row in the checklist.
#[derive(Debug, Clone)]
struct InspectorRow {
    id: InspectorId,
    state: RowState,
    started_at: Option<Instant>,
    sub_steps: Vec<SubStepRow>,
    probes: Vec<ProbeRow>,
    /// Transient metric for the current step — consumed by StepFinished.
    last_metric: Option<(MetricKind, usize)>,
    /// Last metric seen across all steps — used by the parent completion line.
    inspector_metric: Option<(MetricKind, usize)>,
    /// Count of probes that found results — used by NonRpmSoftware completion.
    probes_found_count: Option<usize>,
}

/// In-memory checklist state — the entire model for rich-mode rendering.
///
/// Constructed with all 11 display-order inspectors pre-populated as
/// [`RowState::Pending`].  Events update state; [`render_lines`] produces
/// the visual output as plain strings.
#[derive(Debug)]
struct ChecklistState {
    rows: Vec<InspectorRow>,
    tick_count: u32,
    terminal_height: usize,
    use_color: bool,
    lines_rendered: usize,
}

impl ChecklistState {
    /// Create a new checklist with all inspectors in `Pending` state.
    fn new(use_color: bool, terminal_height: usize) -> Self {
        let rows = display::DISPLAY_ORDER
            .iter()
            .map(|(id, _)| InspectorRow {
                id: *id,
                state: RowState::Pending,
                started_at: None,
                sub_steps: Vec::new(),
                probes: Vec::new(),
                last_metric: None,
                inspector_metric: None,
                probes_found_count: None,
            })
            .collect();

        Self {
            rows,
            tick_count: 0,
            terminal_height,
            use_color,
            lines_rendered: 0,
        }
    }

    /// Find the row index for an inspector.
    fn find_row(&self, id: InspectorId) -> Option<usize> {
        self.rows.iter().position(|r| r.id == id)
    }

    /// Process a progress event, updating internal state.
    fn handle_event(&mut self, event: ProgressEvent) {
        match event {
            ProgressEvent::InspectorStarted(id) => {
                if let Some(idx) = self.find_row(id) {
                    self.rows[idx].state = RowState::Active;
                    self.rows[idx].started_at = Some(Instant::now());
                    self.rows[idx].last_metric = None;
                    self.rows[idx].inspector_metric = None;

                    // Pre-populate sub-steps so the full checklist is
                    // visible upfront (not lazily on StepStarted).
                    match id {
                        InspectorId::Rpm => {
                            self.rows[idx].sub_steps = vec![
                                SubStepRow {
                                    step: StepId::QueryingPackages,
                                    state: RowState::Pending,
                                    metric: None,
                                },
                                SubStepRow {
                                    step: StepId::ClassifyingPackages,
                                    state: RowState::Pending,
                                    metric: None,
                                },
                                SubStepRow {
                                    step: StepId::ResolvingSourceRepos,
                                    state: RowState::Pending,
                                    metric: None,
                                },
                                SubStepRow {
                                    step: StepId::ResolvingDepTree,
                                    state: RowState::Pending,
                                    metric: None,
                                },
                                SubStepRow {
                                    step: StepId::VerifyingIntegrity,
                                    state: RowState::Pending,
                                    metric: None,
                                },
                                SubStepRow {
                                    step: StepId::MappingFileOwnership,
                                    state: RowState::Pending,
                                    metric: None,
                                },
                            ];
                        }
                        InspectorId::Config => {
                            self.rows[idx].sub_steps = vec![
                                SubStepRow {
                                    step: StepId::ApplyingRpmVerification,
                                    state: RowState::Pending,
                                    metric: None,
                                },
                                SubStepRow {
                                    step: StepId::WalkingFilesystem,
                                    state: RowState::Pending,
                                    metric: None,
                                },
                                SubStepRow {
                                    step: StepId::ClassifyingConfigs,
                                    state: RowState::Pending,
                                    metric: None,
                                },
                            ];
                        }
                        _ => {}
                    }
                }
            }
            ProgressEvent::InspectorFinished { id, outcome } => {
                if let Some(idx) = self.find_row(id) {
                    let elapsed = self.rows[idx]
                        .started_at
                        .map(|t| t.elapsed())
                        .unwrap_or_default();

                    // Non-RPM parent: compute ecosystem count from found probes.
                    if id == InspectorId::NonRpmSoftware
                        && matches!(outcome, InspectorOutcome::Complete)
                    {
                        let ecosystems = self.rows[idx]
                            .probes
                            .iter()
                            .filter(|p| matches!(p.state, ProbeRowState::Found { .. }))
                            .count();
                        self.rows[idx].probes_found_count = Some(ecosystems);
                    }

                    // Reconcile child sub-steps: any still Active or
                    // Pending should inherit the parent's terminal state.
                    for sub in &mut self.rows[idx].sub_steps {
                        let still_open = matches!(sub.state, RowState::Active | RowState::Pending);
                        if !still_open {
                            continue;
                        }
                        sub.state = match &outcome {
                            InspectorOutcome::Complete => RowState::Complete {
                                elapsed: Duration::ZERO,
                            },
                            InspectorOutcome::Failed { reason } => RowState::Failed {
                                reason: reason.clone(),
                            },
                            InspectorOutcome::Interrupted => RowState::Interrupted,
                            InspectorOutcome::Degraded { reason } => RowState::Degraded {
                                reason: reason.clone(),
                                elapsed: Duration::ZERO,
                            },
                            InspectorOutcome::Skipped { reason } => RowState::Skipped {
                                reason: reason.clone(),
                            },
                        };
                    }

                    self.rows[idx].state = match outcome {
                        InspectorOutcome::Complete => RowState::Complete { elapsed },
                        InspectorOutcome::Skipped { reason } => RowState::Skipped { reason },
                        InspectorOutcome::Degraded { reason } => {
                            RowState::Degraded { reason, elapsed }
                        }
                        InspectorOutcome::Failed { reason } => RowState::Failed { reason },
                        InspectorOutcome::Interrupted => RowState::Interrupted,
                    };
                    // Preserve last_metric for the completion line rendering.
                    // InspectorStarted already resets it for the next inspector.
                }
            }
            ProgressEvent::StepStarted { inspector, step } => {
                if let Some(idx) = self.find_row(inspector) {
                    // If the sub-step was pre-populated (Pending), transition
                    // it to Active. Otherwise append a new row (backwards compat).
                    if let Some(sub) = self.rows[idx].sub_steps.iter_mut().find(|s| s.step == step)
                    {
                        sub.state = RowState::Active;
                    } else {
                        self.rows[idx].sub_steps.push(SubStepRow {
                            step,
                            state: RowState::Active,
                            metric: None,
                        });
                    }
                }
            }
            ProgressEvent::StepFinished {
                inspector,
                step,
                outcome,
            } => {
                if let Some(idx) = self.find_row(inspector) {
                    // Take metric before borrowing sub_steps mutably.
                    let captured_metric = self.rows[idx].last_metric.take();
                    if let Some(sub) = self.rows[idx].sub_steps.iter_mut().find(|s| s.step == step)
                    {
                        sub.metric = captured_metric;
                        sub.state = match outcome {
                            StepOutcome::Complete => RowState::Complete {
                                elapsed: Duration::ZERO,
                            },
                            StepOutcome::Degraded { reason } => RowState::Degraded {
                                reason,
                                elapsed: Duration::ZERO,
                            },
                            StepOutcome::Failed { reason } => RowState::Failed { reason },
                            StepOutcome::Skipped { reason } => RowState::Skipped { reason },
                            StepOutcome::Interrupted => RowState::Interrupted,
                        };
                    }
                    self.rows[idx].last_metric = None;
                }
            }
            ProgressEvent::Metric {
                inspector,
                kind,
                value,
            } => {
                if let Some(idx) = self.find_row(inspector) {
                    self.rows[idx].last_metric = Some((kind.clone(), value));
                    self.rows[idx].inspector_metric = Some((kind, value));
                }
            }
            ProgressEvent::ProbeStarted { inspector, probe } => {
                if let Some(idx) = self.find_row(inspector) {
                    self.rows[idx].probes.push(ProbeRow {
                        probe,
                        state: ProbeRowState::Active,
                    });
                }
            }
            ProgressEvent::ProbeFinished {
                inspector,
                probe,
                outcome,
            } => {
                if let Some(idx) = self.find_row(inspector) {
                    match outcome {
                        ProbeOutcome::Empty => {
                            // Disappearing empties: remove the probe row entirely
                            self.rows[idx].probes.retain(|p| p.probe != probe);
                        }
                        ProbeOutcome::Found { count } => {
                            if let Some(p) =
                                self.rows[idx].probes.iter_mut().find(|p| p.probe == probe)
                            {
                                p.state = ProbeRowState::Found { count };
                            }
                        }
                    }
                }
            }
        }
    }

    /// Increment the tick counter (called by the background thread).
    fn tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);
    }

    /// Render the current state as a vector of display lines.
    ///
    /// This is a pure function on state — no terminal I/O.  Tests
    /// call this directly to assert rendered output.
    fn render_lines(&self) -> Vec<String> {
        let spinner_frame = SPINNER[(self.tick_count as usize) % SPINNER.len()];
        let use_color = self.use_color;

        // Compute max lines available for the checklist block.
        // Reserve 2 lines for breathing room at the bottom.
        let max_lines = self.terminal_height.saturating_sub(2);

        // Build all candidate lines first, then truncate if needed.
        let mut all_lines: Vec<String> = Vec::new();
        // Track which lines are "pending" so we can drop them on overflow.
        let mut line_categories: Vec<LineCategory> = Vec::new();

        for row in &self.rows {
            let name = display::display_name(row.id);

            let line = match &row.state {
                RowState::Pending => {
                    let sym = colored("\u{25cc}", DIM, use_color); // ◌
                    format!("  {sym} {name}")
                }
                RowState::Active => {
                    let elapsed_str = self.format_elapsed(row.started_at);
                    format!("  {spinner_frame} {name}{elapsed_str}")
                }
                RowState::Complete { elapsed } => {
                    let sym = colored("\u{2713}", GREEN, use_color); // ✓
                    let label = if let Some(count) = row.probes_found_count {
                        if count == 0 {
                            "none found".to_string()
                        } else {
                            if count == 1 {
                                "1 ecosystem".to_string()
                            } else {
                                format!("{count} ecosystems")
                            }
                        }
                    } else {
                        match &row.inspector_metric {
                            Some((kind, value)) => display::metric_label(kind, *value),
                            None => "done".to_string(),
                        }
                    };
                    let suf = format_elapsed_suf(*elapsed);
                    format!("  {sym} {name:<40} {label}{suf}")
                }
                RowState::Skipped { reason } => {
                    let sym = colored("\u{2013}", DIM, use_color); // –
                    format!("  {sym} {name:<40} skipped ({reason})")
                }
                RowState::Degraded { reason, elapsed } => {
                    let sym = colored("~", YELLOW, use_color);
                    let suf = format_elapsed_suf(*elapsed);
                    format!("  {sym} {name:<40} degraded: {reason}{suf}")
                }
                RowState::Failed { reason } => {
                    let sym = colored("\u{2717}", RED, use_color); // ✗
                    format!("  {sym} {name:<40} failed: {reason}")
                }
                RowState::Interrupted => {
                    let sym = colored("\u{25a0}", RED, use_color); // ■
                    format!("  {sym} {name:<40} interrupted")
                }
            };

            let cat = match &row.state {
                RowState::Pending => LineCategory::Pending,
                _ => LineCategory::Visible,
            };
            all_lines.push(line);
            line_categories.push(cat);

            // Sub-step rows (RPM, Config)
            for sub in &row.sub_steps {
                let step_name = display::step_name(&sub.step);
                let sub_line = match &sub.state {
                    RowState::Active => {
                        format!("       {spinner_frame} {step_name}")
                    }
                    RowState::Complete { .. } => {
                        let sym = colored("\u{2713}", GREEN, use_color);
                        let suf = format_sub_step_suffix(&sub.metric);
                        format!("       {sym} {step_name:<36} {suf}")
                    }
                    RowState::Degraded { reason, .. } => {
                        let sym = colored("~", YELLOW, use_color);
                        format!("       {sym} {step_name:<36} degraded: {reason}")
                    }
                    RowState::Failed { reason } => {
                        let sym = colored("\u{2717}", RED, use_color);
                        format!("       {sym} {step_name:<36} failed: {reason}")
                    }
                    RowState::Skipped { reason } => {
                        let sym = colored("\u{2013}", DIM, use_color);
                        format!("       {sym} {step_name:<36} skipped ({reason})")
                    }
                    RowState::Interrupted => {
                        let sym = colored("\u{25a0}", RED, use_color);
                        format!("       {sym} {step_name:<36} interrupted")
                    }
                    RowState::Pending => {
                        format!("       \u{25cc} {step_name}")
                    }
                };
                let sub_cat = match &sub.state {
                    RowState::Pending => LineCategory::Pending,
                    _ => LineCategory::Visible,
                };
                all_lines.push(sub_line);
                line_categories.push(sub_cat);
            }

            // Probe rows (Non-RPM)
            for p in &row.probes {
                let pname = display::probe_name(&p.probe);
                let probe_line = match &p.state {
                    ProbeRowState::Active => {
                        format!("       {spinner_frame} {pname}")
                    }
                    ProbeRowState::Found { count } => {
                        let sym = colored("\u{2713}", GREEN, use_color);
                        format!("       {sym} {pname:<36} {count} found")
                    }
                };
                all_lines.push(probe_line);
                line_categories.push(LineCategory::Visible);
            }
        }

        // If the block fits, return all lines.
        if all_lines.len() <= max_lines {
            return all_lines;
        }

        // Overflow: drop pending lines first, then show a truncation footer.
        let mut result: Vec<String> = Vec::new();
        let mut hidden = 0usize;

        for (line, cat) in all_lines.iter().zip(line_categories.iter()) {
            if result.len() + 1 >= max_lines {
                // Reserve last line for the "... and N more" footer.
                hidden += 1;
                continue;
            }
            match cat {
                LineCategory::Pending => {
                    hidden += 1;
                }
                LineCategory::Visible => {
                    result.push(line.clone());
                }
            }
        }

        if hidden > 0 {
            result.push(format!(
                "  {} ... and {hidden} more",
                colored("\u{00b7}", DIM, use_color)
            ));
        }

        result
    }

    /// Format the elapsed suffix for an active row.
    fn format_elapsed(&self, started_at: Option<Instant>) -> String {
        match started_at {
            Some(t) if t.elapsed() >= ELAPSED_THRESHOLD => {
                format!(" ({}s)", t.elapsed().as_secs())
            }
            _ => String::new(),
        }
    }
}

/// Line categorization for overflow truncation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineCategory {
    /// A pending inspector line — can be hidden on overflow.
    Pending,
    /// An active, completed, or child line — always shown.
    Visible,
}

/// Format the elapsed suffix `" (N.Ns)"` for finished rows.
/// Only shows timing for phases that took longer than the threshold.
fn format_elapsed_suf(elapsed: Duration) -> String {
    let secs = elapsed.as_secs_f64();
    if secs >= super::display::TIMER_THRESHOLD_SECS {
        format!(" ({secs:.1}s)")
    } else {
        String::new()
    }
}

/// Format the metric suffix for a completed sub-step.
fn format_sub_step_suffix(metric: &Option<(MetricKind, usize)>) -> String {
    match metric {
        Some((kind, value)) => display::metric_label(kind, *value),
        None => "done".to_string(),
    }
}

// ── Public renderer ─────────────────────────────────────────────────

/// Rich-mode progress renderer with block-redraw and spinner animation.
///
/// Thread-safe via [`Arc<Mutex<...>>`] wrappers.  A background tick
/// thread redraws the checklist every ~100ms for spinner animation
/// and elapsed-time updates.
pub struct RichRenderer {
    state: Arc<Mutex<ChecklistState>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    stop_tick: Arc<AtomicBool>,
    tick_handle: Option<JoinHandle<()>>,
}

impl RichRenderer {
    /// Create a new rich renderer writing to `writer`.
    ///
    /// When `verbose` is true, all sub-steps are shown regardless of
    /// any future fast-inspector optimizations.
    ///
    /// Spawns a background tick thread that redraws every ~100ms.
    /// Call [`finalize`] to stop the tick thread and print the final
    /// durable output.
    pub fn new(
        writer: Box<dyn Write + Send>,
        use_color: bool,
        terminal_height: usize,
        _verbose: bool,
    ) -> Self {
        let state = Arc::new(Mutex::new(ChecklistState::new(use_color, terminal_height)));
        let writer = Arc::new(Mutex::new(writer));
        let stop_tick = Arc::new(AtomicBool::new(false));

        let tick_state = Arc::clone(&state);
        let tick_writer = Arc::clone(&writer);
        let tick_stop = Arc::clone(&stop_tick);

        let tick_handle = std::thread::spawn(move || {
            while !tick_stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(100));
                if tick_stop.load(Ordering::SeqCst) {
                    break;
                }
                // Lock order: state first, writer second.
                let mut st = tick_state.lock().expect("tick: state lock poisoned");
                st.tick();
                let lines = st.render_lines();
                let prev_rendered = st.lines_rendered;
                st.lines_rendered = lines.len();
                drop(st);

                let mut w = tick_writer.lock().expect("tick: writer lock poisoned");
                redraw(&mut *w, &lines, prev_rendered);
                let _ = w.flush();
            }
        });

        Self {
            state,
            writer,
            stop_tick,
            tick_handle: Some(tick_handle),
        }
    }

    /// Process a progress event, updating state and triggering a redraw.
    pub fn handle(&self, event: ProgressEvent) {
        // Lock order: state first, writer second.
        let mut st = self.state.lock().expect("handle: state lock poisoned");
        st.handle_event(event);
        let lines = st.render_lines();
        let prev_rendered = st.lines_rendered;
        st.lines_rendered = lines.len();
        drop(st);

        let mut w = self.writer.lock().expect("handle: writer lock poisoned");
        redraw(&mut *w, &lines, prev_rendered);
        let _ = w.flush();
    }

    /// Stop the tick thread and print the final durable checklist.
    ///
    /// After calling this, the in-progress block is cleared and
    /// replaced with a permanent rendering (no cursor-up on the
    /// final print).
    pub fn finalize(&mut self) {
        // Signal tick thread to stop.
        self.stop_tick.store(true, Ordering::SeqCst);
        if let Some(handle) = self.tick_handle.take() {
            let _ = handle.join();
        }

        // Lock order: state first, writer second.
        let st = self.state.lock().expect("finalize: state lock poisoned");
        let lines = st.render_lines();
        let prev_rendered = st.lines_rendered;
        drop(st);

        let mut w = self.writer.lock().expect("finalize: writer lock poisoned");

        // Clear the in-progress block.
        clear_block(&mut *w, prev_rendered);

        // Print final state as permanent output (no cursor-up tracking).
        for line in &lines {
            let _ = writeln!(w, "{line}");
        }
        let _ = w.flush();
    }

    /// Cancel rendering (SIGINT path). Stops the tick thread without
    /// reprinting the checklist — leaves the terminal as-is.
    pub fn cancel(&mut self) {
        self.stop_tick.store(true, Ordering::SeqCst);
        if let Some(handle) = self.tick_handle.take() {
            let _ = handle.join();
        }
    }
}

/// Redraw the checklist block using cursor-up and line-clear sequences.
fn redraw(w: &mut dyn Write, lines: &[String], prev_lines: usize) {
    // Move cursor up to the start of the previous block.
    if prev_lines > 0 {
        let _ = write!(w, "\x1b[{prev_lines}A");
    }
    // Write each line with a line-clear prefix.
    for line in lines {
        let _ = writeln!(w, "\x1b[2K{line}");
    }
    // If the new block is shorter, clear leftover lines.
    if lines.len() < prev_lines {
        for _ in 0..(prev_lines - lines.len()) {
            let _ = writeln!(w, "\x1b[2K");
        }
        // Move back up past the extra cleared lines.
        let extra = prev_lines - lines.len();
        if extra > 0 {
            let _ = write!(w, "\x1b[{extra}A");
        }
    }
}

/// Clear `n` lines above the current cursor position.
fn clear_block(w: &mut dyn Write, n: usize) {
    if n > 0 {
        let _ = write!(w, "\x1b[{n}A");
        for _ in 0..n {
            let _ = writeln!(w, "\x1b[2K");
        }
        // Move back up to where we started clearing.
        let _ = write!(w, "\x1b[{n}A");
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test state with no color and a generous terminal height.
    fn test_state() -> ChecklistState {
        ChecklistState::new(false, 40)
    }

    #[test]
    fn state_initializes_all_pending() {
        let state = test_state();
        assert_eq!(state.rows.len(), 11);
        for row in &state.rows {
            assert!(matches!(row.state, RowState::Pending));
        }
    }

    #[test]
    fn state_transitions_active_and_complete() {
        let mut state = test_state();

        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        assert!(matches!(state.rows[0].state, RowState::Active));
        assert!(state.rows[0].started_at.is_some());

        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });
        assert!(matches!(state.rows[0].state, RowState::Complete { .. }));
    }

    #[test]
    fn state_transitions_skipped() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Selinux));
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Selinux,
            outcome: InspectorOutcome::Skipped {
                reason: "disabled".to_string(),
            },
        });
        assert!(matches!(state.rows[9].state, RowState::Skipped { .. }));
    }

    #[test]
    fn state_transitions_degraded() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Storage));
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Storage,
            outcome: InspectorOutcome::Degraded {
                reason: "lsblk partial".to_string(),
            },
        });
        assert!(matches!(state.rows[2].state, RowState::Degraded { .. }));
    }

    #[test]
    fn state_transitions_failed() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Containers));
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Containers,
            outcome: InspectorOutcome::Failed {
                reason: "podman not found".to_string(),
            },
        });
        assert!(matches!(state.rows[5].state, RowState::Failed { .. }));
    }

    #[test]
    fn state_transitions_interrupted() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Network));
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Network,
            outcome: InspectorOutcome::Interrupted,
        });
        assert!(matches!(state.rows[4].state, RowState::Interrupted));
    }

    #[test]
    fn sub_steps_tracked() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        state.handle_event(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
        });
        assert_eq!(state.rows[0].sub_steps.len(), 6); // pre-populated
        assert!(matches!(state.rows[0].sub_steps[0].state, RowState::Active));

        state.handle_event(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::PackagesFound,
            value: 847,
        });
        state.handle_event(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
            outcome: StepOutcome::Complete,
        });
        assert!(matches!(
            state.rows[0].sub_steps[0].state,
            RowState::Complete { .. }
        ));
        assert_eq!(
            state.rows[0].sub_steps[0].metric,
            Some((MetricKind::PackagesFound, 847))
        );
    }

    #[test]
    fn probe_disappearing_empties() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::NonRpmSoftware));

        // Start two probes
        state.handle_event(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PipPackages,
        });
        state.handle_event(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::ElfBinaries,
        });
        assert_eq!(state.rows[10].probes.len(), 2);

        // Finish one as Empty — it should disappear
        state.handle_event(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::ElfBinaries,
            outcome: ProbeOutcome::Empty,
        });
        assert_eq!(state.rows[10].probes.len(), 1);
        assert_eq!(state.rows[10].probes[0].probe, ProbeId::PipPackages);

        // Finish the other as Found
        state.handle_event(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PipPackages,
            outcome: ProbeOutcome::Found { count: 12 },
        });
        assert_eq!(state.rows[10].probes.len(), 1);
        assert!(matches!(
            state.rows[10].probes[0].state,
            ProbeRowState::Found { count: 12 }
        ));
    }

    #[test]
    fn render_lines_pending_format() {
        let state = test_state();
        let lines = state.render_lines();

        // All 11 inspectors should have lines
        assert_eq!(lines.len(), 11);

        // Pending lines use ◌ (dotted circle)
        assert!(
            lines[0].contains('\u{25cc}'),
            "expected ◌ for pending, got: {}",
            lines[0]
        );
        // Rich mode uses glyphs, not [n/total] numbering
        assert!(
            !lines[0].contains("["),
            "rich mode should not have [n/N] numbering, got: {}",
            lines[0]
        );
        assert!(
            lines[0].contains("RPM packages"),
            "expected inspector name, got: {}",
            lines[0]
        );
    }

    #[test]
    fn render_lines_active_has_spinner() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));

        let lines = state.render_lines();
        // Active line should have the spinner character (tick_count=0 → first frame)
        let spinner_char = SPINNER[0];
        assert!(
            lines[0].contains(spinner_char),
            "expected spinner char '{spinner_char}', got: {}",
            lines[0]
        );
    }

    #[test]
    fn render_lines_complete_symbols() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });

        let lines = state.render_lines();
        assert!(
            lines[0].contains('\u{2713}'),
            "expected checkmark for complete, got: {}",
            lines[0]
        );
        assert!(
            lines[0].contains("done"),
            "expected 'done' suffix, got: {}",
            lines[0]
        );
    }

    #[test]
    fn render_lines_sub_steps_indented() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        state.handle_event(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
        });
        state.handle_event(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::PackagesFound,
            value: 500,
        });
        state.handle_event(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
            outcome: StepOutcome::Complete,
        });

        let lines = state.render_lines();
        // Line 0 is the RPM inspector, lines 1..6 are 6 pre-populated sub-steps
        assert_eq!(lines.len(), 17); // 11 inspectors + 6 RPM sub-steps

        // First sub-step line should be indented with 7 spaces
        assert!(
            lines[1].starts_with("       "),
            "expected 7-space indent, got: {:?}",
            lines[1]
        );
        assert!(
            lines[1].contains("500 found"),
            "expected metric suffix, got: {}",
            lines[1]
        );
    }

    #[test]
    fn render_lines_probe_found() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::NonRpmSoftware));
        state.handle_event(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PipPackages,
        });
        state.handle_event(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PipPackages,
            outcome: ProbeOutcome::Found { count: 7 },
        });

        let lines = state.render_lines();
        // Line for the probe should show checkmark and count
        let probe_line = lines
            .iter()
            .find(|l| l.contains("pip packages"))
            .expect("should have pip packages line");
        assert!(
            probe_line.contains("7 found"),
            "expected '7 found', got: {probe_line}"
        );
        assert!(
            probe_line.contains('\u{2713}'),
            "expected checkmark, got: {probe_line}"
        );
    }

    #[test]
    fn overflow_hides_pending() {
        // With terminal_height=10, max_lines=8.
        // 11 inspectors all pending = overflow.
        let state = ChecklistState::new(false, 10);
        let lines = state.render_lines();

        // Should be truncated — fewer lines than 11
        assert!(
            lines.len() < 11,
            "expected overflow truncation, got {} lines",
            lines.len()
        );

        // Last line should be the "... and N more" footer
        let last = lines.last().expect("should have lines");
        assert!(
            last.contains("...") && last.contains("more"),
            "expected overflow footer, got: {last}"
        );
    }

    #[test]
    fn overflow_shows_active_over_pending() {
        // Terminal with 10 lines = max_lines 8.
        let mut state = ChecklistState::new(false, 10);

        // Activate the first 3 inspectors (they become Active, not Pending)
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Services));
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Storage));

        let lines = state.render_lines();

        // Active lines should appear (they have spinner)
        let active_count = lines.iter().filter(|l| l.contains(SPINNER[0])).count();
        assert_eq!(active_count, 3, "expected 3 active lines with spinners");

        // Should still have the overflow footer
        let last = lines.last().expect("should have lines");
        assert!(
            last.contains("..."),
            "expected overflow footer, got: {last}"
        );
    }

    #[test]
    fn tick_advances_spinner() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));

        let lines_0 = state.render_lines();
        state.tick();
        let lines_1 = state.render_lines();

        // The spinner character should change between ticks
        assert_ne!(
            lines_0[0], lines_1[0],
            "spinner frame should change on tick"
        );
    }

    #[test]
    fn all_outcome_symbols_rendered() {
        let mut state = test_state();

        // Complete
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });

        // Skipped
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Selinux));
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Selinux,
            outcome: InspectorOutcome::Skipped {
                reason: "disabled".to_string(),
            },
        });

        // Degraded
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Storage));
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Storage,
            outcome: InspectorOutcome::Degraded {
                reason: "partial".to_string(),
            },
        });

        // Failed
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Containers));
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Containers,
            outcome: InspectorOutcome::Failed {
                reason: "podman missing".to_string(),
            },
        });

        // Interrupted
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Network));
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Network,
            outcome: InspectorOutcome::Interrupted,
        });

        let lines = state.render_lines();
        let joined = lines.join("\n");

        assert!(joined.contains('\u{2713}'), "missing checkmark (complete)");
        assert!(joined.contains('\u{2013}'), "missing en-dash (skipped)");
        assert!(joined.contains('~'), "missing tilde (degraded)");
        assert!(joined.contains('\u{2717}'), "missing cross (failed)");
        assert!(joined.contains('\u{25a0}'), "missing square (interrupted)");
    }

    #[test]
    fn no_color_mode_has_no_ansi() {
        let mut state = ChecklistState::new(false, 40);
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });

        let lines = state.render_lines();
        let joined = lines.join("\n");
        assert!(
            !joined.contains("\x1b["),
            "found ANSI escape in no-color mode: {joined}"
        );
    }

    #[test]
    fn color_mode_has_ansi() {
        let mut state = ChecklistState::new(true, 40);
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });

        let lines = state.render_lines();
        let joined = lines.join("\n");
        assert!(
            joined.contains("\x1b["),
            "expected ANSI escape in color mode: {joined}"
        );
    }

    #[test]
    fn metric_labels_match_spec() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));

        // Step with PackagesFound
        state.handle_event(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
        });
        state.handle_event(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::PackagesFound,
            value: 847,
        });
        state.handle_event(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
            outcome: StepOutcome::Complete,
        });

        // Step with ReposMapped
        state.handle_event(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::ResolvingSourceRepos,
        });
        state.handle_event(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::ReposMapped,
            value: 8,
        });
        state.handle_event(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::ResolvingSourceRepos,
            outcome: StepOutcome::Complete,
        });

        // Finish RPM inspector — last metric was ReposMapped
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });

        let lines = state.render_lines();
        let joined = lines.join("\n");

        assert!(
            joined.contains("847 found"),
            "PackagesFound should say '847 found', got: {joined}"
        );
        assert!(
            joined.contains("8 repos mapped"),
            "ReposMapped should say '8 repos mapped', got: {joined}"
        );
        // Parent completion line should show last metric (no [n/N] numbering in rich mode)
        let rpm_line = lines
            .iter()
            .find(|l| l.contains("RPM packages") && l.contains('\u{2713}'))
            .expect("RPM done line");
        assert!(
            rpm_line.contains("8 repos mapped"),
            "parent completion should show last metric, got: {rpm_line}"
        );
        assert!(
            !rpm_line.contains("["),
            "rich mode should not have numbering, got: {rpm_line}"
        );
    }

    #[test]
    fn metric_resets_between_steps() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));

        // Step 1 with metric
        state.handle_event(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
        });
        state.handle_event(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::PackagesFound,
            value: 847,
        });
        state.handle_event(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
            outcome: StepOutcome::Complete,
        });

        // Step 2 without metric
        state.handle_event(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::ClassifyingPackages,
        });
        state.handle_event(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::ClassifyingPackages,
            outcome: StepOutcome::Complete,
        });

        // Step 1 should have metric, step 2 should say "done"
        let lines = state.render_lines();
        let step1_line = lines
            .iter()
            .find(|l| l.contains("Querying"))
            .expect("querying line");
        let step2_line = lines
            .iter()
            .find(|l| l.contains("Classifying"))
            .expect("classifying line");

        assert!(
            step1_line.contains("847 found"),
            "step 1 should have metric, got: {step1_line}"
        );
        assert!(
            step2_line.contains("done"),
            "step 2 should say 'done', got: {step2_line}"
        );
    }

    #[test]
    fn rpm_substeps_visible_upfront() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));

        let rpm_row = state
            .rows
            .iter()
            .find(|r| r.id == InspectorId::Rpm)
            .unwrap();
        assert_eq!(
            rpm_row.sub_steps.len(),
            6,
            "RPM should have 6 pre-populated sub-steps"
        );
        assert!(
            rpm_row
                .sub_steps
                .iter()
                .all(|s| matches!(s.state, RowState::Pending)),
            "all RPM sub-steps should start as Pending"
        );
    }

    #[test]
    fn config_substeps_visible_upfront() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Config));

        let cfg_row = state
            .rows
            .iter()
            .find(|r| r.id == InspectorId::Config)
            .unwrap();
        assert_eq!(
            cfg_row.sub_steps.len(),
            3,
            "Config should have 3 pre-populated sub-steps"
        );
        assert!(
            cfg_row
                .sub_steps
                .iter()
                .all(|s| matches!(s.state, RowState::Pending)),
            "all Config sub-steps should start as Pending"
        );
    }

    #[test]
    fn step_started_transitions_prepopulated_row() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));

        // StepStarted should transition the pre-populated Pending row
        // to Active, not push a duplicate.
        state.handle_event(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
        });
        assert_eq!(
            state.rows[0].sub_steps.len(),
            6,
            "should not push a duplicate"
        );
        assert!(matches!(state.rows[0].sub_steps[0].state, RowState::Active));
        // Remaining sub-steps stay Pending.
        assert!(matches!(
            state.rows[0].sub_steps[1].state,
            RowState::Pending
        ));
    }

    #[test]
    fn interrupted_parent_reconciles_children() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));

        // Complete the first step normally
        state.handle_event(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
        });
        state.handle_event(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
            outcome: StepOutcome::Complete,
        });

        // Start the second step (it becomes Active)
        state.handle_event(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::ClassifyingPackages,
        });

        // Parent finishes as Interrupted without finishing remaining steps
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Interrupted,
        });

        let rpm_row = state
            .rows
            .iter()
            .find(|r| r.id == InspectorId::Rpm)
            .unwrap();

        // Step 0 (QueryingPackages) was already Complete — stays Complete
        assert!(
            matches!(rpm_row.sub_steps[0].state, RowState::Complete { .. }),
            "completed step should stay complete"
        );
        // Step 1 (ClassifyingPackages) was Active → Interrupted
        assert!(
            matches!(rpm_row.sub_steps[1].state, RowState::Interrupted),
            "active step should become interrupted"
        );
        // Steps 2..5 were Pending → Interrupted
        for sub in &rpm_row.sub_steps[2..] {
            assert!(
                matches!(sub.state, RowState::Interrupted),
                "pending step {:?} should become interrupted, got {:?}",
                sub.step,
                sub.state
            );
        }
    }

    #[test]
    fn failed_parent_reconciles_children() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Config));

        // Start but don't finish any steps
        state.handle_event(ProgressEvent::StepStarted {
            inspector: InspectorId::Config,
            step: StepId::ApplyingRpmVerification,
        });

        // Parent fails
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Config,
            outcome: InspectorOutcome::Failed {
                reason: "rpm -Va crashed".to_string(),
            },
        });

        let cfg_row = state
            .rows
            .iter()
            .find(|r| r.id == InspectorId::Config)
            .unwrap();

        // Active step → Failed
        assert!(
            matches!(cfg_row.sub_steps[0].state, RowState::Failed { .. }),
            "active step should become failed"
        );
        // Pending steps → Failed
        for sub in &cfg_row.sub_steps[1..] {
            assert!(
                matches!(sub.state, RowState::Failed { .. }),
                "pending step {:?} should become failed, got {:?}",
                sub.step,
                sub.state
            );
        }
    }

    #[test]
    fn complete_parent_reconciles_remaining_pending() {
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));

        // Complete only the first step
        state.handle_event(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
        });
        state.handle_event(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
            outcome: StepOutcome::Complete,
        });

        // Parent completes (remaining sub-steps never fired individually)
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });

        let rpm_row = state
            .rows
            .iter()
            .find(|r| r.id == InspectorId::Rpm)
            .unwrap();

        // Remaining pending sub-steps should be swept to Complete
        for sub in &rpm_row.sub_steps[1..] {
            assert!(
                matches!(sub.state, RowState::Complete { .. }),
                "pending step {:?} should become complete on parent complete, got {:?}",
                sub.step,
                sub.state
            );
        }
    }

    #[test]
    fn non_substep_inspector_unaffected() {
        // Inspectors without pre-populated sub-steps (e.g., Services)
        // should not have any sub-steps added by InspectorStarted.
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Services));

        let svc_row = state
            .rows
            .iter()
            .find(|r| r.id == InspectorId::Services)
            .unwrap();
        assert!(
            svc_row.sub_steps.is_empty(),
            "Services should have no sub-steps"
        );
    }

    #[test]
    fn rich_no_numbering_in_output() {
        // Rich mode uses glyphs, not [n/N] numbering (that's flat mode).
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::Selinux));
        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::Selinux,
            outcome: InspectorOutcome::Skipped {
                reason: "disabled".to_string(),
            },
        });

        let lines = state.render_lines();
        for line in &lines {
            assert!(
                !line.contains("[") || line.contains("..."),
                "rich mode should not use [n/N] numbering, got: {line}"
            );
        }
    }

    #[test]
    fn nonrpm_ecosystems_count() {
        // NonRpmSoftware completion line should show "N ecosystems".
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::NonRpmSoftware));

        // Two probes find results, one is empty
        state.handle_event(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PipPackages,
        });
        state.handle_event(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PipPackages,
            outcome: ProbeOutcome::Found { count: 12 },
        });
        state.handle_event(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::NpmPackages,
        });
        state.handle_event(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::NpmPackages,
            outcome: ProbeOutcome::Found { count: 5 },
        });
        state.handle_event(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::ElfBinaries,
        });
        state.handle_event(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::ElfBinaries,
            outcome: ProbeOutcome::Empty,
        });

        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::NonRpmSoftware,
            outcome: InspectorOutcome::Complete,
        });

        let lines = state.render_lines();
        let nonrpm_line = lines
            .iter()
            .find(|l| l.contains("Non-RPM") && l.contains('\u{2713}'))
            .expect("should have NonRpmSoftware done line");
        assert!(
            nonrpm_line.contains("2 ecosystems"),
            "expected '2 ecosystems', got: {nonrpm_line}"
        );
    }

    #[test]
    fn nonrpm_zero_ecosystems() {
        // When all probes are empty, show "none found" (not "0 ecosystems").
        let mut state = test_state();
        state.handle_event(ProgressEvent::InspectorStarted(InspectorId::NonRpmSoftware));

        state.handle_event(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::ElfBinaries,
        });
        state.handle_event(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::ElfBinaries,
            outcome: ProbeOutcome::Empty,
        });

        state.handle_event(ProgressEvent::InspectorFinished {
            id: InspectorId::NonRpmSoftware,
            outcome: InspectorOutcome::Complete,
        });

        let lines = state.render_lines();
        let nonrpm_line = lines
            .iter()
            .find(|l| l.contains("Non-RPM") && l.contains('\u{2713}'))
            .expect("should have NonRpmSoftware done line");
        assert!(
            nonrpm_line.contains("none found"),
            "expected 'none found', got: {nonrpm_line}"
        );
    }
}
