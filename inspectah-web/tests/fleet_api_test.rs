use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::fleet::FleetSnapshotMeta;
use inspectah_refine::session::RefineSession;
use inspectah_web::handlers::AppState;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};
use tower::ServiceExt;

fn app(state: Arc<AppState>) -> axum::Router {
    inspectah_web::router(state, "http://localhost:8642")
}

async fn get_json(app: &axum::Router, path: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

fn single_host_state() -> Arc<AppState> {
    let snap = InspectionSnapshot::new();
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    })
}

fn fleet_state() -> Arc<AppState> {
    let mut snap = InspectionSnapshot::new();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "web-tier".into(),
        host_count: 5,
        hostnames: vec!["web-01".into(), "web-02".into(), "web-03".into()],
        merged_at: "2026-05-21T12:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    })
}

#[tokio::test]
async fn fleet_health_returns_fleet_context() {
    let state = fleet_state();
    let app = app(state);
    let (status, json) = get_json(&app, "/api/health").await;

    assert_eq!(status, StatusCode::OK);

    let fleet = json
        .get("fleet")
        .expect("fleet field should be present for fleet snapshots");
    assert!(!fleet.is_null(), "fleet should not be null for fleet snapshots");

    assert_eq!(fleet["host_count"], 5);
    assert_eq!(fleet["label"], "web-tier");
    assert_eq!(fleet["merged_at"], "2026-05-21T12:00:00Z");

    let hostnames = fleet["hostnames"].as_array().expect("hostnames should be an array");
    assert_eq!(hostnames.len(), 3);
    assert_eq!(hostnames[0], "web-01");

    // zones_active and variant_count should be present
    assert!(fleet.get("zones_active").is_some(), "zones_active should be present");
    assert!(fleet.get("variant_count").is_some(), "variant_count should be present");
}

#[tokio::test]
async fn single_host_health_returns_null_fleet() {
    let state = single_host_state();
    let app = app(state);
    let (status, json) = get_json(&app, "/api/health").await;

    assert_eq!(status, StatusCode::OK);

    let fleet = json.get("fleet").expect("fleet field should be present");
    assert!(fleet.is_null(), "fleet should be null for single-host snapshots");
}

#[tokio::test]
async fn fleet_health_includes_session_is_sensitive() {
    let state = fleet_state();
    let app = app(state);
    let (status, json) = get_json(&app, "/api/health").await;

    assert_eq!(status, StatusCode::OK);

    let sensitive = json
        .get("session_is_sensitive")
        .expect("session_is_sensitive should be present");
    assert_eq!(sensitive, false, "fresh session should not be sensitive");
}

#[tokio::test]
async fn single_host_health_includes_session_is_sensitive() {
    let state = single_host_state();
    let app = app(state);
    let (status, json) = get_json(&app, "/api/health").await;

    assert_eq!(status, StatusCode::OK);

    let sensitive = json
        .get("session_is_sensitive")
        .expect("session_is_sensitive should be present");
    assert_eq!(sensitive, false, "fresh session should not be sensitive");
}
