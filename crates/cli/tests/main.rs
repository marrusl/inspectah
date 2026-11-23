//! The crate's main integration test binary.
//!
//! The manifest sets `autotests = false`, so Cargo compiles no file in
//! `tests/` on its own: every test file must be declared here as a module,
//! or listed as its own `[[test]]` target in the manifest, or it never
//! builds or runs. `test_files_declared` fails when one is neither.
//! `units_scan_test.rs` is the one file built as its own binary.

#[path = "support/sysctl_hosts.rs"]
mod sysctl_hosts;
#[path = "support/unit_hosts.rs"]
mod unit_hosts;

#[path = "../../core/tests/support/test_files_declared.rs"]
mod test_files_declared;

mod aggregate_host_labels_test;
mod baseline_requirement_test;
mod baseline_summary_scan_test;
mod completions_sync_test;
mod copy_source_exists_test;
mod fleet_repo_precedence_test;
mod ghost_config_scan_test;
mod kargs_file_export_test;
mod machine_state_scan_test;
mod not_carried_parity_test;
mod refine_e2e_test;
mod repo_ownership_test;
mod secrets_review_test;
mod selinux_scan_test;
mod staged_writes_parity_test;
mod sysctl_scan_test;
mod tmpfiles_owner_notice_test;
mod unit_owner_evidence_test;
mod unmanaged_redaction_test;
mod users_password_export_test;
mod warning_markdown_test;
