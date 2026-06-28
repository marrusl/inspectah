use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use inspectah_refine::types::RefineError;
use serde_json::json;

pub struct AppError(pub RefineError);

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self.0 {
            RefineError::UnknownTarget(t) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("unknown target: {t}"),
            ),
            RefineError::NothingToUndo => (StatusCode::CONFLICT, "nothing to undo".into()),
            RefineError::NothingToRedo => (StatusCode::CONFLICT, "nothing to redo".into()),
            RefineError::StaleGeneration { expected, actual } => (
                StatusCode::CONFLICT,
                format!("stale generation: expected {expected}, got {actual}"),
            ),
            RefineError::UntrustedSnapshot(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg.clone()),
            RefineError::ArchiveSafety(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            RefineError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            RefineError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            // Internal errors — do not leak details
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into()),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}
