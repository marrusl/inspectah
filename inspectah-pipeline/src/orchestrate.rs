//! Pipeline orchestration — chains collect -> validate -> redact -> render -> tarball.
//!
//! Provides the top-level `run_pipeline` function used by both the CLI and E2E tests.

use std::path::Path;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::inspector::{InspectionContext, Inspector};
use inspectah_core::traits::renderer::RenderContext;

use crate::collect::collect;
use crate::redaction::engine::{redact, RedactOptions};
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
    ctx: &InspectionContext<'_>,
    inspectors: &[Box<dyn Inspector>],
    output_dir: &Path,
    hostname: &str,
) -> Result<(InspectionSnapshot, std::path::PathBuf), PipelineError> {
    // Collect
    let collected = collect(ctx, inspectors);

    // Validate
    let validated = validate(collected)?;

    // Redact
    let mut snapshot = validated.state.snapshot;
    redact(&mut snapshot, &RedactOptions::default());

    // Render all artifacts
    let render_context = RenderContext { target: None };
    render::render_all(&snapshot, &render_context, output_dir)?;

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
