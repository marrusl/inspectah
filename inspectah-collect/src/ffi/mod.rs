//! FFI wrappers for native library integrations.
//!
//! All unsafe code is encapsulated within these modules — no raw pointers
//! cross the module boundary. Each wrapper is feature-gated so the crate
//! compiles cleanly without the native libraries installed.

#[cfg(feature = "ffi-rpm")]
pub mod rpm;
