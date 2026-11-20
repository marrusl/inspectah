//! The crate's only integration test binary.
//!
//! The manifest sets `autotests = false`, so Cargo compiles no file in
//! `tests/` on its own: every test file must be declared here as a module,
//! or it never builds or runs. `test_files_declared` fails when one is
//! missing.

#[path = "support/test_files_declared.rs"]
mod test_files_declared;

mod aggregate_e2e_test;
mod aggregate_merge_test;
mod aggregate_orchestrator_test;
mod aggregate_validate_test;
mod aggregate_zone_test;
mod snapshot_test;
mod subscription_integration_test;
