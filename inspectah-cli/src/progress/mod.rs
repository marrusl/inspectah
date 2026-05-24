//! Terminal progress rendering for scan output.
//!
//! Provides multiple rendering backends:
//! - [`flat::FlatRenderer`] — sequential line output for non-TTY / dumb terminals.
//! - [`plain::PlainRenderer`] — append-only output with Unicode symbols and optional ANSI color.
//! - [`rich::RichRenderer`] — block-redraw checklist with spinners and elapsed timers.

pub mod display;
pub mod flat;
pub mod plain;
pub mod rich;
