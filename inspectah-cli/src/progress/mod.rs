//! Terminal progress rendering for scan output.
//!
//! Provides multiple rendering backends:
//! - [`flat::FlatRenderer`] — sequential line output for non-TTY / dumb terminals.

pub mod display;
pub mod flat;
