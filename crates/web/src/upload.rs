use axum::extract::{Multipart, Path, State};
use axum::response::Json;
use std::sync::Arc;

use crate::error::AppError;
use crate::handlers::AppState;

/// Accept an uploaded RPM file and stage it for export.
///
/// The uploaded file is stored in the session's upload directory.
/// Export merges these files into repoless-packages/ alongside
/// cached RPMs from the source tarball.
///
/// Returns match status: `"matched"` with the canonical `name.arch` when the
/// upload matched a repo-less package, or `"unmatched"` when no match was found
/// (the file is still staged for manual use).
pub async fn upload_rpm(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut uploaded_count = 0u32;
    let mut matched_canonical: Option<String> = None;

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
        matched_canonical = session.mark_uploaded_rpm(&filename, &dest.to_string_lossy());

        uploaded_count += 1;
    }

    let status = if matched_canonical.is_some() {
        "matched"
    } else {
        "unmatched"
    };

    Ok(Json(serde_json::json!({
        "uploaded": uploaded_count,
        "matched": matched_canonical,
        "status": status
    })))
}

/// Remove a previously uploaded RPM file.
///
/// Accepts `name_arch` in canonical `name.arch` format (e.g. `nginx.x86_64`).
/// Reverses the effect of `upload_rpm`: deletes the staged file and clears
/// the `repoless_cached` / `cache_path` fields on the matching PackageEntry.
pub async fn delete_uploaded_rpm(
    State(state): State<Arc<AppState>>,
    Path(name_arch): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Split "name.arch" on the last dot to handle package names containing dots
    let (name, arch) = name_arch.rsplit_once('.').ok_or_else(|| {
        AppError(inspectah_refine::types::RefineError::BadRequest(format!(
            "invalid name.arch format: {name_arch}"
        )))
    })?;

    let mut session = state.session.lock().unwrap();
    let found = session.unmark_uploaded_rpm(name, arch).map_err(AppError)?;

    if !found {
        return Err(AppError(inspectah_refine::types::RefineError::NotFound(
            format!("no uploaded RPM for {name_arch}"),
        )));
    }

    Ok(Json(serde_json::json!({
        "removed": name_arch,
        "status": "removed"
    })))
}
