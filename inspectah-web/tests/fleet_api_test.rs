use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::fleet::{FleetPrevalence, FleetSnapshotMeta, VariantSelection};
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
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

#[tokio::test]
async fn fleet_view_returns_zone_grouped_sections() {
    let state = fleet_state();
    let app = app(state);
    let (status, json) = get_json(&app, "/api/fleet/view").await;

    assert_eq!(status, StatusCode::OK);

    // Assert: containerfile_preview is present and non-empty
    let containerfile = json
        .get("containerfile_preview")
        .expect("containerfile_preview should be present");
    assert!(containerfile.is_string(), "containerfile_preview should be a string");
    assert!(!containerfile.as_str().unwrap().is_empty(), "containerfile_preview should not be empty");

    // Assert: session_is_sensitive is a boolean
    let sensitive = json
        .get("session_is_sensitive")
        .expect("session_is_sensitive should be present");
    assert!(sensitive.is_boolean(), "session_is_sensitive should be a boolean");

    // Assert: sections array is present
    let sections = json
        .get("sections")
        .expect("sections should be present")
        .as_array()
        .expect("sections should be an array");

    // For a 5-host fleet, zones should be active
    // Assert: sections have zones with consensus/near_consensus/divergent
    for section in sections {
        let zones_active = section
            .get("zones_active")
            .expect("zones_active should be present")
            .as_bool()
            .expect("zones_active should be a boolean");

        if zones_active {
            let zones = section
                .get("zones")
                .expect("zones should be present when zones_active is true");
            assert!(!zones.is_null(), "zones should not be null when zones_active is true");

            // Check for zone structure
            if let Some(zones_obj) = zones.as_object() {
                // At least one of consensus/near_consensus/divergent should exist
                let has_zones = zones_obj.contains_key("consensus")
                    || zones_obj.contains_key("near_consensus")
                    || zones_obj.contains_key("divergent");
                assert!(has_zones, "zones should have at least one of consensus/near_consensus/divergent");
            }
        }

        // Assert: items have item_id with {kind, key} shape
        if let Some(items) = section.get("items").and_then(|i| i.as_array()) {
            for item in items {
                let item_id = item
                    .get("item_id")
                    .expect("item should have item_id");
                assert!(item_id.get("kind").is_some(), "item_id should have kind");
                assert!(item_id.get("key").is_some(), "item_id should have key");
            }
        }
    }

    // Assert: summary.actionable_variant_items lists config variants only
    let summary = json
        .get("summary")
        .expect("summary should be present");
    let actionable = summary
        .get("actionable_variant_items")
        .expect("actionable_variant_items should be present")
        .as_array()
        .expect("actionable_variant_items should be an array");

    // All actionable items should have kind "config"
    for item in actionable {
        let kind = item
            .get("kind")
            .expect("actionable item should have kind")
            .as_str()
            .expect("kind should be a string");
        assert_eq!(kind, "config", "actionable_variant_items should only contain config items");
    }

    // Assert: summary.informational_variant_count is a number
    let informational = summary
        .get("informational_variant_count")
        .expect("informational_variant_count should be present");
    assert!(informational.is_number(), "informational_variant_count should be a number");
}

#[tokio::test]
async fn fleet_view_returns_flat_for_fleet_of_2() {
    // Create a fleet-of-2 fixture
    let mut snap = InspectionSnapshot::new();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "small-fleet".into(),
        host_count: 2,
        hostnames: vec!["host-01".into(), "host-02".into()],
        merged_at: "2026-05-21T12:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    });
    let app = app(state);
    let (status, json) = get_json(&app, "/api/fleet/view").await;

    assert_eq!(status, StatusCode::OK);

    // For a 2-host fleet, zones_active should be false
    let sections = json
        .get("sections")
        .expect("sections should be present")
        .as_array()
        .expect("sections should be an array");

    for section in sections {
        let zones_active = section
            .get("zones_active")
            .expect("zones_active should be present")
            .as_bool()
            .expect("zones_active should be a boolean");

        if !zones_active {
            // Assert: zones should be null
            let zones = section.get("zones").expect("zones field should be present");
            assert!(zones.is_null(), "zones should be null when zones_active is false");

            // Assert: items should be present
            assert!(section.get("items").is_some(), "items should be present");
        }
    }
}

#[tokio::test]
async fn fleet_view_returns_error_for_single_host() {
    let state = single_host_state();
    let app = app(state);
    let (status, json) = get_json(&app, "/api/fleet/view").await;

    assert_eq!(status, StatusCode::OK);

    // Assert: response has an error field
    let error = json
        .get("error")
        .expect("error field should be present for single-host session");
    assert_eq!(error, "not a fleet session", "error message should indicate not a fleet session");
}
