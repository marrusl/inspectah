pub mod aggregate;
pub mod autosave;
pub mod baseline_summary;
pub mod classify;
pub mod group_state;
pub mod normalize;
pub mod projection;
pub mod repo_index;
/// Re-exported from `inspectah-core`: the staged-filename allowlist has two
/// users on opposite sides of the crate graph. The staging boundary is here
/// in refine, and the Containerfile renderer that writes a staged name into
/// a `COPY` and a `RUN` is in pipeline, which refine depends on. One
/// allowlist for both means it lives below both.
pub use inspectah_core::rpm_filename;
pub mod session;
pub mod tarball;
pub mod types;
