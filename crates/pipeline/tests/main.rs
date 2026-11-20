//! The crate's only integration test binary.
//!
//! The manifest sets `autotests = false`, so Cargo compiles no file in
//! `tests/` on its own: every test file must be declared here as a module,
//! or it never builds or runs. `test_files_declared` fails when one is
//! missing.

mod selinux_support;

#[path = "../../core/tests/support/test_files_declared.rs"]
mod test_files_declared;

mod build_subscription_test;
mod comment_escape_test;
mod configtree_single_source_test;
mod e2e;
mod failure_policy;
mod failure_policy_2b;
mod instruction_injection_test;
mod not_carried_aggregate_test;
mod parallel_test;
mod redaction_2b_surfaces_test;
mod redaction_2c_surfaces_test;
mod redaction_new_surfaces_test;
mod selinux_block_shell_test;
mod smoke_render;
mod smoke_render_2b;
mod smoke_render_2c;
mod unit_plan_test;
mod users_render_test;
mod var_backing_export_test;
