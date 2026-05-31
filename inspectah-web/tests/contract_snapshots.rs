// inspectah-web/tests/contract_snapshots.rs
//
// Contract snapshot tests for the key API endpoints. These hit the real HTTP
// handler stack via tower::ServiceExt::oneshot, so the serialized JSON shape
// is captured exactly as a client would see it.
//
// Any change to the response shape (field added, removed, renamed, retyped)
// breaks a snapshot and requires explicit `cargo insta review` acceptance.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::containers::{ContainerSection, QuadletUnit, RunningContainer};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection, VersionChange};
use inspectah_core::types::services::{
    PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
};
use inspectah_refine::session::RefineSession;
use inspectah_web::handlers::AppState;
use std::sync::{Arc, Mutex, OnceLock};
use tower::ServiceExt;

/// Build a snapshot with enough populated sections to exercise the contract:
/// - os_release (hostname, OS info)
/// - one service state change
/// - one version change (requires rpm section with base_version data)
/// - one running container
fn contract_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();

    snap.meta.insert(
        "hostname".to_string(),
        serde_json::json!("contract-host.example.com"),
    );

    snap.os_release = Some(OsRelease {
        name: "Red Hat Enterprise Linux".into(),
        version: "9.4 (Plow)".into(),
        version_id: "9.4".into(),
        id: "rhel".into(),
        pretty_name: "Red Hat Enterprise Linux 9.4 (Plow)".into(),
        ..Default::default()
    });

    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            version: "2.4.57".into(),
            release: "11.el9".into(),
            state: PackageState::Added,
            include: true,
            ..Default::default()
        }],
        version_changes: vec![VersionChange {
            name: "openssl".into(),
            arch: "x86_64".into(),
            host_version: "3.0.7-28.el9".into(),
            base_version: "3.0.7-27.el9".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    snap.services = Some(ServiceSection {
        state_changes: vec![ServiceStateChange {
            unit: "httpd.service".into(),
            current_state: ServiceUnitState::Enabled,
            default_state: Some(PresetDefault::Disable),
            include: false,
            owning_package: None,
            fleet: None,
            attention_reason: None,
        }],
        ..Default::default()
    });

    snap.containers = Some(ContainerSection {
        quadlet_units: vec![QuadletUnit {
            name: "myapp.container".into(),
            image: "quay.io/myorg/myapp:latest".into(),
            path: "/etc/containers/systemd/myapp.container".into(),
            content: "[Container]\nImage=quay.io/myorg/myapp:latest".into(),
            ..Default::default()
        }],
        running_containers: vec![RunningContainer {
            id: "abc123".into(),
            name: "redis".into(),
            image: "redis:7".into(),
            status: "running".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    snap
}

fn contract_state() -> Arc<AppState> {
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(contract_snapshot()))),
        sections_cache: OnceLock::new(),
    })
}

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

#[tokio::test]
async fn get_view_contract() {
    let state = contract_state();
    let app = app(state);
    let (status, json) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);
    insta::assert_json_snapshot!("contract_view", json);
}

#[tokio::test]
async fn get_sections_contract() {
    let state = contract_state();
    let app = app(state);
    let (status, json) = get_json(&app, "/api/snapshot/sections").await;
    assert_eq!(status, StatusCode::OK);
    insta::assert_json_snapshot!("contract_sections", json);
}
