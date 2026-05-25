//! Plain (append-only) progress renderer.
//!
//! Writes sequential lines with Unicode symbol prefixes to an arbitrary
//! [`Write`] sink.  Uses ANSI color when `use_color` is true; otherwise
//! emits the same symbols without escape codes.  Screen-reader-friendly,
//! multiplexer-recording-compatible, and safe for terminal scrollback.

use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;

use inspectah_core::types::completeness::InspectorId;
use inspectah_core::types::progress::{
    InspectorOutcome, MetricKind, ProbeOutcome, ProgressEvent, StepOutcome,
};

use super::display;

// ── ANSI helpers ────────────────────────────────────────────────────

const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// Wrap `symbol` in ANSI color codes when color is enabled.
fn colored(symbol: &str, code: &str, use_color: bool) -> String {
    if use_color {
        format!("{code}{symbol}{RESET}")
    } else {
        symbol.to_string()
    }
}

// ── Renderer ────────────────────────────────────────────────────────

/// Plain-mode progress renderer for append-only TTY output.
///
/// Thread-safe via internal [`Mutex`] — progress events may arrive
/// from parallel inspector threads (wave-2 concurrency).
pub struct PlainRenderer {
    inner: Mutex<PlainState>,
}

struct PlainState {
    writer: Box<dyn Write + Send>,
    use_color: bool,
    start_times: HashMap<InspectorId, Instant>,
    /// Per-inspector transient metric for the current step — consumed by StepFinished.
    step_metrics: HashMap<InspectorId, (MetricKind, usize)>,
    /// Per-inspector last metric — used by the parent completion line.
    inspector_metrics: HashMap<InspectorId, (MetricKind, usize)>,
    /// Per-inspector count of probes that found results — for "N ecosystems".
    probes_found: HashMap<InspectorId, usize>,
}

impl PlainRenderer {
    /// Create a new plain renderer writing to `writer`.
    ///
    /// When `use_color` is true, outcome symbols are wrapped in ANSI
    /// color codes.  When false, the same symbols are emitted plain.
    pub fn new(writer: Box<dyn Write + Send>, use_color: bool) -> Self {
        Self {
            inner: Mutex::new(PlainState {
                writer,
                use_color,
                start_times: HashMap::new(),
                step_metrics: HashMap::new(),
                inspector_metrics: HashMap::new(),
                probes_found: HashMap::new(),
            }),
        }
    }

    /// Handle a progress event, writing output to the underlying writer.
    pub fn handle(&self, event: ProgressEvent) {
        let mut state = self.inner.lock().expect("PlainRenderer lock poisoned");
        match event {
            ProgressEvent::InspectorStarted(id) => {
                state.start_times.insert(id, Instant::now());
                state.step_metrics.remove(&id);
                state.inspector_metrics.remove(&id);
                state.probes_found.remove(&id);
                let name = display::display_name(id);
                let _ = writeln!(state.writer, "\u{25b8} {name}");
            }
            ProgressEvent::InspectorFinished { id, outcome } => {
                let name = display::display_name(id);
                let elapsed = state
                    .start_times
                    .remove(&id)
                    .map(|t| t.elapsed().as_secs_f64());
                let use_color = state.use_color;
                let insp_metric = state.inspector_metrics.remove(&id);
                let probes = state.probes_found.remove(&id);
                let (symbol, suffix) =
                    format_inspector_outcome(&outcome, elapsed, &insp_metric, probes, use_color);
                let _ = writeln!(state.writer, "{symbol} {name:<40} {suffix}");
                state.step_metrics.remove(&id);
            }
            ProgressEvent::StepStarted { step, .. } => {
                let name = display::step_name(&step);
                let _ = writeln!(state.writer, "    \u{25b8} {name}");
            }
            ProgressEvent::StepFinished { inspector, step, outcome } => {
                let name = display::step_name(&step);
                let use_color = state.use_color;
                let step_metric = state.step_metrics.remove(&inspector);
                let (symbol, suffix) =
                    format_step_outcome(&outcome, &step_metric, use_color);
                let _ = writeln!(
                    state.writer,
                    "    {symbol} {name:<36} {suffix}"
                );
            }
            ProgressEvent::Metric { inspector, kind, value } => {
                state.step_metrics.insert(inspector, (kind.clone(), value));
                state.inspector_metrics.insert(inspector, (kind, value));
            }
            ProgressEvent::ProbeStarted { inspector, probe } => {
                state.probes_found.entry(inspector).or_insert(0);
                let name = display::probe_name(&probe);
                let _ = writeln!(state.writer, "    \u{25b8} {name}");
            }
            ProgressEvent::ProbeFinished {
                inspector, probe, outcome,
            } => {
                if matches!(outcome, ProbeOutcome::Found { .. }) {
                    *state.probes_found.entry(inspector).or_insert(0) += 1;
                }
                let name = display::probe_name(&probe);
                let use_color = state.use_color;
                let (symbol, suffix) =
                    format_probe_outcome(&outcome, use_color);
                let _ = writeln!(
                    state.writer,
                    "    {symbol} {name:<36} {suffix}"
                );
            }
        }
    }
}

// ── Outcome formatting ──────────────────────────────────────────────

/// Format the symbol and suffix for an inspector finish line.
///
/// When a metric is available, the completion line uses the specific
/// metric label instead of generic "done".
fn format_inspector_outcome(
    outcome: &InspectorOutcome,
    elapsed: Option<f64>,
    last_metric: &Option<(MetricKind, usize)>,
    probes_found: Option<usize>,
    use_color: bool,
) -> (String, String) {
    match outcome {
        InspectorOutcome::Complete => {
            let sym = colored("\u{2713}", GREEN, use_color);
            let label = if let Some(count) = probes_found {
                if count == 0 {
                    "none found".to_string()
                } else {
                    format!("{count} ecosystems")
                }
            } else {
                match last_metric {
                    Some((kind, value)) => display::metric_label(kind, *value),
                    None => "done".to_string(),
                }
            };
            let suf = match elapsed {
                Some(s) => format!("{label} ({s:.1}s)"),
                None => label,
            };
            (sym, suf)
        }
        InspectorOutcome::Skipped { reason } => {
            let sym = colored("\u{2013}", DIM, use_color);
            (sym, format!("skipped ({reason})"))
        }
        InspectorOutcome::Degraded { reason } => {
            let sym = colored("~", YELLOW, use_color);
            let suf = match elapsed {
                Some(s) => format!("degraded: {reason} ({s:.1}s)"),
                None => format!("degraded: {reason}"),
            };
            (sym, suf)
        }
        InspectorOutcome::Failed { reason } => {
            let sym = colored("\u{2717}", RED, use_color);
            (sym, format!("failed: {reason}"))
        }
        InspectorOutcome::Interrupted => {
            let sym = colored("\u{25a0}", RED, use_color);
            (sym, "interrupted".to_string())
        }
    }
}

/// Format the symbol and suffix for a step finish line.
///
/// If a metric was received since the last step/inspector event,
/// it replaces the generic "done" with a count (e.g. "847 found").
fn format_step_outcome(
    outcome: &StepOutcome,
    last_metric: &Option<(MetricKind, usize)>,
    use_color: bool,
) -> (String, String) {
    match outcome {
        StepOutcome::Complete => {
            let sym = colored("\u{2713}", GREEN, use_color);
            let suf = match last_metric {
                Some((kind, value)) => display::metric_label(kind, *value),
                None => "done".to_string(),
            };
            (sym, suf)
        }
        StepOutcome::Skipped { reason } => {
            let sym = colored("\u{2013}", DIM, use_color);
            (sym, format!("skipped ({reason})"))
        }
        StepOutcome::Degraded { reason } => {
            let sym = colored("~", YELLOW, use_color);
            (sym, format!("degraded: {reason}"))
        }
        StepOutcome::Failed { reason } => {
            let sym = colored("\u{2717}", RED, use_color);
            (sym, format!("failed: {reason}"))
        }
        StepOutcome::Interrupted => {
            let sym = colored("\u{25a0}", RED, use_color);
            (sym, "interrupted".to_string())
        }
    }
}

/// Format the symbol and suffix for a probe finish line.
fn format_probe_outcome(
    outcome: &ProbeOutcome,
    use_color: bool,
) -> (String, String) {
    match outcome {
        ProbeOutcome::Found { count } => {
            let sym = colored("\u{2713}", GREEN, use_color);
            (sym, format!("{count} found"))
        }
        ProbeOutcome::Empty => {
            let sym = colored("\u{2013}", DIM, use_color);
            (sym, "none".to_string())
        }
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

    /// Create a `PlainRenderer` backed by a shared buffer.
    fn test_renderer(use_color: bool) -> (PlainRenderer, Arc<Mutex<Vec<u8>>>) {
        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let writer = SharedWriter(Arc::clone(&buf));
        let renderer = PlainRenderer::new(Box::new(writer), use_color);
        (renderer, buf)
    }

    fn output_text(buf: &Arc<Mutex<Vec<u8>>>) -> String {
        String::from_utf8(buf.lock().expect("test lock").clone())
            .expect("valid utf8")
    }

    #[test]
    fn plain_renders_started_and_done_separately() {
        let (renderer, buf) = test_renderer(false);

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });

        let text = output_text(&buf);
        let lines: Vec<&str> = text.lines().collect();

        // Started line uses ▸
        assert!(
            lines[0].starts_with('\u{25b8}'),
            "expected started line with ▸, got: {}", lines[0]
        );
        assert!(
            lines[0].contains("RPM packages"),
            "expected inspector name, got: {}", lines[0]
        );

        // Done line uses ✓ — separate from started
        assert!(
            lines[1].contains('\u{2713}'),
            "expected done line with ✓, got: {}", lines[1]
        );
        assert!(
            lines[1].contains("RPM packages"),
            "expected inspector name on done line, got: {}", lines[1]
        );
    }

    #[test]
    fn plain_uses_correct_symbols() {
        let (renderer, buf) = test_renderer(false);

        // Complete → ✓
        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });

        // Skipped → –
        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Selinux));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Selinux,
            outcome: InspectorOutcome::Skipped {
                reason: "disabled".to_string(),
            },
        });

        // Degraded → ~
        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Storage));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Storage,
            outcome: InspectorOutcome::Degraded {
                reason: "lsblk partial".to_string(),
            },
        });

        // Failed → ✗
        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Containers));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Containers,
            outcome: InspectorOutcome::Failed {
                reason: "podman not found".to_string(),
            },
        });

        // Interrupted → ■
        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Network));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Network,
            outcome: InspectorOutcome::Interrupted,
        });

        let text = output_text(&buf);
        assert!(text.contains('\u{2713}'), "missing ✓ for complete");
        assert!(text.contains('\u{2013}'), "missing – for skipped");
        assert!(text.contains('~'), "missing ~ for degraded");
        assert!(text.contains('\u{2717}'), "missing ✗ for failed");
        assert!(text.contains('\u{25a0}'), "missing ■ for interrupted");
    }

    #[test]
    fn plain_no_color_mode() {
        let (renderer, buf) = test_renderer(false);

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Selinux));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Selinux,
            outcome: InspectorOutcome::Skipped {
                reason: "disabled".to_string(),
            },
        });

        let text = output_text(&buf);
        // No ANSI escape codes anywhere
        assert!(
            !text.contains("\x1b["),
            "found ANSI escape in no-color mode: {text}"
        );
        // But symbols are still present
        assert!(text.contains('\u{2713}'), "missing ✓ in no-color mode");
        assert!(text.contains('\u{2013}'), "missing – in no-color mode");
    }

    #[test]
    fn plain_color_mode_has_ansi_codes() {
        let (renderer, buf) = test_renderer(true);

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });

        let text = output_text(&buf);
        // ✓ should be wrapped in green
        assert!(
            text.contains("\x1b[32m\u{2713}\x1b[0m"),
            "expected green ✓ in color mode, got: {text}"
        );
    }

    #[test]
    fn plain_renders_probes_with_empty_and_found() {
        let (renderer, buf) = test_renderer(false);

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::NonRpmSoftware));

        // Found probe
        renderer.handle(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PipPackages,
        });
        renderer.handle(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PipPackages,
            outcome: ProbeOutcome::Found { count: 12 },
        });

        // Empty probe
        renderer.handle(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::ElfBinaries,
        });
        renderer.handle(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::ElfBinaries,
            outcome: ProbeOutcome::Empty,
        });

        let text = output_text(&buf);
        // Found: ✓ with count
        assert!(
            text.contains("pip packages") && text.contains("12 found"),
            "expected pip 12 found, got: {text}"
        );
        // Empty: – with "none"
        assert!(
            text.contains("ELF binaries") && text.contains("none"),
            "expected ELF none, got: {text}"
        );
    }

    #[test]
    fn plain_metric_labels_match_spec() {
        let (renderer, buf) = test_renderer(false);

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        renderer.handle(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
        });
        renderer.handle(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::PackagesFound,
            value: 847,
        });
        renderer.handle(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
            outcome: StepOutcome::Complete,
        });
        renderer.handle(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::ResolvingSourceRepos,
        });
        renderer.handle(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::ReposMapped,
            value: 8,
        });
        renderer.handle(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::ResolvingSourceRepos,
            outcome: StepOutcome::Complete,
        });
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });

        let text = output_text(&buf);
        assert!(
            text.contains("847 found"),
            "PackagesFound should say '847 found', got: {text}"
        );
        assert!(
            text.contains("8 repos mapped"),
            "ReposMapped should say '8 repos mapped', got: {text}"
        );
    }

    #[test]
    fn plain_renders_sub_steps_indented() {
        let (renderer, buf) = test_renderer(false);

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        renderer.handle(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
        });
        renderer.handle(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::PackagesFound,
            value: 847,
        });
        renderer.handle(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
            outcome: StepOutcome::Complete,
        });

        let text = output_text(&buf);
        let lines: Vec<&str> = text.lines().collect();

        // Inspector line is NOT indented
        assert!(
            lines[0].starts_with('\u{25b8}'),
            "inspector line should not be indented, got: {}", lines[0]
        );
        // Step lines ARE indented (4 spaces)
        assert!(
            lines[1].starts_with("    "),
            "step line should be indented, got: {}", lines[1]
        );
        assert!(
            lines[2].starts_with("    "),
            "step done line should be indented, got: {}", lines[2]
        );
        // Step done has metric
        assert!(
            lines[2].contains("847 found"),
            "expected metric on step done, got: {}", lines[2]
        );
    }

    #[test]
    fn plain_metric_resets_between_inspectors() {
        let (renderer, buf) = test_renderer(false);

        // First inspector with metric
        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        renderer.handle(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
        });
        renderer.handle(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::PackagesFound,
            value: 500,
        });
        renderer.handle(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
            outcome: StepOutcome::Complete,
        });
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });

        // Second inspector: step without metric should say "done"
        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Services));
        renderer.handle(ProgressEvent::StepStarted {
            inspector: InspectorId::Services,
            step: StepId::QueryingPackages,
        });
        renderer.handle(ProgressEvent::StepFinished {
            inspector: InspectorId::Services,
            step: StepId::QueryingPackages,
            outcome: StepOutcome::Complete,
        });

        let text = output_text(&buf);
        let lines: Vec<&str> = text.lines().collect();

        // Find Services step done line
        let services_step = lines
            .iter()
            .skip_while(|l| !l.contains("Services"))
            .find(|l| {
                l.contains("Querying installed packages")
                    && (l.contains("done") || l.contains("found"))
            })
            .expect("should find services step finish line");
        assert!(
            services_step.contains("done"),
            "expected 'done' not metric, got: {services_step}"
        );
    }

    #[test]
    fn plain_step_degraded() {
        let (renderer, buf) = test_renderer(false);

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        renderer.handle(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::VerifyingIntegrity,
        });
        renderer.handle(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::VerifyingIntegrity,
            outcome: StepOutcome::Degraded {
                reason: "rpm -V timed out".to_string(),
            },
        });

        let text = output_text(&buf);
        assert!(
            text.contains('~'),
            "expected ~ for degraded step, got: {text}"
        );
        assert!(
            text.contains("degraded: rpm -V timed out"),
            "expected degraded reason, got: {text}"
        );
    }

    #[test]
    fn plain_nonrpm_ecosystems_count() {
        let (renderer, buf) = test_renderer(false);

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::NonRpmSoftware));
        renderer.handle(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PipPackages,
        });
        renderer.handle(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PipPackages,
            outcome: ProbeOutcome::Found { count: 12 },
        });
        renderer.handle(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::NpmPackages,
        });
        renderer.handle(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::NpmPackages,
            outcome: ProbeOutcome::Found { count: 5 },
        });
        renderer.handle(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::ElfBinaries,
        });
        renderer.handle(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::ElfBinaries,
            outcome: ProbeOutcome::Empty,
        });
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::NonRpmSoftware,
            outcome: InspectorOutcome::Complete,
        });

        let text = output_text(&buf);
        // The parent completion line should show "2 ecosystems"
        let lines: Vec<&str> = text.lines().collect();
        let done_line = lines
            .iter()
            .find(|l| l.contains("Non-RPM") && l.contains('\u{2713}'))
            .expect("should find NonRpmSoftware done line");
        assert!(
            done_line.contains("2 ecosystems"),
            "expected '2 ecosystems', got: {done_line}"
        );
    }

    #[test]
    fn plain_nonrpm_zero_result_shows_none_found() {
        let (renderer, buf) = test_renderer(false);

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::NonRpmSoftware));
        renderer.handle(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::ElfBinaries,
        });
        renderer.handle(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::ElfBinaries,
            outcome: ProbeOutcome::Empty,
        });
        renderer.handle(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PipPackages,
        });
        renderer.handle(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PipPackages,
            outcome: ProbeOutcome::Empty,
        });
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::NonRpmSoftware,
            outcome: InspectorOutcome::Complete,
        });

        let text = output_text(&buf);
        let done_line = text
            .lines()
            .find(|l| l.contains("Non-RPM") && l.contains('\u{2713}'))
            .expect("should find NonRpmSoftware done line");
        assert!(
            done_line.contains("none found"),
            "expected 'none found', got: {done_line}"
        );
    }

    #[test]
    fn plain_per_inspector_metric_isolation() {
        // Two inspectors active at once — RPM metric must not leak to Services.
        let (renderer, buf) = test_renderer(false);

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Services));

        renderer.handle(ProgressEvent::StepStarted {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
        });
        renderer.handle(ProgressEvent::Metric {
            inspector: InspectorId::Rpm,
            kind: MetricKind::PackagesFound,
            value: 500,
        });
        renderer.handle(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::QueryingPackages,
            outcome: StepOutcome::Complete,
        });

        // Services finishes without any metric
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Services,
            outcome: InspectorOutcome::Complete,
        });

        let text = output_text(&buf);
        let lines: Vec<&str> = text.lines().collect();
        let svc_done = lines
            .iter()
            .find(|l| l.contains("Services") && l.contains('\u{2713}'))
            .expect("services done line");
        assert!(
            svc_done.contains("done"),
            "Services should say 'done' not inherit RPM metric, got: {svc_done}"
        );
    }

    #[test]
    fn plain_color_symbols_per_outcome() {
        let (renderer, buf) = test_renderer(true);

        // Skipped → dim
        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Selinux));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Selinux,
            outcome: InspectorOutcome::Skipped {
                reason: "disabled".to_string(),
            },
        });

        // Degraded → yellow
        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Storage));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Storage,
            outcome: InspectorOutcome::Degraded {
                reason: "partial".to_string(),
            },
        });

        // Failed → red
        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Containers));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Containers,
            outcome: InspectorOutcome::Failed {
                reason: "missing".to_string(),
            },
        });

        let text = output_text(&buf);
        assert!(
            text.contains("\x1b[2m\u{2013}\x1b[0m"),
            "expected dim – for skipped"
        );
        assert!(
            text.contains("\x1b[33m~\x1b[0m"),
            "expected yellow ~ for degraded"
        );
        assert!(
            text.contains("\x1b[31m\u{2717}\x1b[0m"),
            "expected red ✗ for failed"
        );
    }
}
