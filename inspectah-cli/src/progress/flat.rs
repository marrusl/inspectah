//! Flat (non-TTY) progress renderer.
//!
//! Writes sequential numbered lines to an arbitrary [`Write`] sink.
//! No ANSI escapes, no cursor manipulation, no color — safe for piped
//! output, CI logs, and `$TERM=dumb` environments.

use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;

use inspectah_core::types::completeness::InspectorId;
use inspectah_core::types::progress::{
    InspectorOutcome, MetricKind, ProbeOutcome, ProgressEvent, StepOutcome,
};

use super::display;

/// Flat-mode progress renderer for non-TTY output.
///
/// Thread-safe via internal [`Mutex`] — progress events may arrive
/// from parallel inspector threads (wave-2 concurrency).
pub struct FlatRenderer {
    inner: Mutex<FlatState>,
}

struct FlatState {
    writer: Box<dyn Write + Send>,
    total: usize,
    start_times: HashMap<InspectorId, Instant>,
    /// Transient metric for the current step — consumed by StepFinished.
    last_metric: Option<(MetricKind, usize)>,
    /// Last metric seen for the current inspector — used by the parent completion line.
    inspector_metric: Option<(MetricKind, usize)>,
}

impl FlatRenderer {
    /// Create a new flat renderer writing to `writer`.
    ///
    /// `total` is the number of inspectors in the scan (used for `[N/total]`).
    pub fn new(writer: Box<dyn Write + Send>, total: usize) -> Self {
        Self {
            inner: Mutex::new(FlatState {
                writer,
                total,
                start_times: HashMap::new(),
                last_metric: None,
                inspector_metric: None,
            }),
        }
    }

    /// Handle a progress event, writing output to the underlying writer.
    pub fn handle(&self, event: ProgressEvent) {
        let mut state = self.inner.lock().expect("FlatRenderer lock poisoned");
        let total = state.total;
        match event {
            ProgressEvent::InspectorStarted(id) => {
                state.start_times.insert(id, Instant::now());
                state.last_metric = None;
                state.inspector_metric = None;
                let pos = display::display_position(id);
                let name = display::display_name(id);
                let _ = writeln!(state.writer, "[{pos}/{total}] {name}...");
            }
            ProgressEvent::InspectorFinished { id, outcome } => {
                let pos = display::display_position(id);
                let name = display::display_name(id);
                let elapsed = state
                    .start_times
                    .remove(&id)
                    .map(|t| t.elapsed().as_secs_f64());
                let suffix = format_inspector_outcome(&outcome, elapsed, &state.inspector_metric);
                let _ = writeln!(state.writer, "[{pos}/{total}] {name}... {suffix}");
                state.last_metric = None;
                state.inspector_metric = None;
            }
            ProgressEvent::StepStarted { step, .. } => {
                let name = display::step_name(&step);
                let _ = writeln!(state.writer, "  {name}...");
            }
            ProgressEvent::StepFinished { step, outcome, .. } => {
                let name = display::step_name(&step);
                let suffix = format_step_outcome(&outcome, &state.last_metric);
                let _ = writeln!(state.writer, "  {name}... {suffix}");
                state.last_metric = None;
            }
            ProgressEvent::Metric { kind, value, .. } => {
                state.last_metric = Some((kind.clone(), value));
                state.inspector_metric = Some((kind, value));
            }
            ProgressEvent::ProbeStarted { probe, .. } => {
                let name = display::probe_name(&probe);
                let _ = writeln!(state.writer, "  {name}...");
            }
            ProgressEvent::ProbeFinished {
                probe, outcome, ..
            } => {
                let name = display::probe_name(&probe);
                let suffix = format_probe_outcome(&outcome);
                let _ = writeln!(state.writer, "  {name}... {suffix}");
            }
        }
    }
}

/// Format the suffix for an inspector finish line.
///
/// When the inspector has a last metric, the completion line uses the
/// metric label instead of generic "done".
fn format_inspector_outcome(
    outcome: &InspectorOutcome,
    elapsed: Option<f64>,
    last_metric: &Option<(MetricKind, usize)>,
) -> String {
    match outcome {
        InspectorOutcome::Complete => {
            let label = match last_metric {
                Some((kind, value)) => display::metric_label(kind, *value),
                None => "done".to_string(),
            };
            match elapsed {
                Some(s) => format!("{label} ({:.1}s)", s),
                None => label,
            }
        }
        InspectorOutcome::Skipped { reason } => format!("skipped ({reason})"),
        InspectorOutcome::Degraded { reason } => match elapsed {
            Some(s) => format!("degraded: {reason} ({:.1}s)", s),
            None => format!("degraded: {reason}"),
        },
        InspectorOutcome::Failed { reason } => format!("failed: {reason}"),
        InspectorOutcome::Interrupted => "interrupted".to_string(),
    }
}

/// Format the suffix for a step finish line.
///
/// If a metric was received since the last step/inspector event,
/// it replaces the generic "done" with a count (e.g. "847 found").
fn format_step_outcome(
    outcome: &StepOutcome,
    last_metric: &Option<(MetricKind, usize)>,
) -> String {
    match outcome {
        StepOutcome::Complete => match last_metric {
            Some((kind, value)) => display::metric_label(kind, *value),
            None => "done".to_string(),
        },
        StepOutcome::Degraded { reason } => format!("degraded: {reason}"),
        StepOutcome::Failed { reason } => format!("failed: {reason}"),
        StepOutcome::Skipped { reason } => format!("skipped ({reason})"),
        StepOutcome::Interrupted => "interrupted".to_string(),
    }
}

/// Format the suffix for a probe finish line.
fn format_probe_outcome(outcome: &ProbeOutcome) -> String {
    match outcome {
        ProbeOutcome::Found { count } => format!("{count} found"),
        ProbeOutcome::Empty => "none".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::progress::{ProbeId, StepId};
    use std::sync::Arc;

    /// Helper: create a `FlatRenderer` backed by a shared `Vec<u8>`.
    fn test_renderer(total: usize) -> (FlatRenderer, Arc<Mutex<Vec<u8>>>) {
        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let writer = SharedWriter(Arc::clone(&buf));
        let renderer = FlatRenderer::new(Box::new(writer), total);
        (renderer, buf)
    }

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

    #[test]
    fn flat_renders_inspector_lifecycle() {
        let (renderer, buf) = test_renderer(11);

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });

        let text = output_text(&buf);
        assert!(text.contains("[1/11] RPM packages..."));
        assert!(text.contains("[1/11] RPM packages... done"));
    }

    #[test]
    fn flat_renders_sub_steps() {
        let (renderer, buf) = test_renderer(11);

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
            step: StepId::ClassifyingPackages,
        });
        renderer.handle(ProgressEvent::StepFinished {
            inspector: InspectorId::Rpm,
            step: StepId::ClassifyingPackages,
            outcome: StepOutcome::Complete,
        });

        let text = output_text(&buf);
        assert!(
            text.contains("Querying installed packages... 847 found"),
            "expected metric-enriched step finish, got: {text}"
        );
        assert!(
            text.contains("Classifying packages... done"),
            "expected plain done (no metric), got: {text}"
        );
    }

    #[test]
    fn flat_renders_probes() {
        let (renderer, buf) = test_renderer(11);

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::NonRpmSoftware));
        renderer.handle(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PythonVenvs,
        });
        renderer.handle(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::PythonVenvs,
            outcome: ProbeOutcome::Found { count: 3 },
        });
        renderer.handle(ProgressEvent::ProbeStarted {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::NpmPackages,
        });
        renderer.handle(ProgressEvent::ProbeFinished {
            inspector: InspectorId::NonRpmSoftware,
            probe: ProbeId::NpmPackages,
            outcome: ProbeOutcome::Empty,
        });

        let text = output_text(&buf);
        assert!(
            text.contains("Python virtualenvs... 3 found"),
            "got: {text}"
        );
        assert!(text.contains("npm packages... none"), "got: {text}");
    }

    #[test]
    fn flat_renders_skipped_failed_degraded() {
        let (renderer, buf) = test_renderer(11);

        // Skipped
        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Selinux));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Selinux,
            outcome: InspectorOutcome::Skipped {
                reason: "disabled".to_string(),
            },
        });

        // Failed
        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Containers));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Containers,
            outcome: InspectorOutcome::Failed {
                reason: "podman not found".to_string(),
            },
        });

        // Degraded
        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Storage));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Storage,
            outcome: InspectorOutcome::Degraded {
                reason: "lsblk partial".to_string(),
            },
        });

        let text = output_text(&buf);
        assert!(
            text.contains("SELinux... skipped (disabled)"),
            "got: {text}"
        );
        assert!(
            text.contains("Containers... failed: podman not found"),
            "got: {text}"
        );
        assert!(
            text.contains("Storage... degraded: lsblk partial"),
            "got: {text}"
        );
    }

    #[test]
    fn flat_renders_step_degraded() {
        let (renderer, buf) = test_renderer(11);

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
            text.contains("Verifying package integrity... degraded: rpm -V timed out"),
            "got: {text}"
        );
    }

    #[test]
    fn flat_metric_resets_between_inspectors() {
        let (renderer, buf) = test_renderer(11);

        // First inspector: metric then finish
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
            step: StepId::QueryingPackages, // reusing step for test
        });
        renderer.handle(ProgressEvent::StepFinished {
            inspector: InspectorId::Services,
            step: StepId::QueryingPackages,
            outcome: StepOutcome::Complete,
        });

        let text = output_text(&buf);
        // The second inspector's step should NOT inherit the first's metric.
        // We need the StepFinished line (has a suffix), not StepStarted.
        let lines: Vec<&str> = text.lines().collect();
        let services_step = lines
            .iter()
            .skip_while(|l| !l.contains("[2/11] Services..."))
            .find(|l| {
                l.contains("Querying installed packages...")
                    && (l.contains("done") || l.contains("found"))
            })
            .expect("should find services step finish line");
        assert!(
            services_step.contains("done"),
            "expected 'done' not metric, got: {services_step}"
        );
    }

    #[test]
    fn flat_metric_labels_match_spec() {
        let (renderer, buf) = test_renderer(11);

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        // PackagesFound step
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
        // ReposMapped step
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
        // Inspector finishes — last metric was ReposMapped
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
        // Parent completion line should show last metric
        assert!(
            text.contains("RPM packages... 8 repos mapped"),
            "parent completion should show last metric, got: {text}"
        );
    }

    #[test]
    fn flat_interrupted_outcome() {
        let (renderer, buf) = test_renderer(11);

        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Network));
        renderer.handle(ProgressEvent::InspectorFinished {
            id: InspectorId::Network,
            outcome: InspectorOutcome::Interrupted,
        });

        let text = output_text(&buf);
        assert!(
            text.contains("Network... interrupted"),
            "got: {text}"
        );
    }

    #[test]
    fn display_position_used_correctly() {
        // Services is position 2 in the display order
        let (renderer, buf) = test_renderer(11);
        renderer.handle(ProgressEvent::InspectorStarted(InspectorId::Services));

        let text = output_text(&buf);
        assert!(text.contains("[2/11]"), "got: {text}");
    }
}
