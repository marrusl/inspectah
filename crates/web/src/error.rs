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
            // Advisory and Inventory findings are display-only. A toggle on
            // one is a well-formed request the session is right to refuse,
            // not a server fault — and the client needs the reason to say
            // anything useful about it.
            RefineError::AdvisoryNotToggleable(t) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("advisory finding is not toggleable: {t}"),
            ),
            RefineError::InventoryNotToggleable(t) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("inventory finding is not toggleable: {t}"),
            ),
            // A group (and, by the same § 3.7 rule, a repo) op whose
            // effective members already hold the requested state, or whose
            // effective set is empty, is refused rather than silently
            // accepted with 200 and no recorded change (item 1.9).
            RefineError::NoopTarget(t) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("nothing to change: {t}"),
            ),
            // Internal errors — do not leak details
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into()),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}
