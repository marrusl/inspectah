//! The crate's only integration test binary.
//!
//! The manifest sets `autotests = false`, so Cargo compiles no file in
//! `tests/` on its own: every test file must be declared here as a module,
//! or it never builds or runs. `test_files_declared` fails when one is
//! missing.

// Reads the repository's test sources, not host files.
#[allow(clippy::disallowed_methods)]
#[path = "../../core/tests/support/test_files_declared.rs"]
mod test_files_declared;

mod baseline_test;
mod config_test;
mod containers_test;
// Exercises the real executor itself, so it builds one.
#[allow(clippy::disallowed_methods)]
mod executor_boundary_test;
mod kernelboot_test;
mod network_test;
mod nonrpm_test;
mod parity_test;
mod repoless_rpm_test;
mod rpm_equivalence;
mod scheduled_test;
mod screen_test;
mod selinux_test;
mod storage_test;
mod units_test;
mod unmanaged_scan_test;
mod users_test;
