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
            // A snapshot whose recorded staged file fails confinement is a
            // conflict with the session's state, not a server fault: the
            // DELETE itself is well-formed, and the upload route already
            // answers the same tampered `cache_path` with 409. The public
            // message names at most the basename the operator chose; the
            // detail, which names the upload directory, goes to the log.
            RefineError::StagedFileRefused(refusal) => {
                eprintln!("refine: {}", self.0);
                (StatusCode::CONFLICT, refusal.public_message())
            }
            // Internal errors — do not leak details
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into()),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use inspectah_core::rpm_filename::StagedName;
    use inspectah_refine::types::{StagedFileRefusal, StagedFileRefusalReason};

    async fn respond(err: RefineError) -> (StatusCode, String) {
        let response = AppError(err).into_response();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn unlisted_variants_stay_a_flat_500() {
        let detail = "remove staged file /var/lib/secret/uploads/x.rpm: denied";
        for err in [
            RefineError::TarballError(detail.into()),
            RefineError::RenderFailed(detail.into()),
            RefineError::SnapshotLoad(detail.into()),
        ] {
            let (status, body) = respond(err).await;
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(body, r#"{"error":"internal error"}"#);
        }
    }

    #[tokio::test]
    async fn a_staged_file_refusal_is_409_and_never_names_a_directory() {
        let dir = "/var/lib/secret/uploads";
        let cases = [
            (
                StagedFileRefusalReason::OutsideUploadDir,
                Some("epel-release-9-12.el9.noarch.rpm"),
                r#"{"error":"refusing to remove epel-release-9-12.el9.noarch.rpm: the snapshot records it outside this session's upload directory"}"#,
            ),
            (
                StagedFileRefusalReason::NotRegularFile,
                Some("a.rpm"),
                r#"{"error":"refusing to remove a.rpm: it is not a regular file"}"#,
            ),
            (
                StagedFileRefusalReason::InvalidName,
                None,
                r#"{"error":"refusing to remove a staged file: the snapshot records a name that is not a valid staged filename"}"#,
            ),
        ];
        for (reason, name, expected) in cases {
            let basename = name.map(|n| StagedName::parse(n).unwrap());
            let err = RefineError::StagedFileRefused(StagedFileRefusal::new(
                reason,
                basename,
                format!("something in {dir}"),
            ));
            let (status, body) = respond(err).await;
            assert_eq!(status, StatusCode::CONFLICT);
            assert_eq!(body, expected);
            assert!(!body.contains('/'), "no path separator: {body}");
        }
    }

    #[test]
    fn a_basename_with_a_separator_or_control_byte_cannot_be_named() {
        // The public message names only a `StagedName`, so these can never
        // reach a response.
        for name in ["../x.rpm", "a/b.rpm", "a\nb.rpm", "a\u{7}b.rpm", "a\rb.rpm"] {
            assert!(StagedName::parse(name).is_err(), "{name:?}");
        }
    }
}
