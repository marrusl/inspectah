use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::containers::{
    ComposeFile, ComposeService, ContainerSection, FlatpakApp, QuadletUnit, RunningContainer,
};
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::kernelboot::{ConfigSnippet, KernelBootSection, SysctlOverride};
use inspectah_core::types::network::{FirewallDirectRule, FirewallZone, NMConnection, NetworkSection, ProxyEntry};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::scheduled::{CronJob, ScheduledTaskSection, SystemdTimer};
use inspectah_core::types::selinux::SelinuxSection;
use inspectah_core::types::services::{ServiceSection, ServiceStateChange, SystemdDropIn};
use inspectah_core::types::storage::{FstabEntry, StorageSection};
use inspectah_core::types::users::UserGroupSection;
use inspectah_refine::session::RefineSession;
use inspectah_web::handlers::{normalize_for_context, AppState};
use std::sync::{Arc, Mutex, OnceLock};
use tower::ServiceExt;

fn test_state() -> Arc<AppState> {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true,
            ..Default::default()
        }],
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    })
}

fn app(state: Arc<AppState>) -> axum::Router {
    inspectah_web::router(state, "http://localhost:8642")
}

async fn get_json(app: &axum::Router, path: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
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
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

#[tokio::test]
async fn health_returns_ok() {
    let app = app(test_state());
    let (status, json) = get_json(&app, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn view_returns_refined_view() {
    let app = app(test_state());
    let (status, json) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.get("packages").is_some());
    assert!(json.get("stats").is_some());
    assert!(json.get("generation").is_some());
    assert_eq!(json["generation"], 0);
}

#[tokio::test]
async fn apply_valid_op() {
    let app = app(test_state());
    let (status, json) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "op": "ExcludePackage",
            "target": {"name": "httpd", "arch": "x86_64"}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["generation"], 1);
    assert_eq!(json["stats"]["excluded_packages"], 1);
}

#[tokio::test]
async fn apply_unknown_target_returns_422() {
    let app = app(test_state());
    let (status, json) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "op": "ExcludePackage",
            "target": {"name": "nonexistent", "arch": "x86_64"}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(json.get("error").is_some());
}

#[tokio::test]
async fn undo_on_fresh_session_returns_409() {
    let app = app(test_state());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/undo")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn redo_on_fresh_session_returns_409() {
    let app = app(test_state());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/redo")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn ops_returns_array() {
    let app = app(test_state());
    let (status, json) = get_json(&app, "/api/ops").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.is_array());
    assert_eq!(json.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn changes_returns_summary() {
    let app = app(test_state());
    let (status, json) = get_json(&app, "/api/changes").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["is_dirty"], false);
}

#[tokio::test]
async fn tarball_with_stale_generation_returns_409() {
    let app = app(test_state());
    let (status, json) = post_json(
        &app,
        "/api/tarball",
        serde_json::json!({"generation": 999}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(json["error"].as_str().unwrap().contains("stale generation"));
}

#[tokio::test]
async fn tarball_with_malformed_body_returns_400() {
    let app = app(test_state());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tarball")
                .header("content-type", "application/json")
                .body(Body::from("not valid json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tarball_with_empty_body_returns_400() {
    let app = app(test_state());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tarball")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn apply_malformed_json_returns_400_json() {
    let app = app(test_state());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/op")
                .header("content-type", "application/json")
                .body(Body::from("not json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.get("error").is_some(), "error response must be JSON with 'error' field");
}

#[tokio::test]
async fn evil_origin_post_rejected() {
    let app = app(test_state());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/undo")
                .header("origin", "http://evil.example.com")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn matching_origin_post_allowed() {
    let app = app(test_state());
    // Undo on fresh session returns 409 (nothing to undo), but NOT 403.
    // This proves the origin guard passed.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/undo")
                .header("origin", "http://localhost:8642")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn undo_without_json_body_returns_400() {
    let app = app(test_state());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/undo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// --- Phase 4 endpoint tests ------------------------------------------------

async fn post_raw(app: &axum::Router, path: &str, body: serde_json::Value) -> StatusCode {
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
    response.status()
}

/// Build a snapshot with representative data across all 9 context sections.
fn rich_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();

    snap.meta.insert(
        "hostname".to_string(),
        serde_json::json!("testhost.example.com"),
    );
    snap.os_release = Some(OsRelease {
        name: "Red Hat Enterprise Linux".into(),
        version: "9.4 (Plow)".into(),
        ..Default::default()
    });

    snap.services = Some(ServiceSection {
        state_changes: vec![ServiceStateChange {
            unit: "httpd.service".into(),
            current_state: "inactive".into(),
            default_state: "disabled".into(),
            action: "enable".into(),
            ..Default::default()
        }],
        drop_ins: vec![
            SystemdDropIn {
                unit: "httpd.service".into(),
                path: "/etc/systemd/system/httpd.service.d/override.conf".into(),
                content: "[Service]\nLimitNOFILE=65535".into(),
                ..Default::default()
            },
            SystemdDropIn {
                unit: "standalone.service".into(),
                path: "/etc/systemd/system/standalone.service.d/custom.conf".into(),
                content: "[Unit]\nAfter=network.target".into(),
                ..Default::default()
            },
        ],
        enabled_units: vec!["sshd.service".into()],
        disabled_units: vec!["cups.service".into()],
    });

    snap.containers = Some(ContainerSection {
        quadlet_units: vec![QuadletUnit {
            name: "myapp.container".into(),
            image: "quay.io/myorg/myapp:latest".into(),
            path: "/etc/containers/systemd/myapp.container".into(),
            content: "[Container]\nImage=quay.io/myorg/myapp:latest".into(),
            ..Default::default()
        }],
        compose_files: vec![ComposeFile {
            path: "/opt/compose/docker-compose.yml".into(),
            images: vec![ComposeService {
                service: "web".into(),
                image: "nginx:latest".into(),
            }],
            ..Default::default()
        }],
        running_containers: vec![RunningContainer {
            id: "abc123".into(),
            name: "redis".into(),
            image: "redis:7".into(),
            status: "running".into(),
            ..Default::default()
        }],
        flatpak_apps: vec![FlatpakApp {
            app_id: "org.gimp.GIMP".into(),
            origin: "flathub".into(),
            branch: "stable".into(),
            ..Default::default()
        }],
    });

    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({"name": "admin", "uid": 1000})],
        groups: vec![serde_json::json!({"name": "wheel", "gid": 10})],
        sudoers_rules: vec!["admin ALL=(ALL) NOPASSWD:ALL".into()],
        ..Default::default()
    });

    snap.network = Some(NetworkSection {
        connections: vec![NMConnection {
            name: "eth0".into(),
            conn_type: "ethernet".into(),
            method: "auto".into(),
            ..Default::default()
        }],
        firewall_zones: vec![FirewallZone {
            name: "public".into(),
            services: vec!["ssh".into(), "http".into()],
            ports: vec!["8080/tcp".into()],
            ..Default::default()
        }],
        firewall_direct_rules: vec![FirewallDirectRule {
            ipv: "ipv4".into(),
            table: "filter".into(),
            chain: "INPUT".into(),
            priority: "0".into(),
            args: "-p tcp --dport 9090 -j ACCEPT".into(),
            ..Default::default()
        }],
        hosts_additions: vec!["192.168.1.100 myhost".into()],
        proxy: vec![ProxyEntry {
            source: "/etc/environment".into(),
            line: "HTTP_PROXY=http://proxy:8080".into(),
        }],
        ..Default::default()
    });

    snap.storage = Some(StorageSection {
        fstab_entries: vec![FstabEntry {
            device: "/dev/sda1".into(),
            mount_point: "/boot".into(),
            fstype: "xfs".into(),
            options: "defaults".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    snap.scheduled_tasks = Some(ScheduledTaskSection {
        cron_jobs: vec![CronJob {
            path: "/etc/cron.d/backup".into(),
            source: "0 2 * * * root /usr/local/bin/backup.sh".into(),
            ..Default::default()
        }],
        systemd_timers: vec![SystemdTimer {
            name: "logrotate.timer".into(),
            on_calendar: "daily".into(),
            exec_start: "/usr/sbin/logrotate".into(),
            description: "Daily log rotation".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    snap.kernel_boot = Some(KernelBootSection {
        cmdline: "quiet crashkernel=auto".into(),
        sysctl_overrides: vec![SysctlOverride {
            key: "kernel.sysrq".into(),
            runtime: "16".into(),
            default: "0".into(),
            source: "/etc/sysctl.d/99-custom.conf".into(),
            ..Default::default()
        }],
        modules_load_d: vec![ConfigSnippet {
            path: "/etc/modules-load.d/custom.conf".into(),
            content: "br_netfilter".into(),
        }],
        ..Default::default()
    });

    snap.selinux = Some(SelinuxSection {
        mode: "enforcing".into(),
        fips_mode: true,
        custom_modules: vec!["my_policy".into()],
        fcontext_rules: vec!["/opt/app(/.*)? system_u:object_r:httpd_sys_content_t:s0".into()],
        ..Default::default()
    });

    snap
}

fn rich_state() -> Arc<AppState> {
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(rich_snapshot()))),
        sections_cache: OnceLock::new(),
    })
}

#[tokio::test]
async fn sections_returns_nine_sections() {
    let app = app(rich_state());
    let (status, json) = get_json(&app, "/api/snapshot/sections").await;
    assert_eq!(status, StatusCode::OK);

    let sections = json.as_array().expect("sections is an array");
    assert_eq!(sections.len(), 9, "exactly 9 context sections");

    let ids: Vec<&str> = sections
        .iter()
        .filter_map(|s| s["id"].as_str())
        .collect();
    assert!(ids.contains(&"services"));
    assert!(ids.contains(&"containers"));
    assert!(ids.contains(&"users_groups"));
    assert!(ids.contains(&"network"));
    assert!(ids.contains(&"storage"));
    assert!(ids.contains(&"scheduled_tasks"));
    assert!(ids.contains(&"non_rpm_software"));
    assert!(ids.contains(&"kernel_boot"));
    assert!(ids.contains(&"selinux"));
}

#[tokio::test]
async fn sections_items_have_required_fields() {
    let app = app(rich_state());
    let (_, json) = get_json(&app, "/api/snapshot/sections").await;

    let sections = json.as_array().unwrap();
    for section in sections {
        assert!(section.get("id").is_some(), "section must have id");
        assert!(
            section.get("display_name").is_some(),
            "section must have display_name"
        );
        let items = section["items"].as_array().expect("items must be array");
        for item in items {
            assert!(item.get("id").is_some(), "item must have id");
            assert!(item.get("title").is_some(), "item must have title");
            assert!(
                item.get("searchable_text").is_some(),
                "item must have searchable_text"
            );
        }
    }
}

#[tokio::test]
async fn sections_cached_across_calls() {
    let state = rich_state();
    let app = app(state.clone());

    let (_, json1) = get_json(&app, "/api/snapshot/sections").await;
    let (_, json2) = get_json(&app, "/api/snapshot/sections").await;

    assert_eq!(json1, json2, "cached sections must be identical");
    assert!(
        state.sections_cache.get().is_some(),
        "cache must be populated"
    );
}

#[tokio::test]
async fn sections_empty_snapshot_returns_empty_items() {
    // A bare snapshot with no sections set — all 9 sections should exist
    // but with empty items vecs.
    let snap = InspectionSnapshot::new();
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    });
    let app = app(state);
    let (status, json) = get_json(&app, "/api/snapshot/sections").await;
    assert_eq!(status, StatusCode::OK);

    let sections = json.as_array().unwrap();
    assert_eq!(sections.len(), 9);
    for section in sections {
        let items = section["items"].as_array().unwrap();
        assert!(
            items.is_empty(),
            "section '{}' should have no items on empty snapshot",
            section["id"]
        );
    }
}

#[tokio::test]
async fn viewed_roundtrip() {
    let state = rich_state();
    let app = app(state);

    // Initially no viewed items
    let (status, json) = get_json(&app, "/api/viewed").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["ids"].as_array().unwrap().len(), 0);

    // Mark an item as viewed
    let status = post_raw(
        &app,
        "/api/viewed",
        serde_json::json!({"id": "services:httpd.service"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Mark another item
    let status = post_raw(
        &app,
        "/api/viewed",
        serde_json::json!({"id": "storage:/boot"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify both are returned
    let (status, json) = get_json(&app, "/api/viewed").await;
    assert_eq!(status, StatusCode::OK);
    let ids = json["ids"].as_array().unwrap();
    assert_eq!(ids.len(), 2);
    let id_strs: Vec<&str> = ids.iter().filter_map(|v| v.as_str()).collect();
    assert!(id_strs.contains(&"services:httpd.service"));
    assert!(id_strs.contains(&"storage:/boot"));
}

#[tokio::test]
async fn viewed_idempotent() {
    let state = rich_state();
    let app = app(state);

    // Mark the same item twice
    post_raw(
        &app,
        "/api/viewed",
        serde_json::json!({"id": "services:httpd.service"}),
    )
    .await;
    post_raw(
        &app,
        "/api/viewed",
        serde_json::json!({"id": "services:httpd.service"}),
    )
    .await;

    let (_, json) = get_json(&app, "/api/viewed").await;
    let ids = json["ids"].as_array().unwrap();
    assert_eq!(ids.len(), 1, "duplicate viewed marks should be idempotent");
}

#[tokio::test]
async fn viewed_malformed_body_returns_400() {
    let app = app(rich_state());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/viewed")
                .header("content-type", "application/json")
                .body(Body::from("not json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn health_extended_fields() {
    let state = rich_state();
    let app = app(state);
    let (status, json) = get_json(&app, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["hostname"], "testhost.example.com");
    assert_eq!(
        json["os_release"],
        "Red Hat Enterprise Linux 9.4 (Plow)"
    );
    assert!(json.get("system_type").is_some());
    assert!(json.get("completeness").is_some());
}

#[tokio::test]
async fn health_minimal_snapshot() {
    // health endpoint should work even with empty meta and no os_release
    let app = app(test_state());
    let (status, json) = get_json(&app, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["hostname"], "");
    assert!(json["os_release"].is_null());
}

// --- normalize_for_context unit tests ---------------------------------------

#[test]
fn normalize_services_maps_state_changes_with_dropins() {
    let snap = rich_snapshot();
    let sections = normalize_for_context(&snap);
    let svc = sections.iter().find(|s| s.id == "services").unwrap();

    // httpd.service state_change (with folded drop-in detail)
    let httpd = svc.items.iter().find(|i| i.id == "httpd.service").unwrap();
    assert_eq!(httpd.title, "httpd.service");
    assert!(
        httpd.subtitle.as_ref().unwrap().contains("inactive"),
        "subtitle should contain current_state"
    );
    assert!(
        httpd.detail.is_some(),
        "httpd should have drop-in content folded in"
    );

    // standalone drop-in (no matching state_change)
    let standalone = svc
        .items
        .iter()
        .find(|i| i.id.contains("standalone.service"))
        .unwrap();
    assert_eq!(standalone.subtitle.as_deref(), Some("drop-in"));

    // enabled_units
    let sshd = svc.items.iter().find(|i| i.id == "sshd.service").unwrap();
    assert_eq!(sshd.subtitle.as_deref(), Some("enabled"));

    // disabled_units
    let cups = svc.items.iter().find(|i| i.id == "cups.service").unwrap();
    assert_eq!(cups.subtitle.as_deref(), Some("disabled"));
}

#[test]
fn normalize_containers_maps_all_types() {
    let snap = rich_snapshot();
    let sections = normalize_for_context(&snap);
    let ctr = sections.iter().find(|s| s.id == "containers").unwrap();

    assert_eq!(ctr.items.len(), 4, "quadlet + compose + running + flatpak");

    // QuadletUnit
    let quadlet = ctr.items.iter().find(|i| i.id == "myapp.container").unwrap();
    assert_eq!(quadlet.subtitle.as_deref(), Some("quay.io/myorg/myapp:latest"));

    // ComposeFile
    let compose = ctr.items.iter().find(|i| i.id == "/opt/compose/docker-compose.yml").unwrap();
    assert_eq!(compose.title, "docker-compose.yml");

    // RunningContainer
    let running = ctr.items.iter().find(|i| i.id == "abc123").unwrap();
    assert_eq!(running.title, "redis");
    assert!(running.subtitle.as_ref().unwrap().contains("redis:7"));

    // FlatpakApp
    let flatpak = ctr.items.iter().find(|i| i.id == "org.gimp.GIMP").unwrap();
    assert_eq!(flatpak.subtitle.as_deref(), Some("flathub/stable"));
}

#[test]
fn normalize_users_groups_extracts_json_fields() {
    let snap = rich_snapshot();
    let sections = normalize_for_context(&snap);
    let ug = sections.iter().find(|s| s.id == "users_groups").unwrap();

    assert_eq!(ug.items.len(), 2, "1 user + 1 group");

    let user = ug.items.iter().find(|i| i.id == "admin").unwrap();
    assert_eq!(user.subtitle.as_deref(), Some("uid:1000"));
    assert!(
        user.detail.as_ref().unwrap().contains("sudoers"),
        "admin user should have sudoers detail"
    );

    let group = ug.items.iter().find(|i| i.id == "wheel").unwrap();
    assert_eq!(group.subtitle.as_deref(), Some("gid:10"));
}

#[test]
fn normalize_network_maps_connections_and_firewall() {
    let snap = rich_snapshot();
    let sections = normalize_for_context(&snap);
    let net = sections.iter().find(|s| s.id == "network").unwrap();

    // NMConnection
    let eth0 = net.items.iter().find(|i| i.id == "eth0").unwrap();
    assert!(eth0.subtitle.as_ref().unwrap().contains("ethernet"));

    // FirewallZone
    let public = net.items.iter().find(|i| i.id == "public").unwrap();
    assert!(
        public.subtitle.as_ref().unwrap().contains("ssh"),
        "zone subtitle should summarize services"
    );

    // FirewallDirectRule
    let direct = net
        .items
        .iter()
        .find(|i| i.id == "ipv4:INPUT:0")
        .unwrap();
    assert_eq!(direct.title, "INPUT");

    // hosts_additions
    let hosts = net
        .items
        .iter()
        .find(|i| i.id == "192.168.1.100 myhost")
        .unwrap();
    assert_eq!(hosts.subtitle.as_deref(), Some("hosts"));

    // ProxyEntry
    let proxy = net
        .items
        .iter()
        .find(|i| i.id.contains("/etc/environment"))
        .unwrap();
    assert!(proxy.subtitle.as_ref().unwrap().contains("HTTP_PROXY"));
}

#[test]
fn normalize_storage_maps_fstab() {
    let snap = rich_snapshot();
    let sections = normalize_for_context(&snap);
    let stor = sections.iter().find(|s| s.id == "storage").unwrap();

    let boot = stor.items.iter().find(|i| i.id == "/boot").unwrap();
    assert!(boot.subtitle.as_ref().unwrap().contains("xfs"));
    assert_eq!(boot.detail.as_deref(), Some("defaults"));
}

#[test]
fn normalize_scheduled_tasks_maps_cron_and_timers() {
    let snap = rich_snapshot();
    let sections = normalize_for_context(&snap);
    let sched = sections.iter().find(|s| s.id == "scheduled_tasks").unwrap();

    assert_eq!(sched.items.len(), 2, "1 cron + 1 timer");

    let cron = sched.items.iter().find(|i| i.id == "/etc/cron.d/backup").unwrap();
    assert_eq!(cron.title, "backup");

    let timer = sched
        .items
        .iter()
        .find(|i| i.id == "logrotate.timer")
        .unwrap();
    assert_eq!(timer.subtitle.as_deref(), Some("daily"));
}

#[test]
fn normalize_selinux_includes_mode_and_modules() {
    let snap = rich_snapshot();
    let sections = normalize_for_context(&snap);
    let se = sections.iter().find(|s| s.id == "selinux").unwrap();

    // Mode item
    let mode = se.items.iter().find(|i| i.id == "selinux_mode").unwrap();
    assert_eq!(mode.title, "SELinux mode");
    assert_eq!(mode.subtitle.as_deref(), Some("enforcing"));

    // FIPS mode item
    let fips = se.items.iter().find(|i| i.id == "fips_mode").unwrap();
    assert_eq!(fips.subtitle.as_deref(), Some("enabled"));

    // custom_modules
    let module = se.items.iter().find(|i| i.id == "my_policy").unwrap();
    assert_eq!(module.subtitle.as_deref(), Some("custom module"));

    // fcontext_rules
    let fcontext = se.items.iter().find(|i| i.id.contains("/opt/app")).unwrap();
    assert_eq!(fcontext.subtitle.as_deref(), Some("fcontext"));
}

#[test]
fn normalize_kernel_boot_maps_cmdline_and_sysctl() {
    let snap = rich_snapshot();
    let sections = normalize_for_context(&snap);
    let kb = sections.iter().find(|s| s.id == "kernel_boot").unwrap();

    let cmdline = kb.items.iter().find(|i| i.id == "cmdline").unwrap();
    assert_eq!(cmdline.title, "Kernel cmdline");
    assert!(cmdline.detail.as_ref().unwrap().contains("quiet"));

    let sysctl = kb
        .items
        .iter()
        .find(|i| i.id == "kernel.sysrq")
        .unwrap();
    assert!(sysctl.subtitle.as_ref().unwrap().contains("16"));

    let modload = kb
        .items
        .iter()
        .find(|i| i.id.contains("custom.conf"))
        .unwrap();
    assert_eq!(modload.subtitle.as_deref(), Some("modules-load.d"));
}

#[test]
fn normalize_non_rpm_empty_section() {
    let snap = rich_snapshot();
    let sections = normalize_for_context(&snap);
    let nrpm = sections
        .iter()
        .find(|s| s.id == "non_rpm_software")
        .unwrap();
    // Rich snapshot has no non-rpm items — section should exist but be empty
    assert!(nrpm.items.is_empty());
}
