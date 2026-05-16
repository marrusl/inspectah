pub mod assets;
pub mod error;
pub mod handlers;

use axum::routing::{get, post};
use axum::Router;
use handlers::AppState;
use tower_http::cors::{AllowOrigin, CorsLayer};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::IntoResponse;

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
                ).into_response();
            }
        }
    }
    next.run(req).await
}

pub fn router(state: AppState, served_origin: &str) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::exact(
            HeaderValue::from_str(served_origin).unwrap(),
        ))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([axum::http::header::CONTENT_TYPE]);

    let served = served_origin.to_string();

    Router::new()
        .route("/", get(assets::serve_report))
        .route("/assets/{*path}", get(assets::serve_static))
        .route("/api/health", get(handlers::health))
        .route("/api/view", get(handlers::get_view))
        .route("/api/op", post(handlers::apply_op))
        .route("/api/undo", post(handlers::undo))
        .route("/api/redo", post(handlers::redo))
        .route("/api/ops", get(handlers::get_ops))
        .route("/api/changes", get(handlers::get_changes))
        .route("/api/tarball", post(handlers::export_tarball))
        .layer(cors)
        .layer(axum::middleware::from_fn(move |req, next| {
            origin_guard(served.clone(), req, next)
        }))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024)) // 1 MiB
        .with_state(state)
}
