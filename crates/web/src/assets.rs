use axum::http::{HeaderValue, StatusCode, header};
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

/// Fallback handler: serves embedded static files by full path, or falls back
/// to `index.html` for SPA client-side routing.
///
/// Unlike the old `serve_static` route, this receives the full URI path
/// (e.g. `/assets/index-CXipiI4o.js`) and strips only the leading `/` before
/// looking up the file in rust-embed.  This matches the `ui/dist/` directory
/// layout where assets live under `assets/`.
pub async fn serve_fallback(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try the exact embedded file first.
    if let Some(content) = StaticAssets::get(path) {
        let mime = mime_guess::from_path(path)
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
        return resp;
    }

    // No matching file — serve index.html for SPA client-side routing.
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
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
