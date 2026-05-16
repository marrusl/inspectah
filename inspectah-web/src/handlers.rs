use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::RefinementOp;
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, Mutex};

use crate::error::AppError;

pub type AppState = Arc<Mutex<RefineSession>>;

pub async fn health() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

pub async fn get_view(State(state): State<AppState>) -> impl IntoResponse {
    let session = state.lock().unwrap();
    Json(serde_json::to_value(session.view()).unwrap())
}

pub async fn apply_op(
    State(state): State<AppState>,
    Json(op): Json<RefinementOp>,
) -> Result<impl IntoResponse, AppError> {
    let mut session = state.lock().unwrap();
    session.apply(op).map_err(AppError)?;
    Ok(Json(serde_json::to_value(session.view()).unwrap()))
}

pub async fn undo(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let mut session = state.lock().unwrap();
    session.undo().map_err(AppError)?;
    Ok(Json(serde_json::to_value(session.view()).unwrap()))
}

pub async fn redo(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let mut session = state.lock().unwrap();
    session.redo().map_err(AppError)?;
    Ok(Json(serde_json::to_value(session.view()).unwrap()))
}

pub async fn get_ops(State(state): State<AppState>) -> impl IntoResponse {
    let session = state.lock().unwrap();
    Json(serde_json::to_value(session.ops_history()).unwrap())
}

pub async fn get_changes(State(state): State<AppState>) -> impl IntoResponse {
    let session = state.lock().unwrap();
    Json(serde_json::to_value(session.pending_changes()).unwrap())
}

#[derive(Deserialize)]
pub struct TarballRequest {
    pub generation: u64,
}

pub async fn export_tarball(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, AppError> {
    // Parse generation from request body — malformed JSON → 400
    let req: TarballRequest = serde_json::from_slice(&body)
        .map_err(|_| AppError(inspectah_refine::types::RefineError::BadRequest(
            "request body must be JSON with 'generation' field".into()
        )))?;

    // Snapshot state under the lock, then release before expensive work.
    // This prevents export from monopolizing the session mutex.
    let (projected, _generation) = {
        let session = state.lock().unwrap();
        if req.generation != session.generation() {
            return Err(AppError(inspectah_refine::types::RefineError::StaleGeneration {
                expected: req.generation,
                actual: session.generation(),
            }));
        }
        (session.snapshot_projected(), session.generation())
    };
    // Lock is released here.

    // Expensive render + tar work happens outside the lock via spawn_blocking.
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, inspectah_refine::types::RefineError> {
        let tempdir = tempfile::tempdir()?;
        let tarball_path = tempdir.path().join("inspectah-refine-output.tar.gz");
        // render_refine_export is a free function, not a session method
        inspectah_refine::session::render_refine_export(&projected, &tarball_path)?;
        Ok(std::fs::read(&tarball_path)?)
    })
    .await
    .map_err(|e| AppError(inspectah_refine::types::RefineError::TarballError(e.to_string())))?
    .map_err(AppError)?;

    Ok((
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, "application/gzip"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"inspectah-refine-output.tar.gz\"",
            ),
        ],
        bytes,
    ))
}
