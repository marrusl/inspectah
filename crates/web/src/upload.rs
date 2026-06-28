use axum::extract::{Multipart, State};
use axum::response::Json;
use std::sync::Arc;

use crate::error::AppError;
use crate::handlers::AppState;

/// Accept an uploaded RPM file and stage it for export.
///
/// The uploaded file is stored in the session's upload directory.
/// Export merges these files into repoless-packages/ alongside
/// cached RPMs from the source tarball.
pub async fn upload_rpm(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut uploaded_count = 0u32;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        AppError(inspectah_refine::types::RefineError::BadRequest(format!(
            "multipart error: {e}"
        )))
    })? {
        let raw_filename = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown.rpm".to_string());
        // Sanitize: strip directory components to prevent path traversal
        let filename = std::path::Path::new(&raw_filename)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown.rpm".to_string());

        if !filename.ends_with(".rpm") {
            return Err(AppError(inspectah_refine::types::RefineError::BadRequest(
                format!("only .rpm files accepted, got: {filename}"),
            )));
        }

        let data = field.bytes().await.map_err(|e| {
            AppError(inspectah_refine::types::RefineError::BadRequest(format!(
                "failed to read upload: {e}"
            )))
        })?;

        let mut session = state.session.lock().unwrap();
        let upload_dir = session.ensure_upload_dir().map_err(AppError)?;
        let dest = upload_dir.join(&filename);
        std::fs::write(&dest, &data).map_err(|e| {
            AppError(inspectah_refine::types::RefineError::TarballError(format!(
                "write uploaded RPM: {e}"
            )))
        })?;

        // Mark the matching PackageEntry as cached so the renderer
        // generates active COPY/localinstall lines instead of MANUAL blocks.
        session.mark_uploaded_rpm(&filename, &dest.to_string_lossy());

        uploaded_count += 1;
    }

    Ok(Json(serde_json::json!({
        "uploaded": uploaded_count,
        "status": "staged"
    })))
}
