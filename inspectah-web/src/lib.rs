pub mod assets;
pub mod error;
pub mod handlers;

use axum::routing::{get, post};
use axum::Router;
use handlers::AppState;
use tower_http::cors::{AllowOrigin, CorsLayer};
use axum::http::{HeaderValue, Method};

pub fn router(state: AppState, served_origin: &str) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::exact(
            HeaderValue::from_str(served_origin).unwrap(),
        ))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([axum::http::header::CONTENT_TYPE]);

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
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024)) // 1 MiB
        .with_state(state)
}
