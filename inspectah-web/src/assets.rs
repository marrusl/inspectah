use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
                    img-src 'self' data:; font-src 'self'; connect-src 'self'; \
                    frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

#[derive(Embed)]
#[folder = "ui/dist/"]
pub struct StaticAssets;

pub async fn serve_report() -> Response {
    match StaticAssets::get("index.html") {
        Some(content) => {
            let mut resp = (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                content.data.to_vec(),
            )
                .into_response();
            resp.headers_mut().insert(
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static(CSP),
            );
            resp
        }
        None => (StatusCode::OK, "inspectah refine server running").into_response(),
    }
}

pub async fn serve_static(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    match StaticAssets::get(&path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string();
            let mut resp = (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime)],
                content.data.to_vec(),
            )
                .into_response();
            resp.headers_mut().insert(
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static(CSP),
            );
            resp
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
