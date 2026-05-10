use std::path::Path;

/// Render-time context — carries target and triage info.
/// Phase 1: target is None, triage_actions is empty.
/// Signature is correct for later phases from the start.
pub struct RenderContext {
    pub target: Option<crate::types::preflight::RenderTarget>,
}

pub trait Renderer: Send + Sync {
    fn name(&self) -> &str;
    fn render(
        &self,
        snapshot: &crate::snapshot::InspectionSnapshot,
        context: &RenderContext,
        output_dir: &Path,
    ) -> Result<(), RenderError>;
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("render failed: {0}")]
    Failed(String),
}
