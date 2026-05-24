//! Terminal progress rendering for scan output.
//!
//! Provides multiple rendering backends:
//! - [`flat::FlatRenderer`] — sequential line output for non-TTY / dumb terminals.
//! - [`plain::PlainRenderer`] — append-only output with Unicode symbols and optional ANSI color.
//! - [`rich::RichRenderer`] — block-redraw checklist with spinners and elapsed timers.
//!
//! [`TerminalProgress`] is the unified dispatcher that selects a backend
//! based on CLI flags, environment variables, or TTY auto-detection.

use std::sync::Mutex;

use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::progress::ProgressEvent;

pub mod display;
pub mod flat;
pub mod plain;
pub mod rich;

// ── Mode detection ──────────────────────────────────────────────────

/// Progress display mode selectable via CLI `--progress` flag.
#[derive(Clone, Debug, clap::ValueEnum)]
pub enum ProgressMode {
    /// Block-redraw checklist with spinners (default for TTY).
    Rich,
    /// Append-only lines with Unicode symbols (durable scrollback).
    Plain,
    /// Numbered sequential lines, no ANSI (CI / piped output).
    Flat,
}

/// Resolved rendering mode (internal, not user-facing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Rich,
    Plain,
    Flat,
}

/// Resolve rendering mode.
///
/// Priority: CLI flag > `INSPECTAH_PROGRESS` env > TTY auto-detect.
pub fn detect_mode(cli_flag: Option<&ProgressMode>) -> Mode {
    if let Some(flag) = cli_flag {
        return match flag {
            ProgressMode::Rich => Mode::Rich,
            ProgressMode::Plain => Mode::Plain,
            ProgressMode::Flat => Mode::Flat,
        };
    }

    if let Ok(val) = std::env::var("INSPECTAH_PROGRESS") {
        return match val.to_lowercase().as_str() {
            "plain" => Mode::Plain,
            "flat" => Mode::Flat,
            "rich" => Mode::Rich,
            _ => Mode::Rich,
        };
    }

    let is_tty = std::io::IsTerminal::is_terminal(&std::io::stderr());
    let is_dumb = std::env::var("TERM")
        .map(|t| t == "dumb")
        .unwrap_or(false);

    if !is_tty || is_dumb {
        Mode::Flat
    } else {
        Mode::Rich
    }
}

/// Whether to use ANSI color (independent of mode).
///
/// Respects the `NO_COLOR` convention (<https://no-color.org/>).
pub fn use_color() -> bool {
    std::env::var("NO_COLOR").is_err()
}

// ── Unified dispatcher ──────────────────────────────────────────────

/// Unified terminal progress dispatcher.
///
/// Wraps one of the three rendering backends behind [`ProgressSink`].
/// Thread-safe: the inner renderer is behind a [`Mutex`] to allow
/// `finalize()` through a shared reference (rich mode needs `&mut self`
/// internally to join the tick thread).
pub struct TerminalProgress {
    mode: Mode,
    inner: Mutex<TerminalProgressInner>,
}

enum TerminalProgressInner {
    Rich(rich::RichRenderer),
    Plain(plain::PlainRenderer),
    Flat(flat::FlatRenderer),
}

impl TerminalProgress {
    /// Create a new terminal progress dispatcher.
    ///
    /// Selects the rendering backend based on `mode` and configures
    /// color and terminal dimensions accordingly.
    pub fn new(mode: Mode, use_color: bool) -> Self {
        let inner = match mode {
            Mode::Rich => {
                let term_height = terminal_size::terminal_size()
                    .map(|(_, h)| h.0 as usize)
                    .unwrap_or(24);
                TerminalProgressInner::Rich(rich::RichRenderer::new(
                    Box::new(std::io::stderr()),
                    use_color,
                    term_height,
                ))
            }
            Mode::Plain => TerminalProgressInner::Plain(plain::PlainRenderer::new(
                Box::new(std::io::stderr()),
                use_color,
            )),
            Mode::Flat => TerminalProgressInner::Flat(flat::FlatRenderer::new(
                Box::new(std::io::stderr()),
                display::DISPLAY_ORDER.len(),
            )),
        };
        Self {
            mode,
            inner: Mutex::new(inner),
        }
    }

    /// The resolved rendering mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Finalize rendering (rich mode: stop tick thread, print scrollback).
    ///
    /// No-op for plain and flat modes.
    pub fn finalize(&self) {
        let mut inner = self.inner.lock().expect("TerminalProgress lock poisoned");
        if let TerminalProgressInner::Rich(ref mut r) = *inner {
            r.finalize();
        }
    }
}

impl ProgressSink for TerminalProgress {
    fn emit(&self, event: ProgressEvent) {
        let inner = self.inner.lock().expect("TerminalProgress lock poisoned");
        match &*inner {
            TerminalProgressInner::Rich(r) => r.handle(event),
            TerminalProgressInner::Plain(r) => r.handle(event),
            TerminalProgressInner::Flat(r) => r.handle(event),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SAFETY: These tests manipulate process-global env vars. They are
    // inherently racy when run in parallel, but Rust's test harness runs
    // them on separate threads and the env vars are unique enough to
    // avoid cross-contamination in practice. The `unsafe` blocks are
    // required by edition 2024.

    #[test]
    fn test_mode_detection_cli_flag_overrides_env() {
        // CLI flag should win even when env is set.
        unsafe { std::env::set_var("INSPECTAH_PROGRESS", "flat") };
        let mode = detect_mode(Some(&ProgressMode::Rich));
        unsafe { std::env::remove_var("INSPECTAH_PROGRESS") };
        assert_eq!(mode, Mode::Rich);
    }

    #[test]
    fn test_mode_detection_env_overrides_tty() {
        // Env var should override TTY auto-detection.
        unsafe { std::env::set_var("INSPECTAH_PROGRESS", "plain") };
        let mode = detect_mode(None);
        unsafe { std::env::remove_var("INSPECTAH_PROGRESS") };
        assert_eq!(mode, Mode::Plain);
    }

    #[test]
    fn test_mode_detection_env_flat() {
        unsafe { std::env::set_var("INSPECTAH_PROGRESS", "flat") };
        let mode = detect_mode(None);
        unsafe { std::env::remove_var("INSPECTAH_PROGRESS") };
        assert_eq!(mode, Mode::Flat);
    }

    #[test]
    fn test_mode_detection_env_unknown_defaults_rich() {
        unsafe { std::env::set_var("INSPECTAH_PROGRESS", "unknown_value") };
        let mode = detect_mode(None);
        unsafe { std::env::remove_var("INSPECTAH_PROGRESS") };
        assert_eq!(mode, Mode::Rich);
    }

    #[test]
    fn test_use_color_respects_no_color() {
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
    fn test_terminal_progress_mode_accessor() {
        let tp = TerminalProgress::new(Mode::Flat, false);
        assert_eq!(tp.mode(), Mode::Flat);
    }

    #[test]
    fn test_terminal_progress_flat_emits_without_panic() {
        use inspectah_core::types::completeness::InspectorId;

        let tp = TerminalProgress::new(Mode::Flat, false);
        tp.emit(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        tp.emit(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: inspectah_core::types::progress::InspectorOutcome::Complete,
        });
        // No panic = success.
    }

    #[test]
    fn test_terminal_progress_plain_emits_without_panic() {
        use inspectah_core::types::completeness::InspectorId;

        let tp = TerminalProgress::new(Mode::Plain, false);
        tp.emit(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        tp.emit(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: inspectah_core::types::progress::InspectorOutcome::Complete,
        });
    }

    #[test]
    fn test_terminal_progress_finalize_noop_for_flat() {
        let tp = TerminalProgress::new(Mode::Flat, false);
        tp.finalize(); // Should not panic.
    }

    #[test]
    fn test_terminal_progress_finalize_noop_for_plain() {
        let tp = TerminalProgress::new(Mode::Plain, false);
        tp.finalize(); // Should not panic.
    }
}
