pub mod assets;
pub mod error;
pub mod handlers;

use axum::Router;
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use handlers::AppState;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};

/// Middleware that rejects POST requests with a mismatched Origin header.
///
/// Defense-in-depth beyond CORS preflight: even if a browser somehow
/// skips preflight, mutating requests from foreign origins are blocked.
/// Requests with no Origin header (non-browser clients like curl) are allowed.
async fn origin_guard(
    served_origin: String,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if req.method() == Method::POST {
        match req.headers().get("origin") {
            Some(origin) if origin.to_str().unwrap_or("") == served_origin => {
                // Origin matches — allow
            }
            None => {
                // No Origin header — allow (non-browser clients)
            }
            Some(_) => {
                return (
                    StatusCode::FORBIDDEN,
                    axum::response::Json(serde_json::json!({"error": "origin not allowed"})),
                )
                    .into_response();
            }
        }
    }
    next.run(req).await
}

pub fn router(state: Arc<AppState>, served_origin: &str) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::exact(
            HeaderValue::from_str(served_origin).unwrap(),
        ))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderName::from_static("x-acknowledge-sensitive"),
        ]);

    let served = served_origin.to_string();

    Router::new()
        .route("/", get(assets::serve_report))
        .route("/api/health", get(handlers::health))
        .route("/api/view", get(handlers::get_view))
        .route("/api/op", post(handlers::apply_op))
        .route("/api/undo", post(handlers::undo))
        .route("/api/redo", post(handlers::redo))
        .route("/api/ops", get(handlers::get_ops))
        .route("/api/changes", get(handlers::get_changes))
        .route("/api/tarball", post(handlers::export_tarball))
        .route("/api/user-strategy", post(handlers::user_strategy))
        .route("/api/user-password", post(handlers::user_password))
        .route("/api/user-preview", get(handlers::user_preview))
        .route("/api/snapshot/sections", get(handlers::get_sections))
        .route(
            "/api/viewed",
            get(handlers::get_viewed).post(handlers::mark_viewed),
        )
        .layer(cors)
        .layer(axum::middleware::from_fn(move |req, next| {
            origin_guard(served.clone(), req, next)
        }))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024)) // 1 MiB
        .with_state(state)
        .fallback(assets::serve_fallback)
}
