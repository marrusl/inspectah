//! The crate's only integration test binary.
//!
//! The manifest sets `autotests = false`, so Cargo compiles no file in
//! `tests/` on its own: every test file must be declared here as a module,
//! or it never builds or runs. `test_files_declared` fails when one is
//! missing.

#[path = "../../core/tests/support/test_files_declared.rs"]
mod test_files_declared;

mod baseline_test;
mod config_test;
mod containers_test;
mod executor_boundary_test;
mod kernelboot_test;
mod network_test;
mod nonrpm_test;
mod parity_test;
mod repoless_rpm_test;
mod rpm_equivalence;
mod scheduled_test;
mod selinux_test;
mod storage_test;
mod units_test;
mod unmanaged_scan_test;
mod users_test;
