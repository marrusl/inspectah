//! Pipeline orchestration — chains collect -> validate -> redact -> render -> tarball.
//!
//! Provides the top-level `run_pipeline` function used by both the CLI and E2E tests.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use inspectah_collect::inspectors::rpm::repoless::scan_dnf_cache_for_repoless;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::Inspector;
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::system::SourceSystem;

use crate::collect::collect;
use crate::redaction::engine::{RedactOptions, redact};
use crate::render;
use crate::render::tarball::{create_tarball, get_output_stamp};
use crate::validate::validate;

/// Errors from pipeline orchestration.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("validation failed: {0}")]
    Validation(#[from] crate::validate::ValidationError),
    #[error("render failed: {0}")]
    Render(#[from] inspectah_core::traits::renderer::RenderError),
    #[error("tarball failed: {0}")]
    Tarball(#[from] crate::render::tarball::TarballError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Run the full pipeline: collect -> validate -> redact -> render -> tarball.
///
/// Returns the final snapshot (post-redaction) and the path to the tarball.
pub fn run_pipeline(
    source: &SourceSystem,
    executor: &dyn Executor,
    inspectors: &[Box<dyn Inspector>],
    output_dir: &Path,
    hostname: &str,
) -> Result<(InspectionSnapshot, std::path::PathBuf), PipelineError> {
    // Collect
    let collected = collect(
        source,
        executor,
        inspectors,
        None,
        &NullProgress,
        &AtomicBool::new(false),
    );

    // Validate
    let validated = validate(collected)?;

    // Redact
    let mut snapshot = validated.state.snapshot;
    redact(&mut snapshot, &RedactOptions::default());
    // Annotate repo-less RPMs with dnf cache paths, as the scan command
    // does. This pass writes `repoless_annotation`, which the defaults
    // below read: without it a repo-less package routes by name and an
    // unneeded one is excluded instead of keeping its repo-less handling.
    if let Some(rpm) = snapshot.rpm.as_mut() {
        scan_dnf_cache_for_repoless(executor, rpm);
    }

    // Exclude auto packages nothing included needs. Must follow every
    // step that writes `repoless_annotation`, which the class reads.
    render::not_carried::apply_scan_package_defaults(&mut snapshot);

    // Render all artifacts
    let render_context = RenderContext { target: None };
    render::render_all(&snapshot, &render_context, output_dir)?;

    // Stage the cached RPMs the Containerfile's repo-less block copies,
    // as the scan command does, so no `COPY` names a missing file.
    if let Some(rpm) = snapshot.rpm.as_ref() {
        render::repoless::write_cached_rpms(rpm, output_dir)?;
    }

    // Write schema placeholder
    let schema_dir = output_dir.join("schema");
    std::fs::create_dir_all(&schema_dir)?;
    std::fs::write(
        schema_dir.join("snapshot.schema.json"),
        r#"{"$schema":"http://json-schema.org/draft-07/schema#","title":"InspectionSnapshot","description":"Phase 7 placeholder","type":"object"}"#,
    )?;

    // Create tarball
    let stamp = get_output_stamp(hostname);
    let tarball_path = output_dir
        .parent()
        .unwrap_or(output_dir)
        .join(format!("{stamp}.tar.gz"));

    create_tarball(output_dir, &tarball_path, &stamp)?;

    Ok((snapshot, tarball_path))
}
