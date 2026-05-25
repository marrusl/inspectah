use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::containers::{ContainerSection, QuadletUnit};
use inspectah_core::types::fleet::{FleetPrevalence, FleetSnapshotMeta, VariantSelection};
use inspectah_core::types::services::{ServiceSection, SystemdDropIn};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::ContentHash;
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

async fn post_json(
    app: &axum::Router,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
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
    assert!(
        !fleet.is_null(),
        "fleet should not be null for fleet snapshots"
    );

    assert_eq!(fleet["host_count"], 5);
    assert_eq!(fleet["label"], "web-tier");
    assert_eq!(fleet["merged_at"], "2026-05-21T12:00:00Z");

    let hostnames = fleet["hostnames"]
        .as_array()
        .expect("hostnames should be an array");
    assert_eq!(hostnames.len(), 3);
    assert_eq!(hostnames[0], "web-01");

    // zones_active and variant_count should be present
    assert!(
        fleet.get("zones_active").is_some(),
        "zones_active should be present"
    );
    assert!(
        fleet.get("variant_count").is_some(),
        "variant_count should be present"
    );
}

#[tokio::test]
async fn single_host_health_returns_null_fleet() {
    let state = single_host_state();
    let app = app(state);
    let (status, json) = get_json(&app, "/api/health").await;

    assert_eq!(status, StatusCode::OK);

    let fleet = json.get("fleet").expect("fleet field should be present");
    assert!(
        fleet.is_null(),
        "fleet should be null for single-host snapshots"
    );
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
    assert!(
        containerfile.is_string(),
        "containerfile_preview should be a string"
    );
    assert!(
        !containerfile.as_str().unwrap().is_empty(),
        "containerfile_preview should not be empty"
    );

    // Assert: session_is_sensitive is a boolean
    let sensitive = json
        .get("session_is_sensitive")
        .expect("session_is_sensitive should be present");
    assert!(
        sensitive.is_boolean(),
        "session_is_sensitive should be a boolean"
    );

    // Assert: sections array is present
    let sections = json
        .get("sections")
        .expect("sections should be present")
        .as_array()
        .expect("sections should be an array");

    // For a 5-host fleet, zones should be grouped
    // Assert: sections have zones with consensus/near_consensus/divergent
    for section in sections {
        // Check whether this section uses zone grouping (presence of zones field)
        // or flat listing (presence of items field)
        let has_zones = section.get("zones").is_some() && !section.get("zones").unwrap().is_null();
        let has_items = section.get("items").is_some();

        // Each section should have either zones or items (or both empty if no data)
        assert!(has_zones || has_items, "section should have zones or items");

        if has_zones {
            let zones = section.get("zones").unwrap();

            // Check for zone structure
            if let Some(zones_obj) = zones.as_object() {
                // At least one of consensus/near_consensus/divergent should exist
                let has_zone_groups = zones_obj.contains_key("consensus")
                    || zones_obj.contains_key("near_consensus")
                    || zones_obj.contains_key("divergent");
                assert!(
                    has_zone_groups,
                    "zones should have at least one of consensus/near_consensus/divergent"
                );
            }
        }

        // Assert: items have item_id with {kind, key} shape
        if let Some(items) = section.get("items").and_then(|i| i.as_array()) {
            for item in items {
                let item_id = item.get("item_id").expect("item should have item_id");
                assert!(item_id.get("kind").is_some(), "item_id should have kind");
                assert!(item_id.get("key").is_some(), "item_id should have key");
            }
        }
    }

    // Assert: summary.actionable_variant_items lists config variants only
    let summary = json.get("summary").expect("summary should be present");
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
        assert_eq!(
            kind, "config",
            "actionable_variant_items should only contain config items"
        );
    }

    // Assert: summary.informational_variant_count is a number
    let informational = summary
        .get("informational_variant_count")
        .expect("informational_variant_count should be present");
    assert!(
        informational.is_number(),
        "informational_variant_count should be a number"
    );
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

    // For a 2-host fleet, sections should use flat listing (no zone grouping)
    let sections = json
        .get("sections")
        .expect("sections should be present")
        .as_array()
        .expect("sections should be an array");

    for section in sections {
        // In a small fleet, zones should be absent or null (flat listing mode)
        let zones = section.get("zones");
        let has_zones = zones.is_some() && !zones.unwrap().is_null();

        if !has_zones {
            // Assert: items should be present for flat listing
            assert!(
                section.get("items").is_some(),
                "items should be present in flat mode"
            );
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
    assert_eq!(
        error, "not a fleet session",
        "error message should indicate not a fleet session"
    );
}

// ---------------------------------------------------------------------------
// Fleet diff tests
// ---------------------------------------------------------------------------

const VARIANT_A_CONTENT: &str = "# Config A\nserver_name = web-01\nport = 8080\n";
const VARIANT_B_CONTENT: &str = "# Config A\nserver_name = web-02\nport = 9090\ntimeout = 30\n";

fn fleet_state_with_variants() -> Arc<AppState> {
    let hash_a = ContentHash::from_content(VARIANT_A_CONTENT.as_bytes());
    let hash_b = ContentHash::from_content(VARIANT_B_CONTENT.as_bytes());
    _ = (&hash_a, &hash_b); // suppress unused warnings in fixture

    let mut snap = InspectionSnapshot::new();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "web-tier".into(),
        host_count: 5,
        hostnames: vec![
            "web-01".into(),
            "web-02".into(),
            "web-03".into(),
            "web-04".into(),
            "web-05".into(),
        ],
        merged_at: "2026-05-21T12:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry {
                path: "/etc/app/config.conf".into(),
                kind: ConfigFileKind::RpmOwnedModified,
                content: VARIANT_A_CONTENT.into(),
                include: true,
                variant_selection: VariantSelection::Selected,
                fleet: Some(FleetPrevalence {
                    count: 3,
                    total: 5,
                    hosts: vec!["web-01".into(), "web-02".into(), "web-03".into()],
                }),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/app/config.conf".into(),
                kind: ConfigFileKind::RpmOwnedModified,
                content: VARIANT_B_CONTENT.into(),
                include: true,
                variant_selection: VariantSelection::Alternative,
                fleet: Some(FleetPrevalence {
                    count: 2,
                    total: 5,
                    hosts: vec!["web-04".into(), "web-05".into()],
                }),
                ..Default::default()
            },
        ],
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    })
}

#[tokio::test]
async fn fleet_diff_returns_unified_diff() {
    let state = fleet_state_with_variants();
    let app = app(state);

    let hash_a = ContentHash::from_content(VARIANT_A_CONTENT.as_bytes());
    let hash_b = ContentHash::from_content(VARIANT_B_CONTENT.as_bytes());

    let (status, json) = post_json(
        &app,
        "/api/fleet/diff",
        serde_json::json!({
            "item_id": {"kind": "Config", "key": {"path": "/etc/app/config.conf"}},
            "base": hash_a.as_str(),
            "target": hash_b.as_str(),
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);

    // Verify hashes echo back
    assert_eq!(json["base_hash"], hash_a.as_str());
    assert_eq!(json["target_hash"], hash_b.as_str());

    // Verify host lists
    let base_hosts = json["base_hosts"]
        .as_array()
        .expect("base_hosts should be array");
    assert_eq!(base_hosts.len(), 3);
    let target_hosts = json["target_hosts"]
        .as_array()
        .expect("target_hosts should be array");
    assert_eq!(target_hosts.len(), 2);

    // Verify hunks exist with changes
    let hunks = json["hunks"].as_array().expect("hunks should be array");
    assert!(!hunks.is_empty(), "should have at least one hunk");

    // Verify hunk structure
    let hunk = &hunks[0];
    assert!(
        hunk.get("base_range").is_some(),
        "hunk should have base_range"
    );
    assert!(
        hunk.get("target_range").is_some(),
        "hunk should have target_range"
    );
    let changes = hunk["changes"].as_array().expect("changes should be array");
    assert!(!changes.is_empty(), "hunk should have changes");

    // Verify change kinds are valid strings
    for change in changes {
        let kind = change["kind"].as_str().expect("kind should be string");
        assert!(
            ["equal", "delete", "insert"].contains(&kind),
            "change kind should be equal/delete/insert, got: {kind}"
        );
    }

    // Verify stats
    let stats = json.get("stats").expect("stats should be present");
    assert!(
        stats["total_changes"].as_u64().unwrap() > 0,
        "should have changes"
    );
    assert_eq!(
        stats["total_changes"].as_u64().unwrap(),
        stats["insertions"].as_u64().unwrap() + stats["deletions"].as_u64().unwrap()
    );
}

#[tokio::test]
async fn fleet_diff_422_unknown_item() {
    let state = fleet_state_with_variants();
    let app = app(state);

    let (status, json) = post_json(
        &app,
        "/api/fleet/diff",
        serde_json::json!({
            "item_id": {"kind": "Config", "key": {"path": "/etc/nonexistent.conf"}},
            "base": "abc123",
            "target": "def456",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("unknown config path"),
        "error should mention unknown config path"
    );
}

#[tokio::test]
async fn fleet_diff_422_unknown_hash() {
    let state = fleet_state_with_variants();
    let app = app(state);

    let hash_a = ContentHash::from_content(VARIANT_A_CONTENT.as_bytes());

    let (status, json) = post_json(
        &app,
        "/api/fleet/diff",
        serde_json::json!({
            "item_id": {"kind": "Config", "key": {"path": "/etc/app/config.conf"}},
            "base": hash_a.as_str(),
            "target": "nonexistent_hash",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("unknown target hash"),
        "error should mention unknown target hash"
    );
}

#[tokio::test]
async fn fleet_diff_422_binary() {
    // Build a state with binary content (contains null bytes)
    let binary_content = "binary\0content";
    let text_content = "normal text content\n";

    let mut snap = InspectionSnapshot::new();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "test".into(),
        host_count: 2,
        hostnames: vec!["h1".into(), "h2".into()],
        merged_at: "2026-05-21T12:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry {
                path: "/etc/binary.conf".into(),
                kind: ConfigFileKind::Unowned,
                content: binary_content.into(),
                include: true,
                variant_selection: VariantSelection::Selected,
                fleet: Some(FleetPrevalence {
                    count: 1,
                    total: 2,
                    hosts: vec!["h1".into()],
                }),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/binary.conf".into(),
                kind: ConfigFileKind::Unowned,
                content: text_content.into(),
                include: true,
                variant_selection: VariantSelection::Alternative,
                fleet: Some(FleetPrevalence {
                    count: 1,
                    total: 2,
                    hosts: vec!["h2".into()],
                }),
                ..Default::default()
            },
        ],
    });
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    });
    let app = app(state);

    let hash_binary = ContentHash::from_content(binary_content.as_bytes());
    let hash_text = ContentHash::from_content(text_content.as_bytes());

    let (status, json) = post_json(
        &app,
        "/api/fleet/diff",
        serde_json::json!({
            "item_id": {"kind": "Config", "key": {"path": "/etc/binary.conf"}},
            "base": hash_binary.as_str(),
            "target": hash_text.as_str(),
        }),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        json["error"].as_str().unwrap().contains("binary"),
        "error should mention binary content"
    );
}

// ---------------------------------------------------------------------------
// Informational variant tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fleet_view_informational_variants_from_quadlets_and_dropins() {
    let mut snap = InspectionSnapshot::new();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "web-tier".into(),
        host_count: 3,
        hostnames: vec!["h1".into(), "h2".into(), "h3".into()],
        merged_at: "2026-05-22T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });

    // Quadlet with 2 variants (same path, different content)
    snap.containers = Some(ContainerSection {
        quadlet_units: vec![
            QuadletUnit {
                path: "/etc/containers/systemd/app.container".into(),
                name: "app.container".into(),
                content: "[Container]\nImage=quay.io/app:v1\n".into(),
                include: true,
                variant_selection: VariantSelection::Selected,
                fleet: Some(FleetPrevalence {
                    count: 2,
                    total: 3,
                    hosts: vec!["h1".into(), "h2".into()],
                }),
                ..Default::default()
            },
            QuadletUnit {
                path: "/etc/containers/systemd/app.container".into(),
                name: "app.container".into(),
                content: "[Container]\nImage=quay.io/app:v2\n".into(),
                include: true,
                variant_selection: VariantSelection::Alternative,
                fleet: Some(FleetPrevalence {
                    count: 1,
                    total: 3,
                    hosts: vec!["h3".into()],
                }),
                ..Default::default()
            },
        ],
        ..Default::default()
    });

    // Service drop-in with 2 variants
    snap.services = Some(ServiceSection {
        drop_ins: vec![
            SystemdDropIn {
                unit: "httpd.service".into(),
                path: "/etc/systemd/system/httpd.service.d/override.conf".into(),
                content: "[Service]\nRestart=always\n".into(),
                include: true,
                variant_selection: VariantSelection::Selected,
                fleet: Some(FleetPrevalence {
                    count: 2,
                    total: 3,
                    hosts: vec!["h1".into(), "h2".into()],
                }),
            },
            SystemdDropIn {
                unit: "httpd.service".into(),
                path: "/etc/systemd/system/httpd.service.d/override.conf".into(),
                content: "[Service]\nRestart=on-failure\n".into(),
                include: true,
                variant_selection: VariantSelection::Alternative,
                fleet: Some(FleetPrevalence {
                    count: 1,
                    total: 3,
                    hosts: vec!["h3".into()],
                }),
            },
        ],
        ..Default::default()
    });

    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    });
    let app = app(state);
    let (status, json) = get_json(&app, "/api/fleet/view").await;

    assert_eq!(status, StatusCode::OK);

    let summary = json.get("summary").expect("summary should be present");
    let informational = summary["informational_variant_count"]
        .as_u64()
        .expect("informational_variant_count should be a number");
    assert!(
        informational > 0,
        "informational_variant_count should be non-zero when context items have variants, got {informational}"
    );
    // 1 quadlet path with variants + 1 drop-in path with variants = 2 items
    assert_eq!(
        informational, 2,
        "expected 2 informational items with variants (1 quadlet + 1 drop-in)"
    );

    // Verify the containers section has a quadlet item with variants
    let sections = json["sections"].as_array().expect("sections array");
    let containers = sections
        .iter()
        .find(|s| s["id"] == "containers")
        .expect("containers section should exist");

    // Find the quadlet item (may be in zones or flat items)
    let quadlet_item = find_item_in_section(containers, "Quadlet");
    assert!(
        quadlet_item.is_some(),
        "containers section should have a Quadlet item"
    );
    let quadlet = quadlet_item.unwrap();
    let variants = quadlet
        .get("variants")
        .expect("quadlet item should have variants");
    assert!(!variants.is_null(), "variants should not be null");
    assert_eq!(variants["count"], 2, "quadlet should have 2 variants");

    // Verify the services section has a drop-in item with variants
    let services = sections
        .iter()
        .find(|s| s["id"] == "services")
        .expect("services section should exist");
    let dropin_item = find_item_in_section(services, "DropIn");
    assert!(
        dropin_item.is_some(),
        "services section should have a DropIn item"
    );
    let dropin = dropin_item.unwrap();
    let dropin_variants = dropin
        .get("variants")
        .expect("drop-in item should have variants");
    assert!(
        !dropin_variants.is_null(),
        "drop-in variants should not be null"
    );
    assert_eq!(
        dropin_variants["count"], 2,
        "drop-in should have 2 variants"
    );
}

/// Find an item with the given `kind` in a section (checking both zones and flat items).
fn find_item_in_section<'a>(
    section: &'a serde_json::Value,
    kind: &str,
) -> Option<&'a serde_json::Value> {
    // Check flat items
    if let Some(items) = section.get("items").and_then(|i| i.as_array()) {
        if let Some(item) = items.iter().find(|i| i["item_id"]["kind"] == kind) {
            return Some(item);
        }
    }
    // Check zone-grouped items
    if let Some(zones) = section.get("zones").and_then(|z| z.as_object()) {
        for (_zone_name, zone_group) in zones {
            if let Some(items) = zone_group.get("items").and_then(|i| i.as_array()) {
                if let Some(item) = items.iter().find(|i| i["item_id"]["kind"] == kind) {
                    return Some(item);
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Repo groups / repo conflict tests
// ---------------------------------------------------------------------------

fn fleet_state_with_packages() -> Arc<AppState> {
    use inspectah_core::fleet::merge_snapshots;
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::rpm::{PackageEntry, PackageState, RepoFile, RpmSection};

    // Build individual per-host snapshots and pass through merge_snapshots()
    // to exercise the full vertical: merge computes → snapshot stores →
    // session copies → handler maps.
    let make_host = |hostname: &str, nginx_repo: &str, nginx_version: &str| -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.meta
            .insert("hostname".into(), serde_json::json!(hostname));
        snap.os_release = Some(OsRelease {
            version_id: "9.4".into(),
            ..Default::default()
        });
        snap.rpm = Some(RpmSection {
            packages_added: vec![
                PackageEntry {
                    name: "httpd".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    include: true,
                    ..Default::default()
                },
                PackageEntry {
                    name: "epel-release".into(),
                    arch: "noarch".into(),
                    state: PackageState::Added,
                    source_repo: "epel".into(),
                    include: true,
                    ..Default::default()
                },
                PackageEntry {
                    name: "nginx".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: nginx_repo.into(),
                    version: nginx_version.into(),
                    include: true,
                    ..Default::default()
                },
            ],
            repo_files: vec![
                RepoFile {
                    path: "/etc/yum.repos.d/centos.repo".into(),
                    content: "[baseos]\nname=CentOS BaseOS\n\n[appstream]\nname=CentOS AppStream\n"
                        .into(),
                    include: true,
                    ..Default::default()
                },
                RepoFile {
                    path: "/etc/yum.repos.d/epel.repo".into(),
                    content: "[epel]\nname=EPEL 9\n".into(),
                    include: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        snap
    };

    // nginx: epel on web-01 and web-02 (different versions), appstream on web-03
    let s1 = make_host("web-01", "epel", "1.24.0");
    let s2 = make_host("web-02", "epel", "1.25.0");
    let s3 = make_host("web-03", "appstream", "1.22.0");

    let (merged, _warnings) =
        merge_snapshots(vec![s1, s2, s3], None).expect("merge should succeed");

    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(merged))),
        sections_cache: OnceLock::new(),
    })
}

/// Collect all fleet items matching a given source_repo from all sections.
fn fleet_items_by_repo<'a>(json: &'a serde_json::Value, repo: &str) -> Vec<&'a serde_json::Value> {
    json["sections"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| {
            let mut items = Vec::new();
            if let Some(arr) = s.get("items").and_then(|i| i.as_array()) {
                items.extend(arr.iter());
            }
            if let Some(zones) = s.get("zones").and_then(|z| z.as_object()) {
                for zone_group in zones.values() {
                    if let Some(arr) = zone_group.get("items").and_then(|i| i.as_array()) {
                        items.extend(arr.iter());
                    }
                }
            }
            items
        })
        .filter(|item| item["source_repo"].as_str() == Some(repo))
        .collect()
}

#[tokio::test]
async fn fleet_view_includes_repo_groups() {
    let state = fleet_state_with_packages();
    let app = app(state);
    let (status, json) = get_json(&app, "/api/fleet/view").await;

    assert_eq!(status, StatusCode::OK);
    let repo_groups = json.get("repo_groups").unwrap().as_array().unwrap();
    assert!(!repo_groups.is_empty(), "repo_groups should not be empty");

    // Verify known repos are present
    let section_ids: Vec<&str> = repo_groups
        .iter()
        .map(|g| g["section_id"].as_str().unwrap())
        .collect();
    assert!(
        section_ids.contains(&"appstream"),
        "appstream should be in repo_groups"
    );
    assert!(
        section_ids.contains(&"epel"),
        "epel should be in repo_groups"
    );

    // Verify repo_conflict_count reflects the conflict map
    let conflict_count = json["repo_conflict_count"].as_u64().unwrap();
    assert_eq!(conflict_count, 1, "should have 1 repo conflict (nginx)");
}

#[tokio::test]
async fn fleet_view_items_have_source_repo_and_conflict() {
    let state = fleet_state_with_packages();
    let app = app(state);
    let (_, json) = get_json(&app, "/api/fleet/view").await;

    // Find the packages section
    let packages_section = json["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "packages")
        .expect("packages section should exist");

    // Get all package items (may be in zones or flat)
    let mut all_items = Vec::new();
    if let Some(items) = packages_section.get("items").and_then(|i| i.as_array()) {
        all_items.extend(items.iter());
    }
    if let Some(zones) = packages_section.get("zones").and_then(|z| z.as_object()) {
        for zone_group in zones.values() {
            if let Some(items) = zone_group.get("items").and_then(|i| i.as_array()) {
                all_items.extend(items.iter());
            }
        }
    }

    // httpd should have source_repo "appstream" and no repo_conflict
    let httpd = all_items
        .iter()
        .find(|i| {
            i["item_id"]["key"]["name_arch"]
                .as_str()
                .map(|s| s == "httpd.x86_64")
                .unwrap_or(false)
        })
        .expect("httpd.x86_64 should exist");
    assert_eq!(httpd["source_repo"], "appstream");
    assert!(
        httpd.get("repo_conflict").is_none() || httpd["repo_conflict"].is_null(),
        "httpd should not have a repo conflict"
    );

    // nginx should have source_repo and a repo_conflict array
    let nginx = all_items
        .iter()
        .find(|i| {
            i["item_id"]["key"]["name_arch"]
                .as_str()
                .map(|s| s == "nginx.x86_64")
                .unwrap_or(false)
        })
        .expect("nginx.x86_64 should exist");
    assert_eq!(nginx["source_repo"], "epel");
    let conflict = nginx["repo_conflict"].as_array().unwrap();
    assert_eq!(conflict.len(), 2, "nginx should have 2 repo sources");
    assert_eq!(conflict[0]["repo"], "epel");
    assert_eq!(conflict[0]["host_count"], 2);
    assert_eq!(conflict[1]["repo"], "appstream");
    assert_eq!(conflict[1]["host_count"], 1);
}

#[tokio::test]
async fn fleet_exclude_repo_round_trip() {
    let state = fleet_state_with_packages();
    let app = app(state);

    // 1. Initial view — epel packages included, repo enabled
    let (_, initial) = get_json(&app, "/api/fleet/view").await;
    let epel_items = fleet_items_by_repo(&initial, "epel");
    assert!(!epel_items.is_empty(), "should have epel packages");
    for item in &epel_items {
        assert_eq!(
            item["include"], true,
            "epel packages should be included initially"
        );
    }
    let epel_group = initial["repo_groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["section_id"] == "epel")
        .unwrap();
    assert_eq!(epel_group["enabled"], true);

    // 2. ExcludeRepo
    let (status, _) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "op": "ExcludeRepo",
            "target": { "section_id": "epel" }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 3. After exclude — FleetItem.include=false AND repo_groups.enabled=false
    let (_, after_exclude) = get_json(&app, "/api/fleet/view").await;
    let epel_items = fleet_items_by_repo(&after_exclude, "epel");
    for item in &epel_items {
        assert_eq!(
            item["include"], false,
            "epel packages should be excluded after ExcludeRepo"
        );
    }
    let epel_group = after_exclude["repo_groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["section_id"] == "epel")
        .unwrap();
    assert_eq!(
        epel_group["enabled"], false,
        "epel repo should be disabled after ExcludeRepo"
    );

    // 4. IncludeRepo
    let (status, _) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "op": "IncludeRepo",
            "target": { "section_id": "epel" }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 5. After include — all back to include=true, repo enabled=true
    let (_, after_include) = get_json(&app, "/api/fleet/view").await;
    let epel_items = fleet_items_by_repo(&after_include, "epel");
    for item in &epel_items {
        assert_eq!(
            item["include"], true,
            "epel packages should be re-included after IncludeRepo"
        );
    }
    let epel_group = after_include["repo_groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["section_id"] == "epel")
        .unwrap();
    assert_eq!(
        epel_group["enabled"], true,
        "epel repo should be re-enabled after IncludeRepo"
    );
}
