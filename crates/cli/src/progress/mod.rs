//! Terminal progress rendering for scan output.
//!
//! Provides two rendering backends:
//! - [`pretty::PrettyRenderer`] — arrival-order receipt with spinners and ANSI color (TTY).
//! - [`flat::FlatRenderer`] — sequential numbered lines, no ANSI (CI / piped output).
//!
//! [`TerminalProgress`] is the unified dispatcher that selects a backend
//! based on CLI flags, environment variables, or TTY auto-detection.

use std::sync::Mutex;

use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::completeness::InspectorId;
use inspectah_core::types::progress::ProgressEvent;

pub mod display;
pub mod flat;
pub mod pretty;
pub mod receipt;

// ── Mode detection ──────────────────────────────────────────────────

/// Progress display mode selectable via CLI `--progress` flag.
#[derive(Clone, Debug, clap::ValueEnum)]
pub enum ProgressMode {
    /// Arrival-order receipt with spinners and ANSI color (default for TTY).
    Pretty,
    /// Numbered sequential lines, no ANSI (CI / piped output).
    Flat,
}

/// Resolved rendering mode (internal, not user-facing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Pretty,
    Flat,
}

/// Resolve rendering mode.
///
/// Priority: CLI flag > `INSPECTAH_PROGRESS` env > TTY auto-detect.
pub fn detect_mode(cli_flag: Option<&ProgressMode>) -> Mode {
    if let Some(flag) = cli_flag {
        return match flag {
            ProgressMode::Pretty => Mode::Pretty,
            ProgressMode::Flat => Mode::Flat,
        };
    }

    if let Ok(val) = std::env::var("INSPECTAH_PROGRESS") {
        return match val.to_lowercase().as_str() {
            "pretty" => Mode::Pretty,
            "flat" => Mode::Flat,
            _ => Mode::Pretty,
        };
    }

    let is_tty = std::io::IsTerminal::is_terminal(&std::io::stderr());
    let is_dumb = std::env::var("TERM").map(|t| t == "dumb").unwrap_or(false);

    if !is_tty || is_dumb {
        Mode::Flat
    } else {
        Mode::Pretty
    }
}

/// Whether to use ANSI color (independent of mode).
///
/// Respects the `NO_COLOR` convention (<https://no-color.org/>).
pub fn use_color() -> bool {
    std::env::var("NO_COLOR").is_err()
}

// ── Verbosity ───────────────────────────────────────────────────────

/// Verbosity level for scan progress output.
///
/// Orthogonal to [`Mode`] — controls *how much* detail the renderer
/// shows, not *which* renderer is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
    /// Suppress the scan checklist entirely; completion/warnings still print.
    Quiet,
    /// Default behavior.
    Normal,
    /// Show sub-steps for all inspectors, including fast ones.
    Verbose,
}

// ── Unified dispatcher ──────────────────────────────────────────────

/// Unified terminal progress dispatcher.
///
/// Wraps one of the two rendering backends behind [`ProgressSink`].
/// Thread-safe: the inner renderer is behind a [`Mutex`] to allow
/// `finalize()` through a shared reference (pretty mode needs `&mut self`
/// internally to join the tick thread).
pub struct TerminalProgress {
    inner: Mutex<TerminalProgressInner>,
}

enum TerminalProgressInner {
    Pretty(pretty::PrettyRenderer),
    Flat(flat::FlatRenderer),
    /// Quiet mode — swallows all events; completion block still prints.
    Null,
}

impl TerminalProgress {
    /// Create a new terminal progress dispatcher.
    ///
    /// Selects the rendering backend based on `mode` and `verbosity`.
    /// [`Verbosity::Quiet`] short-circuits to a null backend that
    /// swallows all events (completion/warnings are handled separately
    /// by the caller).
    pub fn new(mode: Mode, use_color: bool, verbosity: Verbosity, has_subscription: bool) -> Self {
        if verbosity == Verbosity::Quiet {
            return Self {
                inner: Mutex::new(TerminalProgressInner::Null),
            };
        }

        let verbose = verbosity == Verbosity::Verbose;
        let active_order = display::active_display_order(has_subscription);

        let inner = match mode {
            Mode::Pretty => TerminalProgressInner::Pretty(pretty::PrettyRenderer::new(
                Box::new(std::io::stderr()),
                use_color,
                verbose,
                active_order,
            )),
            Mode::Flat => TerminalProgressInner::Flat(flat::FlatRenderer::new(
                Box::new(std::io::stderr()),
                verbose,
                active_order,
            )),
        };
        Self {
            inner: Mutex::new(inner),
        }
    }

    /// Finalize rendering — print summary + footer via the active renderer.
    ///
    /// No-op for null mode.
    pub fn finalize(&self, scan: receipt::ScanFinalize) {
        let inner = self.inner.lock().expect("TerminalProgress lock poisoned");
        match &*inner {
            TerminalProgressInner::Pretty(r) => r.finalize(&scan),
            TerminalProgressInner::Flat(r) => r.finalize(&scan),
            TerminalProgressInner::Null => {}
        }
    }

    /// Cancel rendering (SIGINT path). Stops the tick thread without
    /// printing summary/footer — leaves the terminal as-is.
    ///
    /// No-op for flat and null modes (they don't have a tick thread).
    pub fn cancel(&self) {
        let inner = self.inner.lock().expect("TerminalProgress lock poisoned");
        match &*inner {
            TerminalProgressInner::Pretty(r) => r.cancel(),
            TerminalProgressInner::Flat(_) => {}
            TerminalProgressInner::Null => {}
        }
    }

    /// Return the set of inspector IDs that have finished (for SIGINT reconciliation).
    pub fn finished_inspectors(&self) -> Vec<InspectorId> {
        let inner = self.inner.lock().expect("TerminalProgress lock poisoned");
        match &*inner {
            TerminalProgressInner::Pretty(r) => r.receipt_lines().iter().map(|l| l.id).collect(),
            TerminalProgressInner::Flat(r) => r.receipt_lines().iter().map(|l| l.id).collect(),
            TerminalProgressInner::Null => Vec::new(),
        }
    }
}

impl ProgressSink for TerminalProgress {
    fn emit(&self, event: ProgressEvent) {
        let inner = self.inner.lock().expect("TerminalProgress lock poisoned");
        match &*inner {
            TerminalProgressInner::Pretty(r) => r.handle(event),
            TerminalProgressInner::Flat(r) => r.handle(event),
            TerminalProgressInner::Null => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SAFETY: These tests manipulate process-global env vars (`set_var` /
    // `remove_var`). Rust runs tests on separate threads, so concurrent
    // mutations race. We serialize all env-touching tests through a single
    // mutex so that only one test owns the process environment at a time.
    // The `unsafe` blocks are required by edition 2024.

    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_mode_detection_cli_flag_overrides_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        // CLI flag should win even when env is set.
        unsafe { std::env::set_var("INSPECTAH_PROGRESS", "flat") };
        let mode = detect_mode(Some(&ProgressMode::Pretty));
        unsafe { std::env::remove_var("INSPECTAH_PROGRESS") };
        assert_eq!(mode, Mode::Pretty);
    }

    #[test]
    fn test_mode_detection_env_overrides_tty() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Env var should override TTY auto-detection.
        unsafe { std::env::set_var("INSPECTAH_PROGRESS", "pretty") };
        let mode = detect_mode(None);
        unsafe { std::env::remove_var("INSPECTAH_PROGRESS") };
        assert_eq!(mode, Mode::Pretty);
    }

    #[test]
    fn test_mode_detection_env_flat() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("INSPECTAH_PROGRESS", "flat") };
        let mode = detect_mode(None);
        unsafe { std::env::remove_var("INSPECTAH_PROGRESS") };
        assert_eq!(mode, Mode::Flat);
    }

    #[test]
    fn test_mode_detection_env_unknown_defaults_pretty() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("INSPECTAH_PROGRESS", "unknown_value") };
        let mode = detect_mode(None);
        unsafe { std::env::remove_var("INSPECTAH_PROGRESS") };
        assert_eq!(mode, Mode::Pretty);
    }

    #[test]
    fn test_use_color_respects_no_color() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Without NO_COLOR, should return true.
        unsafe { std::env::remove_var("NO_COLOR") };
        assert!(use_color());

        // With NO_COLOR set, should return false.
        unsafe { std::env::set_var("NO_COLOR", "1") };
        assert!(!use_color());
        unsafe { std::env::remove_var("NO_COLOR") };
    }

    #[test]
    fn test_terminal_progress_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TerminalProgress>();
    }

    #[test]
    fn test_terminal_progress_flat_emits_without_panic() {
        use inspectah_core::types::completeness::InspectorId;

        let tp = TerminalProgress::new(Mode::Flat, false, Verbosity::Normal, false);
        tp.emit(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        tp.emit(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: inspectah_core::types::progress::InspectorOutcome::Complete,
        });
        // No panic = success.
    }

    #[test]
    fn test_terminal_progress_pretty_emits_without_panic() {
        use inspectah_core::types::completeness::InspectorId;

        let tp = TerminalProgress::new(Mode::Pretty, false, Verbosity::Normal, false);
        tp.emit(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        tp.emit(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: inspectah_core::types::progress::InspectorOutcome::Complete,
        });
    }

    #[test]
    fn test_terminal_progress_finalize_noop_for_null() {
        use receipt::{ScanEndState, ScanFinalize};

        let tp = TerminalProgress::new(Mode::Pretty, false, Verbosity::Quiet, false);
        tp.finalize(ScanFinalize {
            elapsed: std::time::Duration::from_secs(1),
            end_state: ScanEndState::InspectOnlyStdout,
            version_changes: None,
        }); // Should not panic.
    }

    #[test]
    fn test_quiet_mode_swallows_events() {
        use inspectah_core::types::completeness::InspectorId;
        use receipt::{ScanEndState, ScanFinalize};

        let tp = TerminalProgress::new(Mode::Pretty, false, Verbosity::Quiet, false);
        tp.emit(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        tp.emit(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: inspectah_core::types::progress::InspectorOutcome::Complete,
        });
        tp.finalize(ScanFinalize {
            elapsed: std::time::Duration::from_secs(1),
            end_state: ScanEndState::InspectOnlyStdout,
            version_changes: None,
        }); // No panic = success.
    }

    #[test]
    fn test_quiet_mode_cancel_noop() {
        let tp = TerminalProgress::new(Mode::Flat, false, Verbosity::Quiet, false);
        tp.cancel(); // Should not panic.
    }

    #[test]
    fn test_verbose_mode_creates_renderer() {
        use inspectah_core::types::completeness::InspectorId;

        let tp = TerminalProgress::new(Mode::Flat, false, Verbosity::Verbose, false);
        tp.emit(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        tp.emit(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: inspectah_core::types::progress::InspectorOutcome::Complete,
        });
        // No panic = renderer was created (not null).
    }

    #[test]
    fn test_finished_inspectors_tracks_completions() {
        use inspectah_core::types::completeness::InspectorId;

        let tp = TerminalProgress::new(Mode::Flat, false, Verbosity::Normal, false);
        assert!(tp.finished_inspectors().is_empty());

        tp.emit(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        tp.emit(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: inspectah_core::types::progress::InspectorOutcome::Complete,
        });

        let finished = tp.finished_inspectors();
        assert_eq!(finished.len(), 1);
        assert_eq!(finished[0], InspectorId::Rpm);
    }
}
