//! The crate's only integration test binary.
//!
//! The manifest sets `autotests = false`, so Cargo compiles no file in
//! `tests/` on its own: every test file must be declared here as a module,
//! or it never builds or runs. `test_files_declared` fails when one is
//! missing.

#[path = "../../core/tests/support/test_files_declared.rs"]
mod test_files_declared;

// The collector-driven hosts the scan tests use, shared by the modules
// below that build sessions from collector output.
#[path = "../../cli/tests/support/sysctl_hosts.rs"]
mod sysctl_hosts;

mod aggregate_api_test;
mod api_test;
mod config_advisory_resume_test;
mod contract_snapshots;
mod decision_surface_test;
mod fixture_structure_test;
mod machine_state_api_test;
mod system_tuning_fixtures_test;
