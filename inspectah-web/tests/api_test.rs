use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::containers::{
    ComposeFile, ComposeService, ContainerSection, FlatpakApp, QuadletUnit, RunningContainer,
};
use inspectah_core::types::kernelboot::{ConfigSnippet, KernelBootSection, SysctlOverride};
use inspectah_core::types::network::{
    FirewallDirectRule, FirewallZone, NMConnection, NetworkSection, ProxyEntry,
};
use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection, PipPackage};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::scheduled::{CronJob, ScheduledTaskSection, SystemdTimer};
use inspectah_core::types::selinux::SelinuxSection;
use inspectah_core::types::services::{
    PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState, SystemdDropIn,
};
use inspectah_core::types::storage::{FstabEntry, StorageSection};
use inspectah_core::types::users::UserGroupSection;
use inspectah_core::types::warnings::{Warning, WarningSeverity};
use inspectah_refine::session::RefineSession;
use inspectah_web::adapter::build_web_sections;
use inspectah_web::handlers::AppState;
use std::sync::{Arc, Mutex, OnceLock};
use tower::ServiceExt;

fn test_state() -> Arc<AppState> {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: true,
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true,
            locked: false,
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
            "op": "SetInclude",
            "target": {
                "item_id": {"kind": "Package", "key": {"name": "httpd", "arch": "x86_64"}},
                "include": true
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["generation"], 1);
    // Package stats are now in sections array
    let pkg_stats = json["stats"]["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["kind"] == "package")
        .expect("package section should exist");
    assert_eq!(pkg_stats["included"], 1);
}

#[tokio::test]
async fn apply_unknown_target_returns_422() {
    let app = app(test_state());
    let (status, json) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "op": "SetInclude",
            "target": {
                "item_id": {"kind": "Package", "key": {"name": "nonexistent", "arch": "x86_64"}},
                "include": false
            }
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
    let (status, json) =
        post_json(&app, "/api/tarball", serde_json::json!({"generation": 999})).await;
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
    assert!(
        json.get("error").is_some(),
        "error response must be JSON with 'error' field"
    );
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
        version_id: "9.4".into(),
        id: "rhel".into(),
        pretty_name: "Red Hat Enterprise Linux 9.4 (Plow)".into(),
        ..Default::default()
    });

    snap.services = Some(ServiceSection {
        state_changes: vec![ServiceStateChange {
            unit: "httpd.service".into(),
            current_state: inspectah_core::types::services::ServiceUnitState::Enabled,
            default_state: Some(inspectah_core::types::services::PresetDefault::Disable),
            include: false,
            locked: false,
            owning_package: None,
            fleet: None,
            attention_reason: None,
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
        preset_matched_units: Vec::new(),
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

    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![
            NonRpmItem {
                name: "myapp-venv".into(),
                path: "/opt/myapp/venv".into(),
                method: "pip".into(),
                confidence: "high".into(),
                lang: "python".into(),
                version: "3.11".into(),
                packages: vec![
                    PipPackage {
                        name: "requests".into(),
                        version: "2.31.0".into(),
                    },
                    PipPackage {
                        name: "flask".into(),
                        version: "3.0.0".into(),
                    },
                ],
                ..Default::default()
            },
            NonRpmItem {
                name: "node-app".into(),
                path: "/opt/node-app".into(),
                method: "binary".into(),
                confidence: "medium".into(),
                lang: "javascript".into(),
                ..Default::default()
            },
        ],
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
    // Users & groups moved to ViewResponse — 9 sections remain.
    assert_eq!(sections.len(), 9, "exactly 9 context sections");

    let ids: Vec<&str> = sections.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(ids.contains(&"services"));
    assert!(ids.contains(&"version_changes"));
    assert!(ids.contains(&"containers"));
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
    // Users & groups moved to ViewResponse — 9 sections remain.
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

    // Host object structure
    let host = json.get("host").expect("health must have 'host' object");
    assert_eq!(host["hostname"], "testhost.example.com");
    assert_eq!(
        host["os_name"], "Red Hat Enterprise Linux 9.4 (Plow)",
        "os_name uses pretty_name"
    );
    assert_eq!(host["os_version"], "9.4", "os_version uses version_id");
    assert_eq!(host["os_id"], "rhel");
    assert!(host.get("system_type").is_some());
    assert_eq!(
        host["schema_version"],
        inspectah_core::snapshot::SCHEMA_VERSION
    );
    assert!(json.get("completeness").is_some());
}

#[tokio::test]
async fn health_minimal_snapshot() {
    // health endpoint should work even with empty meta and no os_release
    let app = app(test_state());
    let (status, json) = get_json(&app, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "ok");

    let host = json.get("host").expect("health must have 'host' object");
    assert_eq!(host["hostname"], "");
    assert_eq!(host["os_name"], "");
    assert_eq!(host["os_version"], "");
    assert_eq!(host["os_id"], "");
    assert_eq!(
        host["schema_version"],
        inspectah_core::snapshot::SCHEMA_VERSION
    );
}

#[tokio::test]
async fn view_response_repo_groups_include_tier() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                locked: false,
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "epel-release".into(),
                arch: "noarch".into(),
                state: PackageState::Added,
                include: true,
                locked: false,
                source_repo: "epel".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    });
    let app = app(state);
    let (status, json) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);

    let groups = json["repo_groups"]
        .as_array()
        .expect("repo_groups must be array");
    let appstream = groups
        .iter()
        .find(|g| g["section_id"] == "appstream")
        .unwrap();
    assert_eq!(appstream["tier"], "distro");

    let epel = groups.iter().find(|g| g["section_id"] == "epel").unwrap();
    assert_eq!(epel["tier"], "third_party");
}

#[tokio::test]
async fn view_response_excludes_leaf_dep_tree() {
    let app = app(test_state());
    let (status, json) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        json.get("leaf_dep_tree").is_none(),
        "leaf_dep_tree should no longer appear in view response"
    );
}

// --- build_web_sections unit tests --------------------------------------------

#[test]
fn normalize_services_maps_state_changes_with_dropins() {
    let snap = rich_snapshot();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(session.reference());
    let svc = sections.iter().find(|s| s.id == "services").unwrap();

    // httpd.service state_change (with folded drop-in detail)
    let httpd = svc.items.iter().find(|i| i.id == "httpd.service").unwrap();
    assert_eq!(httpd.title, "httpd.service");
    assert!(
        httpd.subtitle.as_ref().unwrap().contains("enabled"),
        "subtitle should contain current_state"
    );
    assert!(
        httpd
            .subtitle
            .as_ref()
            .unwrap()
            .contains("diverges from preset"),
        "subtitle should indicate preset divergence"
    );
    assert!(
        httpd.detail.is_some(),
        "httpd should have drop-in content folded in"
    );

    // standalone drop-in (no matching state_change, not in enabled/disabled)
    let standalone = svc
        .items
        .iter()
        .find(|i| i.id.contains("standalone.service"))
        .unwrap();
    assert_eq!(standalone.subtitle.as_deref(), Some("drop-in override"));

    // enabled_units (legacy snapshot with empty preset_matched_units)
    let sshd = svc.items.iter().find(|i| i.id == "sshd.service").unwrap();
    assert_eq!(sshd.subtitle.as_deref(), Some("enabled (no preset rule)"));

    // disabled_units (legacy snapshot with empty preset_matched_units)
    let cups = svc.items.iter().find(|i| i.id == "cups.service").unwrap();
    assert_eq!(cups.subtitle.as_deref(), Some("disabled (no preset rule)"));
}

#[test]
fn normalize_containers_maps_all_types() {
    let snap = rich_snapshot();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(session.reference());
    let ctr = sections.iter().find(|s| s.id == "containers").unwrap();

    assert_eq!(ctr.items.len(), 4, "quadlet + compose + running + flatpak");

    // QuadletUnit
    let quadlet = ctr
        .items
        .iter()
        .find(|i| i.id == "myapp.container")
        .unwrap();
    assert_eq!(
        quadlet.subtitle.as_deref(),
        Some("quay.io/myorg/myapp:latest")
    );

    // ComposeFile
    let compose = ctr
        .items
        .iter()
        .find(|i| i.id == "/opt/compose/docker-compose.yml")
        .unwrap();
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
fn build_web_sections_excludes_users_groups() {
    // Users & groups data moved to ViewResponse.users_groups_decisions.
    let snap = rich_snapshot();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(session.reference());
    assert!(
        sections.iter().all(|s| s.id != "users_groups"),
        "users_groups section should no longer appear in build_web_sections"
    );
}

#[test]
fn normalize_network_maps_connections_and_firewall() {
    let snap = rich_snapshot();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(session.reference());
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
    let direct = net.items.iter().find(|i| i.id == "ipv4:INPUT:0").unwrap();
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
    let session = RefineSession::new(snap);
    let sections = build_web_sections(session.reference());
    let stor = sections.iter().find(|s| s.id == "storage").unwrap();

    let boot = stor.items.iter().find(|i| i.id == "/boot").unwrap();
    assert!(boot.subtitle.as_ref().unwrap().contains("xfs"));
    assert_eq!(boot.detail.as_deref(), Some("defaults"));
}

#[test]
fn normalize_scheduled_tasks_maps_cron_and_timers() {
    let snap = rich_snapshot();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(session.reference());
    let sched = sections.iter().find(|s| s.id == "scheduled_tasks").unwrap();

    assert_eq!(sched.items.len(), 2, "1 cron + 1 timer");

    let cron = sched
        .items
        .iter()
        .find(|i| i.id == "/etc/cron.d/backup")
        .unwrap();
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
    let session = RefineSession::new(snap);
    let sections = build_web_sections(session.reference());
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
    let session = RefineSession::new(snap);
    let sections = build_web_sections(session.reference());
    let kb = sections.iter().find(|s| s.id == "kernel_boot").unwrap();

    let cmdline = kb.items.iter().find(|i| i.id == "cmdline").unwrap();
    assert_eq!(cmdline.title, "Kernel cmdline");
    assert!(cmdline.detail.as_ref().unwrap().contains("quiet"));

    let sysctl = kb.items.iter().find(|i| i.id == "kernel.sysrq").unwrap();
    assert!(sysctl.subtitle.as_ref().unwrap().contains("16"));

    let modload = kb
        .items
        .iter()
        .find(|i| i.id.contains("custom.conf"))
        .unwrap();
    assert_eq!(modload.subtitle.as_deref(), Some("modules-load.d"));
}

#[test]
fn normalize_non_rpm_maps_packages_and_version() {
    let snap = rich_snapshot();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(session.reference());
    let nrpm = sections
        .iter()
        .find(|s| s.id == "non_rpm_software")
        .unwrap();

    assert_eq!(nrpm.items.len(), 2, "2 non-rpm items");

    // Item with pip packages — detail should list packages, not path
    let venv = nrpm.items.iter().find(|i| i.id == "myapp-venv").unwrap();
    let detail = venv.detail.as_ref().unwrap();
    assert!(
        detail.contains("requests==2.31.0"),
        "detail should include pip package name+version"
    );
    assert!(
        detail.contains("flask==3.0.0"),
        "detail should include all pip packages"
    );
    // searchable_text should include version
    assert!(
        venv.searchable_text.contains("3.11"),
        "searchable_text should include version when non-empty"
    );

    // Item without packages — detail should fall back to path
    let node = nrpm.items.iter().find(|i| i.id == "node-app").unwrap();
    assert_eq!(
        node.detail.as_deref(),
        Some("/opt/node-app"),
        "detail should fall back to path when no packages"
    );
    // Empty version should not add trailing space to searchable_text
    assert!(
        !node.searchable_text.ends_with(' '),
        "no trailing space in searchable_text when version is empty"
    );
}

#[test]
fn normalize_non_rpm_empty_section() {
    // Snapshot with no non-rpm data
    let snap = InspectionSnapshot::new();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(session.reference());
    let nrpm = sections
        .iter()
        .find(|s| s.id == "non_rpm_software")
        .unwrap();
    assert!(nrpm.items.is_empty());
}

#[test]
fn build_web_sections_section_count_and_ids() {
    let snap = rich_snapshot();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(session.reference());
    // Users & groups moved to ViewResponse — 9 sections remain.
    assert_eq!(sections.len(), 9, "exactly 9 context sections");

    let expected_ids = [
        "services",
        "version_changes",
        "containers",
        "network",
        "storage",
        "scheduled_tasks",
        "non_rpm_software",
        "kernel_boot",
        "selinux",
    ];
    for (i, expected) in expected_ids.iter().enumerate() {
        assert_eq!(
            sections[i].id, *expected,
            "section {} should be '{}'",
            i, expected
        );
    }
}

#[test]
fn build_web_sections_item_counts() {
    let snap = rich_snapshot();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(session.reference());

    // Verify non-zero item counts for sections with data
    let svc = sections.iter().find(|s| s.id == "services").unwrap();
    assert!(
        !svc.items.is_empty(),
        "services should have items from rich_snapshot"
    );

    let nrpm = sections
        .iter()
        .find(|s| s.id == "non_rpm_software")
        .unwrap();
    assert_eq!(nrpm.items.len(), 2, "non_rpm_software should have 2 items");
}

#[test]
fn sections_cache_returns_same_pointer() {
    use std::ptr;
    let state = rich_state();

    // First call populates cache
    let sections1 = state.sections_cache.get_or_init(|| {
        let session = state.session.lock().unwrap();
        build_web_sections(session.reference())
    });
    // Second call returns cached value
    let sections2 = state.sections_cache.get().unwrap();
    assert!(
        ptr::eq(sections1, sections2),
        "OnceLock must return the same allocation"
    );
}

#[tokio::test]
async fn health_pretty_name_fallback_to_name() {
    // When pretty_name is empty, os_name should fall back to name
    let mut snap = InspectionSnapshot::new();
    snap.os_release = Some(OsRelease {
        name: "Fedora Linux".into(),
        pretty_name: "".into(),
        version_id: "41".into(),
        id: "fedora".into(),
        ..Default::default()
    });
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    });
    let app = app(state);
    let (_, json) = get_json(&app, "/api/health").await;
    let host = json.get("host").unwrap();
    assert_eq!(host["os_name"], "Fedora Linux", "should fall back to name");
    assert_eq!(host["os_version"], "41");
    assert_eq!(host["os_id"], "fedora");
}

// --- Service subsection tests ------------------------------------------------

fn service_subsection_state() -> Arc<AppState> {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["firewalld".into()]),
        packages_added: vec![PackageEntry {
            name: "custom-app".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: false,
            locked: false,
            source_repo: "appstream".into(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![
            ServiceStateChange {
                unit: "custom-app.service".into(),
                current_state: ServiceUnitState::Enabled,
                default_state: Some(PresetDefault::Disable),
                include: true,
                locked: false,
                owning_package: Some("custom-app".into()),
                fleet: None,
                attention_reason: None,
            },
            ServiceStateChange {
                unit: "sssd-kcm.service".into(),
                current_state: ServiceUnitState::Disabled,
                default_state: Some(PresetDefault::Enable),
                include: true,
                locked: false,
                owning_package: Some("sssd".into()),
                fleet: None,
                attention_reason: None,
            },
        ],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });
    snap.warnings.push(Warning {
        inspector: "services".into(),
        message: "unit linked.service has state 'linked'".into(),
        severity: Some(WarningSeverity::Warning),
        extra: std::collections::HashMap::from([
            ("unit".into(), serde_json::json!("linked.service")),
            ("raw_state".into(), serde_json::json!("linked")),
        ]),
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    })
}

#[tokio::test]
async fn sections_include_service_subsections() {
    let app = app(service_subsection_state());
    let (status, json) = get_json(&app, "/api/snapshot/sections").await;
    assert_eq!(status, StatusCode::OK);

    let sections = json.as_array().unwrap();
    let svc = sections.iter().find(|s| s["id"] == "services").unwrap();

    let subsections = svc["subsections"]
        .as_array()
        .expect("subsections must be array");
    // At minimum, omissions and warnings should be present from the fixture.
    // Advisory subsection is validated in the handler unit test (before
    // RefineSession normalization can reclassify package include flags).
    assert!(
        subsections.iter().any(|s| s["id"] == "omitted_services"),
        "omitted_services subsection must be present"
    );
    assert!(
        subsections.iter().any(|s| s["id"] == "service_warnings"),
        "service_warnings subsection must be present"
    );

    // Verify subsection items have the right shape
    for sub in subsections {
        assert!(sub.get("id").is_some(), "subsection must have id");
        assert!(
            sub.get("display_name").is_some(),
            "subsection must have display_name"
        );
        let sub_items = sub["items"]
            .as_array()
            .expect("subsection items must be array");
        for item in sub_items {
            assert!(item.get("id").is_some(), "subsection item must have id");
            assert!(
                item.get("title").is_some(),
                "subsection item must have title"
            );
        }
    }
}

// --- Embedded asset resolution tests -----------------------------------------

#[test]
fn embedded_assets_include_prefixed_files() {
    use inspectah_web::assets::StaticAssets;

    let asset_files: Vec<String> = StaticAssets::iter()
        .filter(|path| path.starts_with("assets/"))
        .map(|path| path.to_string())
        .collect();

    assert!(
        !asset_files.is_empty(),
        "rust-embed must include files under the assets/ prefix"
    );

    // Every assets/* file must be resolvable via StaticAssets::get with the
    // full prefix — this is the invariant the fallback handler relies on.
    for file in &asset_files {
        assert!(
            StaticAssets::get(file).is_some(),
            "StaticAssets::get({:?}) must resolve",
            file
        );
    }
}

#[test]
fn embedded_assets_include_index_html() {
    use inspectah_web::assets::StaticAssets;

    assert!(
        StaticAssets::get("index.html").is_some(),
        "index.html must be embedded at the root of ui/dist/"
    );
}

#[tokio::test]
async fn fallback_serves_asset_files() {
    use inspectah_web::assets::StaticAssets;

    // Dynamically discover a JS asset — Vite hashes change every build.
    let js_asset = StaticAssets::iter()
        .find(|path| path.starts_with("assets/") && path.ends_with(".js"))
        .expect("at least one assets/*.js file must be embedded");

    let app = app(test_state());

    // Request the discovered JS asset via /assets/ path.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/{}", js_asset))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "/assets/<file> must resolve via fallback"
    );

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("javascript"),
        "JS asset must have a javascript content-type, got: {}",
        content_type
    );
}

#[tokio::test]
async fn fallback_serves_spa_for_unknown_paths() {
    let app = app(test_state());

    // A path that is neither an API route nor an embedded file should get
    // index.html (SPA client-side routing).
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/some/client/route")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "unknown paths should fall back to index.html"
    );

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/html"),
        "SPA fallback must serve text/html, got: {}",
        content_type
    );
}
