//! The crate's only integration test binary.
//!
//! The manifest sets `autotests = false`, so Cargo compiles no file in
//! `tests/` on its own: every test file must be declared here as a module,
//! or it never builds or runs. `test_files_declared` fails when one is
//! missing.

mod helpers;

#[path = "../../core/tests/support/test_files_declared.rs"]
mod test_files_declared;

mod aggregate_diff_test;
mod aggregate_e2e_test;
mod aggregate_export_test;
mod aggregate_session_test;
mod aggregate_types_test;
mod aggregate_variant_ops_test;
mod attention_test;
mod autosave_integration_test;
mod autosave_test;
mod containerfile_export_parity_test;
mod cross_crate_integration_test;
mod dependency_presence_test;
mod disabled_repo_gate_test;
mod export_contract_test;
mod export_parity_test;
mod gpgkey_reference_test;
mod normalize_test;
mod repo_exclusion_carry_test;
mod repo_index_test;
mod serde_test;
mod session_test;
mod source_repo_gate_test;
mod tarball_test;
mod testdata_tarballs_load;
mod types_test;
mod upload_export_test;
mod users_integration_test;
