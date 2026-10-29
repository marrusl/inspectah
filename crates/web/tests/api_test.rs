use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
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
use inspectah_core::types::selinux::{
    FcontextFileType, FcontextRule, SelinuxContext, SelinuxSection,
};
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
            disposition: FindingKind::included(),
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            disposition: FindingKind::included(),
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
            "kind": "Op",
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

/// Path of the advisory config entry in [`advisory_state`].
const ADVISORY_CONFIG_PATH: &str = "/etc/init.d/legacy-app";

/// Session state carrying one modernization advisory config entry.
fn advisory_state() -> Arc<AppState> {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: ADVISORY_CONFIG_PATH.into(),
            kind: ConfigFileKind::Unowned,
            disposition: FindingKind::advisory(
                inspectah_core::types::AdvisoryType::Modernization,
                "sysvinit script — port to a systemd unit",
            ),
            ..Default::default()
        }],
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    })
}

/// A refused toggle is a client-side contract violation, not a server fault.
/// Both non-toggleable dispositions must say so with a status the frontend
/// can branch on — 500 reads as "inspectah broke" and hides the reason.
#[tokio::test]
async fn set_include_on_advisory_returns_422() {
    let app = app(advisory_state());
    let (status, json) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "kind": "Op",
            "op": "SetInclude",
            "target": {
                "item_id": {"kind": "Config", "key": {"path": ADVISORY_CONFIG_PATH}},
                "include": false
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let message = json["error"]
        .as_str()
        .expect("error body carries a message");
    assert!(
        message.contains(ADVISORY_CONFIG_PATH),
        "the refusal must name the item it refused: {message}"
    );
    assert!(
        message.contains("advisory"),
        "the refusal must say why the item is not toggleable: {message}"
    );
}

#[tokio::test]
async fn set_include_on_inventory_returns_422() {
    let app = app(test_state());
    let (status, json) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "kind": "Op",
            "op": "SetInclude",
            "target": {
                "item_id": {"kind": "NMConnection", "key": {"path": "/etc/NetworkManager/system-connections/eth0.nmconnection"}},
                "include": false
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let message = json["error"]
        .as_str()
        .expect("error body carries a message");
    assert!(
        message.contains("inventory"),
        "the refusal must say why the item is not toggleable: {message}"
    );
}

#[tokio::test]
async fn apply_unknown_target_returns_422() {
    let app = app(test_state());
    let (status, json) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "kind": "Op",
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
async fn api_op_accepts_ungroup_directive() {
    use inspectah_core::types::rpm::InstalledGroup;

    // Build a snapshot with an installed group so UngroupGroup has a valid target
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "podman".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            disposition: FindingKind::included(),
            locked: false,
            ..Default::default()
        }],
        installed_groups: Some(vec![InstalledGroup {
            name: "Container Management".into(),
            members: vec!["podman".into()],
            ..Default::default()
        }]),
        ..Default::default()
    });
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    });
    let app = app(state);

    let (status, json) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "kind": "View",
            "directive": "UngroupGroup",
            "group_name": "Container Management"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // The response should include a generation bump
    assert_eq!(json["generation"], 1);
}

/// § 8.12: `SetInclude(Group, false)` on a group whose sole effective member
/// is already excluded is refused with 422, not silently accepted with 200
/// and no recorded change (item 1.9).
#[tokio::test]
async fn set_include_group_already_excluded_returns_422_and_records_nothing() {
    use inspectah_core::types::rpm::InstalledGroup;

    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "podman".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            disposition: FindingKind::excluded(),
            locked: false,
            ..Default::default()
        }],
        installed_groups: Some(vec![InstalledGroup {
            name: "Container Management".into(),
            members: vec!["podman".into()],
            ..Default::default()
        }]),
        ..Default::default()
    });
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    });
    let app = app(state);

    let (_, ops_before) = get_json(&app, "/api/ops").await;

    let (status, json) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "kind": "Op",
            "op": "SetInclude",
            "target": {
                "item_id": {"kind": "Group", "key": {"name": "Container Management"}},
                "include": false
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        json["error"],
        "nothing to change: all 1 effective members of 'Container Management' are already excluded"
    );

    let (_, ops_after) = get_json(&app, "/api/ops").await;
    assert_eq!(
        ops_before, ops_after,
        "a refused op must not appear in the timeline"
    );
}

#[tokio::test]
async fn api_op_rejects_unknown_timeline_kind() {
    let app = app(test_state());
    let (status, json) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "kind": "Unknown",
            "foo": "bar"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
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
            disposition: FindingKind::excluded(),
            locked: false,
            owning_package: None,
            aggregate: None,
            attention_reason: None,
            shadow_type: None,
            shadow_rationale: None,
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
                        pinned: false,
                    },
                    PipPackage {
                        name: "flask".into(),
                        version: "3.0.0".into(),
                        pinned: false,
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
        fcontext_rules: vec![FcontextRule::Label {
            spec: "/opt/app(/.*)?".into(),
            file_type: FcontextFileType::All,
            context: SelinuxContext {
                user: "system_u".into(),
                role: "object_r".into(),
                type_: "httpd_sys_content_t".into(),
                level: Some("s0".into()),
            },
            disposition: FindingKind::included(),
            aggregate: None,
        }],
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
async fn sections_returns_eight_sections() {
    let app = app(rich_state());
    let (status, json) = get_json(&app, "/api/snapshot/sections").await;
    assert_eq!(status, StatusCode::OK);

    let sections = json.as_array().expect("sections is an array");
    // Users & groups moved to ViewResponse, version_changes retired — 8 sections remain.
    assert_eq!(sections.len(), 8, "exactly 8 context sections");

    let ids: Vec<&str> = sections.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(ids.contains(&"services"));
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
    // Users & groups moved to ViewResponse, version_changes retired — 8 sections remain.
    assert_eq!(sections.len(), 8);
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
    // version_id "9.4" already appears in pretty_name, so deduplicate_version
    // returns "" to avoid redundancy in the UI.
    assert_eq!(
        host["os_version"], "",
        "os_version is empty when version_id is already in os_name"
    );
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
                disposition: FindingKind::included(),
                locked: false,
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "epel-release".into(),
                arch: "noarch".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(),
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

// -- Task 7: the member-based repo no-op rule, wire-level (§ 3.2, § 8.8) -----

/// A `Verified` "vendor" repo with two actionable, unlocked members ("a",
/// "b"), included by default. A baseline (even empty) is required so both
/// normalize to Site -> leaf -> included (no `leaf_packages` means every
/// Site package defaults to leaf) rather than Investigate -> excluded with
/// no baseline at all.
fn vendor_repo_state() -> Arc<AppState> {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(inspectah_core::baseline::BaselineData {
        image_digest: "sha256:test".into(),
        packages: std::collections::HashMap::new(),
        extracted_at: "2026-01-01T00:00:00Z".into(),
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "a".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "vendor".into(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            },
            PackageEntry {
                name: "b".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "vendor".into(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            },
        ],
        repo_files: vec![inspectah_core::types::rpm::RepoFile {
            path: "/etc/yum.repos.d/vendor.repo".into(),
            content: "[vendor]\nname=Vendor\nbaseurl=http://example.invalid/vendor\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    })
}

async fn post_set_include_pkg(
    app: &axum::Router,
    name: &str,
    arch: &str,
    include: bool,
) -> (StatusCode, serde_json::Value) {
    post_json(
        app,
        "/api/op",
        serde_json::json!({
            "kind": "Op",
            "op": "SetInclude",
            "target": {
                "item_id": {"kind": "Package", "key": {"name": name, "arch": arch}},
                "include": include
            }
        }),
    )
    .await
}

async fn post_set_include_repo(
    app: &axum::Router,
    path: &str,
    include: bool,
) -> (StatusCode, serde_json::Value) {
    post_json(
        app,
        "/api/op",
        serde_json::json!({
            "kind": "Op",
            "op": "SetInclude",
            "target": {
                "item_id": {"kind": "Repo", "key": {"path": path}},
                "include": include
            }
        }),
    )
    .await
}

async fn ops_len(app: &axum::Router) -> usize {
    let (_, json) = get_json(app, "/api/ops").await;
    json.as_array().expect("/api/ops is an array").len()
}

/// § 8.8 state (a): enabled history (no repo op), both members individually
/// excluded. The counts derive `include: true` as the accepted direction;
/// `include: false`, the direction a history-reading switch would post
/// (the repo-exclusion history here is empty, i.e. "enabled"), is refused.
#[tokio::test]
async fn set_include_repo_state_a_accepted_and_refused() {
    let app = app(vendor_repo_state());
    post_set_include_pkg(&app, "a", "x86_64", false).await;
    post_set_include_pkg(&app, "b", "x86_64", false).await;
    let before = ops_len(&app).await;

    // Refused direction first: must not record.
    let (status, json) = post_set_include_repo(&app, "vendor", false).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        json["error"],
        "nothing to change: all 2 effective members of 'vendor' are already excluded"
    );
    assert_eq!(ops_len(&app).await, before, "a refused op must not record");

    // Accepted direction: one new /api/ops entry.
    let (status, _) = post_set_include_repo(&app, "vendor", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ops_len(&app).await, before + 1);
}

/// § 8.8 state (b): excluded history (a recorded repo exclude), both members
/// individually included. The counts derive `include: false` as the
/// accepted direction; `include: true`, the direction a history-reading
/// switch would post (the history here is "excluded"), is refused.
#[tokio::test]
async fn set_include_repo_state_b_accepted_and_refused() {
    let app = app(vendor_repo_state());
    post_set_include_repo(&app, "vendor", false).await;
    post_set_include_pkg(&app, "a", "x86_64", true).await;
    post_set_include_pkg(&app, "b", "x86_64", true).await;
    let before = ops_len(&app).await;

    // Refused direction first: must not record.
    let (status, json) = post_set_include_repo(&app, "vendor", true).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        json["error"],
        "nothing to change: all 2 effective members of 'vendor' are already included"
    );
    assert_eq!(ops_len(&app).await, before, "a refused op must not record");

    // Accepted direction: one new /api/ops entry.
    let (status, _) = post_set_include_repo(&app, "vendor", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ops_len(&app).await, before + 1);
}

/// § 8.8 + position 26: a repo the index knows and accepts, whose effective
/// set is empty (both declared members present and locked), is refused in
/// both directions with the "nothing to decide" reason -- never silently
/// accepted, and never recorded. The wire `error` is the error.rs prefix
/// ("nothing to change: ") plus the `NoopTarget` message in full: the brief
/// describes the client-visible text as starting "nothing to decide", but
/// `error.rs`'s existing, out-of-scope-for-this-task mapping always adds
/// that prefix first, so the full string is asserted here rather than a
/// `starts_with` check that the actual prefix would fail.
#[tokio::test]
async fn set_include_repo_with_no_effective_member_returns_422_in_both_directions() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "a".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "vendor".into(),
                disposition: FindingKind::included(),
                locked: true,
                ..Default::default()
            },
            PackageEntry {
                name: "b".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "vendor".into(),
                disposition: FindingKind::included(),
                locked: true,
                ..Default::default()
            },
        ],
        repo_files: vec![inspectah_core::types::rpm::RepoFile {
            path: "/etc/yum.repos.d/vendor.repo".into(),
            content: "[vendor]\nname=Vendor\nbaseurl=http://example.invalid/vendor\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    });
    let app = app(state);
    let before = ops_len(&app).await;

    for include in [true, false] {
        let (status, json) = post_set_include_repo(&app, "vendor", include).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            json["error"],
            "nothing to change: nothing to decide in 'vendor': all 2 members are locked"
        );
    }
    assert_eq!(ops_len(&app).await, before, "a refused op must not record");
}

/// § 8.12's second API line, owned by this task: `SetInclude(Repo)` on an
/// `Incomplete` repo with at least one effective member returns 200 --
/// Task 5 made `validate_target` accept `Incomplete`, and this is the
/// wire-level proof that the new provenance value is not refused.
#[tokio::test]
async fn set_include_repo_on_incomplete_provenance_with_effective_member_returns_200() {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(inspectah_core::baseline::BaselineData {
        image_digest: "sha256:test".into(),
        packages: std::collections::HashMap::new(),
        extracted_at: "2026-01-01T00:00:00Z".into(),
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "a".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "vendor".into(),
            disposition: FindingKind::included(),
            locked: false,
            ..Default::default()
        }],
        // No baseurl/metalink/mirrorlist -> Incomplete, not Verified.
        repo_files: vec![inspectah_core::types::rpm::RepoFile {
            path: "/etc/yum.repos.d/vendor.repo".into(),
            content: "[vendor]\nname=Vendor\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    });
    let app = app(state);

    let (status, _) = post_set_include_repo(&app, "vendor", false).await;
    assert_eq!(status, StatusCode::OK);
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

    // Top-level items should be empty (using subsections)
    assert!(net.items.is_empty());

    // Helper to find item across all subsections
    let find_item = |id: &str| {
        net.subsections
            .iter()
            .flat_map(|s| &s.items)
            .find(|i| i.id == id)
    };

    // NMConnection (in connections subsection)
    let eth0 = find_item("eth0").unwrap();
    assert!(eth0.subtitle.as_ref().unwrap().contains("ethernet"));

    // FirewallZone (in firewall subsection)
    let public = find_item("public").unwrap();
    assert!(
        public.subtitle.as_ref().unwrap().contains("ssh"),
        "zone subtitle should summarize services"
    );

    // FirewallDirectRule (in firewall subsection)
    let direct = find_item("ipv4:INPUT:0").unwrap();
    assert_eq!(direct.title, "INPUT");

    // hosts_additions (in dns_hosts subsection)
    let hosts = find_item("192.168.1.100 myhost").unwrap();
    assert_eq!(hosts.subtitle.as_deref(), Some("hosts"));

    // ProxyEntry (in proxy subsection)
    let proxy = net
        .subsections
        .iter()
        .flat_map(|s| &s.items)
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

    // Items are now in subsections
    let defaults = kb
        .subsections
        .iter()
        .find(|s| s.id == "defaults_context")
        .unwrap();
    let customizations = kb
        .subsections
        .iter()
        .find(|s| s.id == "customizations")
        .unwrap();

    let cmdline = defaults.items.iter().find(|i| i.id == "cmdline").unwrap();
    assert_eq!(cmdline.title, "Kernel cmdline");
    assert!(cmdline.detail.as_ref().unwrap().contains("quiet"));

    let sysctl = customizations
        .items
        .iter()
        .find(|i| i.id == "kernel.sysrq")
        .unwrap();
    assert!(sysctl.subtitle.as_ref().unwrap().contains("16"));

    let modload = customizations
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
    // Users & groups moved to ViewResponse, version_changes retired — 8 sections remain.
    assert_eq!(sections.len(), 8, "exactly 8 context sections");

    let expected_ids = [
        "services",
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
            disposition: FindingKind::excluded(),
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
                disposition: FindingKind::included(),
                locked: false,
                owning_package: Some("custom-app".into()),
                aggregate: None,
                attention_reason: None,
                shadow_type: None,
                shadow_rationale: None,
            },
            ServiceStateChange {
                unit: "sssd-kcm.service".into(),
                current_state: ServiceUnitState::Disabled,
                default_state: Some(PresetDefault::Enable),
                disposition: FindingKind::included(),
                locked: false,
                owning_package: Some("sssd".into()),
                aggregate: None,
                attention_reason: None,
                shadow_type: None,
                shadow_rationale: None,
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
                .uri(format!("/{}", js_asset))
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

// --- Locked-item API contract test -------------------------------------------

/// Build an AppState with an anaconda-sourced platform-plumbing package.
/// The package is NOT pre-locked — it enters as a normal PackageEntry and
/// must be locked by the anaconda classifier during RefineSession::new().
fn locked_package_state() -> Arc<AppState> {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "grub2-efi-aa64-cdboot".into(),
            arch: "aarch64".into(),
            state: PackageState::Added,
            source_repo: "anaconda".into(),
            disposition: FindingKind::included(),
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });
    snap.config = Some(ConfigSection { files: vec![] });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    })
}

#[tokio::test]
async fn locked_package_rejects_set_include_via_api() {
    let state = locked_package_state();
    let app = app(state.clone());

    // POST SetInclude(true) on a locked package — should succeed (200) but be a no-op
    let (status, _json) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "kind": "Op",
            "op": "SetInclude",
            "target": {
                "item_id": {"kind": "Package", "key": {"name": "grub2-efi-aa64-cdboot", "arch": "aarch64"}},
                "include": true
            }
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "SetInclude on locked item must return 200"
    );

    // GET /api/view and verify the package is still excluded
    let (view_status, view_json) = get_json(&app, "/api/view").await;
    assert_eq!(view_status, StatusCode::OK);

    // Generation must be 0 — the op was a no-op, so no mutation occurred
    assert_eq!(
        view_json["generation"], 0,
        "generation must stay 0 after no-op on locked item"
    );

    // Verify via the underlying session that the package is still excluded
    let session = state.session.lock().unwrap();
    let projected = session.snapshot_projected();
    let pkg = projected
        .rpm
        .as_ref()
        .unwrap()
        .packages_added
        .iter()
        .find(|p| p.name == "grub2-efi-aa64-cdboot")
        .unwrap();
    assert!(
        !pkg.disposition.is_included(),
        "API: locked package must stay excluded after SetInclude(true)"
    );
    assert!(pkg.locked, "API: locked flag must be preserved");
}

// --- Package group and provenance API tests ---------------------------------

#[tokio::test]
async fn api_view_includes_package_groups() {
    use inspectah_core::types::rpm::InstalledGroup;

    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "gcc".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            },
            PackageEntry {
                name: "make".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(),
                locked: true,
                ..Default::default()
            },
        ],
        installed_groups: Some(vec![InstalledGroup {
            name: "Development Tools".into(),
            members: vec!["gcc".into(), "make".into()],
            optional_installed: vec!["valgrind".into()],
        }]),
        ..Default::default()
    });
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    });
    let app = app(state);

    let (status, json) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);

    let groups = json["package_groups"]
        .as_array()
        .expect("package_groups must be array");
    assert_eq!(groups.len(), 1, "one installed group");
    assert_eq!(groups[0]["name"], "Development Tools");
    assert_eq!(groups[0]["member_count"], 2);
    assert_eq!(groups[0]["optional_spillover_count"], 1);

    // Render state reflects the session's classification of members.
    let state = groups[0]["render_state"].as_str().unwrap();
    assert!(
        ["renderable", "excluded", "ungrouped", "degraded"].contains(&state),
        "render_state must be a known state, got: {state}"
    );

    let members = groups[0]["members"].as_array().expect("members array");
    assert_eq!(members.len(), 2);
    // Each member has the expected shape
    for m in members {
        assert!(m.get("name").is_some());
        assert!(m.get("locked").is_some());
        assert!(m["overlap_groups"].is_array());
    }
}

#[tokio::test]
async fn api_view_populates_optional_spillover_provenance() {
    use inspectah_core::types::rpm::InstalledGroup;

    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "gcc".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            },
            PackageEntry {
                name: "valgrind".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            },
        ],
        installed_groups: Some(vec![InstalledGroup {
            name: "Development Tools".into(),
            members: vec!["gcc".into()],
            optional_installed: vec!["valgrind".into()],
        }]),
        ..Default::default()
    });
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    });
    let app = app(state);

    let (status, json) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);

    let provenances = json
        .get("package_provenances")
        .expect("package_provenances field");
    let valgrind_prov = provenances
        .get("valgrind.x86_64")
        .expect("valgrind should have provenance");
    assert_eq!(valgrind_prov["kind"], "optional_spillover");
    assert_eq!(valgrind_prov["group_name"], "Development Tools");
}

#[tokio::test]
async fn api_view_populates_ungrouped_member_provenance() {
    use inspectah_core::types::rpm::InstalledGroup;

    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "podman".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            disposition: FindingKind::included(),
            locked: false,
            ..Default::default()
        }],
        installed_groups: Some(vec![InstalledGroup {
            name: "Container Management".into(),
            members: vec!["podman".into()],
            ..Default::default()
        }]),
        ..Default::default()
    });
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    });
    let app = app(state);

    // Apply UngroupGroup directive
    let (status, _) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "kind": "View",
            "directive": "UngroupGroup",
            "group_name": "Container Management"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Now GET /api/view — podman should have ungrouped_member provenance
    let (status, json) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);

    let provenances = json
        .get("package_provenances")
        .expect("package_provenances field");
    let podman_prov = provenances
        .get("podman.x86_64")
        .expect("podman should have provenance after ungroup");
    assert_eq!(podman_prov["kind"], "ungrouped_member");
    assert_eq!(podman_prov["group_name"], "Container Management");

    // Verify group render_state is ungrouped
    let groups = json["package_groups"].as_array().unwrap();
    let cm = groups
        .iter()
        .find(|g| g["name"] == "Container Management")
        .unwrap();
    assert_eq!(cm["render_state"], "ungrouped");
}

#[tokio::test]
async fn api_ops_returns_timeline_entries_with_active_flag() {
    use inspectah_core::types::rpm::InstalledGroup;

    // Build a snapshot with a package and a group
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "podman".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            disposition: FindingKind::included(),
            locked: false,
            ..Default::default()
        }],
        installed_groups: Some(vec![InstalledGroup {
            name: "Container Management".into(),
            members: vec!["podman".into()],
            ..Default::default()
        }]),
        ..Default::default()
    });
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    });
    let app = app(state);

    // Apply an op
    let (status, _) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "kind": "Op",
            "op": "SetInclude",
            "target": {
                "item_id": {
                    "kind": "Package",
                    "key": {"name": "podman", "arch": "x86_64"}
                },
                "include": false
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Apply a view directive
    let (status, _) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "kind": "View",
            "directive": "UngroupGroup",
            "group_name": "Container Management"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // GET /api/ops — should return timeline entries with active: true
    let (status, json) = get_json(&app, "/api/ops").await;
    assert_eq!(status, StatusCode::OK);

    let entries = json.as_array().expect("ops should be an array");
    assert!(
        !entries.is_empty(),
        "timeline should have at least one entry"
    );

    // Verify the View directive entry has correct structure
    let view_entry = entries.iter().find(|e| e["kind"] == "View");
    assert!(view_entry.is_some(), "timeline should contain a View entry");
    let view_entry = view_entry.unwrap();
    assert_eq!(view_entry["directive"], "UngroupGroup");
    assert_eq!(view_entry["group_name"], "Container Management");
    assert_eq!(view_entry["active"], true);

    // Undo the directive
    let (status, _) = post_json(&app, "/api/undo", serde_json::json!({})).await;
    assert_eq!(status, StatusCode::OK);

    // GET /api/ops again — the entry should now have active: false
    let (status, json) = get_json(&app, "/api/ops").await;
    assert_eq!(status, StatusCode::OK);

    let entries = json.as_array().expect("ops should be an array");
    let view_entry = entries.iter().find(|e| e["kind"] == "View").unwrap();

    // Entry now inactive (in redo tail)
    assert_eq!(view_entry["active"], false);
}

// -- /api/upload-rpm -------------------------------------------------------

const UPLOAD_BOUNDARY: &str = "inspectah-test-boundary";

/// The upload directory's name under the test's temporary directory. It is
/// a subdirectory so a test can prove nothing escaped into the parent.
const UPLOAD_SUBDIR: &str = "snap-uploads";

/// One repo-less `epel-release` at 9-11.el9 and one repo-less
/// `custom-tool`, staging into a directory that does not exist yet.
fn upload_state() -> (Arc<AppState>, tempfile::TempDir) {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "epel-release".into(),
                version: "9".into(),
                release: "11.el9".into(),
                arch: "noarch".into(),
                disposition: FindingKind::included(),
                repoless_annotation: "No repo source".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "custom-tool".into(),
                version: "1.0".into(),
                release: "1.el9".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                repoless_annotation: "No repo source".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    let tmp = tempfile::tempdir().unwrap();
    let mut session = RefineSession::new(snap);
    session.set_upload_dir(tmp.path().join(UPLOAD_SUBDIR));
    (
        Arc::new(AppState {
            session: Arc::new(Mutex::new(session)),
            sections_cache: OnceLock::new(),
        }),
        tmp,
    )
}

/// One multipart part: the `Content-Disposition` value verbatim, and the
/// body bytes.
type MultipartPart = (Vec<u8>, Vec<u8>);

fn part(disposition: &str, content: &[u8]) -> MultipartPart {
    (disposition.as_bytes().to_vec(), content.to_vec())
}

/// A `file` part whose filename is `name`, quoted as a browser sends it.
fn file_part(name: &str, content: &[u8]) -> MultipartPart {
    part(
        &format!("form-data; name=\"file\"; filename=\"{name}\""),
        content,
    )
}

fn multipart_body(parts: &[MultipartPart]) -> Vec<u8> {
    let mut body = Vec::new();
    for (disposition, content) in parts {
        body.extend_from_slice(format!("--{UPLOAD_BOUNDARY}\r\nContent-Disposition: ").as_bytes());
        body.extend_from_slice(disposition);
        body.extend_from_slice(b"\r\nContent-Type: application/x-rpm\r\n\r\n");
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{UPLOAD_BOUNDARY}--\r\n").as_bytes());
    body
}

fn upload_request(body: Body) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/upload-rpm")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={UPLOAD_BOUNDARY}"),
        )
        .body(body)
        .unwrap()
}

async fn post_upload_body(app: &axum::Router, body: Body) -> (StatusCode, serde_json::Value) {
    let response = app.clone().oneshot(upload_request(body)).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

async fn post_upload(
    app: &axum::Router,
    parts: &[MultipartPart],
) -> (StatusCode, serde_json::Value) {
    post_upload_body(app, Body::from(multipart_body(parts))).await
}

/// Every entry in `dir`, sorted. Empty when the directory does not exist.
fn dir_entries(dir: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn part_files(dir: &std::path::Path) -> Vec<String> {
    dir_entries(dir)
        .into_iter()
        .filter(|n| n.starts_with(".upload-") && n.ends_with(".part"))
        .collect()
}

async fn view(app: &axum::Router) -> serde_json::Value {
    let (status, body) = get_json(app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);
    body
}

/// The `PackageEntry` the view carries for a package name.
fn view_package(view: &serde_json::Value, name: &str) -> serde_json::Value {
    view["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["entry"]["name"] == name)
        .unwrap_or_else(|| panic!("no package {name} in the view"))["entry"]
        .clone()
}

#[tokio::test]
async fn upload_filename_rule_at_the_handler() {
    // § 8.2: each name is refused with 400 naming the rule, with nothing
    // in the upload directory and the view generation unchanged.
    let over_long = format!("{}.rpm", "a".repeat(197)); // 201 bytes
    let rejected = [
        "epel-release-9$(id)-1.noarch.rpm",
        "epel-release 9-1.noarch.rpm",
        "epel-release-9-1.noarch (1).rpm",
        "../x.rpm",
        "..\\x.rpm",
        "/etc/x.rpm",
        ".epel-release-9-1.noarch.rpm",
        over_long.as_str(),
    ];
    for name in rejected {
        let (state, tmp) = upload_state();
        let dir = tmp.path().join(UPLOAD_SUBDIR);
        let app = app(state);
        let generation_before = view(&app).await["generation"].clone();

        let (status, body) = post_upload(&app, &[file_part(name, b"an rpm")]).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}: {body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .starts_with("filename rejected: staged filename "),
            "{name}: {body}"
        );
        assert_eq!(dir_entries(&dir), Vec::<String>::new(), "{name}");
        assert_eq!(view(&app).await["generation"], generation_before, "{name}");
    }

    // The header forms a browser can send that carry no usable filename,
    // and the field-count and field-name rules. Each is 400 with nothing
    // staged; the reason differs, so only the status and the filesystem
    // are asserted here.
    let mut invalid_utf8 = b"form-data; name=\"file\"; filename=\"a".to_vec();
    invalid_utf8.push(0xFF);
    invalid_utf8.extend_from_slice(b"-1-1.noarch.rpm\"");
    let malformed: Vec<(&str, Vec<MultipartPart>)> = vec![
        (
            "a percent-encoded newline",
            vec![part(
                "form-data; name=\"file\"; filename*=UTF-8''epel-release-9-1%0ARUN%20id.noarch.rpm",
                b"an rpm",
            )],
        ),
        (
            "a percent-encoded carriage return",
            vec![part(
                "form-data; name=\"file\"; filename*=UTF-8''epel-release-9-1%0D.noarch.rpm",
                b"an rpm",
            )],
        ),
        (
            "an invalid UTF-8 filename",
            vec![(invalid_utf8, b"an rpm".to_vec())],
        ),
        (
            "two fields",
            vec![
                file_part("epel-release-9-12.el9.noarch.rpm", b"first"),
                file_part("epel-release-latest-9.noarch.rpm", b"second"),
            ],
        ),
        (
            "a field not named file",
            vec![part(
                "form-data; name=\"rpm\"; filename=\"epel-release-9-12.el9.noarch.rpm\"",
                b"an rpm",
            )],
        ),
        (
            "a file field with no filename",
            vec![part("form-data; name=\"file\"", b"an rpm")],
        ),
        ("no field at all", vec![]),
    ];
    for (label, parts) in malformed {
        let (state, tmp) = upload_state();
        let dir = tmp.path().join(UPLOAD_SUBDIR);
        let app = app(state);
        let generation_before = view(&app).await["generation"].clone();

        let (status, body) = post_upload(&app, &parts).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{label}: {body}");
        assert_eq!(dir_entries(&dir), Vec::<String>::new(), "{label}");
        assert_eq!(view(&app).await["generation"], generation_before, "{label}");
    }

    // Accepted (200): the seven names of § 8.2, each staged under the name
    // it was sent with.
    let accepted = [
        "epel-release-9-12.el9.noarch.rpm",
        "epel-release-latest-9.noarch.rpm",
        "google-chrome-stable_current_x86_64.rpm",
        "libstdc++-devel-11-1.el9.x86_64.rpm",
        "foo-1.0~rc1-1.fc40.x86_64.rpm",
        "foo-1.2^20240101git1-1.fc40.x86_64.rpm",
        "X-1-1.noarch.RPM",
    ];
    for name in accepted {
        let (state, tmp) = upload_state();
        let dir = tmp.path().join(UPLOAD_SUBDIR);
        let app = app(state);

        let (status, body) = post_upload(&app, &[file_part(name, b"an rpm")]).await;

        assert_eq!(status, StatusCode::OK, "{name}: {body}");
        assert_eq!(body["staged_as"], name, "{name}");
        assert_eq!(dir_entries(&dir), vec![name.to_string()], "{name}");
    }
}

#[tokio::test]
async fn rejected_name_reaches_neither_the_filesystem_nor_the_containerfile() {
    // The assertion at the staging boundary: a name the allowlist refuses
    // must not reach disk by any route, and must leave the Containerfile
    // naming nothing. `commit` takes a `StagedName`, so the rename cannot
    // be reached with an unvalidated name at all; this proves it through
    // the one route a real rejected name arrives by.
    for name in ["epel-release-9$(id)-1.noarch.rpm", "../x.rpm"] {
        let (state, tmp) = upload_state();
        let dir = tmp.path().join(UPLOAD_SUBDIR);
        let app = app(state);
        let before = view(&app).await;

        let (status, _) = post_upload(&app, &[file_part(name, b"an rpm")]).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}");

        // Nothing staged, no part file, and nothing written beside the
        // directory either: `../x.rpm` is rejected, never stripped to
        // `x.rpm` and never placed one level up.
        assert_eq!(dir_entries(&dir), Vec::<String>::new(), "{name}");
        assert_eq!(
            dir_entries(tmp.path()),
            vec![UPLOAD_SUBDIR.to_string()],
            "{name}"
        );

        // Nothing recorded on the package, so nothing can render.
        let after = view(&app).await;
        assert_eq!(after["generation"], before["generation"], "{name}");
        let pkg = view_package(&after, "epel-release");
        assert_eq!(pkg["uploaded"], serde_json::Value::Null, "{name}");
        assert_eq!(pkg["cache_path"], serde_json::Value::Null, "{name}");

        let containerfile = after["containerfile_preview"].as_str().unwrap();
        assert!(
            !containerfile.contains("COPY repoless-packages/"),
            "{name}: {containerfile}"
        );
        assert!(!containerfile.contains("$("), "{name}");
        assert!(!containerfile.contains("x.rpm"), "{name}");
        assert!(
            containerfile.contains("# MANUAL: epel-release"),
            "{name}: the package is still waiting for an RPM"
        );
    }

    // The control: the same assertions against an accepted name, so the
    // Containerfile half of this test can fail. The `COPY` names the
    // staged filename, which is what makes the half above mean anything:
    // before the export verified staged names, no staged name reached a
    // `COPY` at all and "a rejected one does not" was true for free.
    let (state, tmp) = upload_state();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    let app = app(state);
    let (status, _) = post_upload(
        &app,
        &[file_part("epel-release-9-12.el9.noarch.rpm", b"an rpm")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        dir_entries(&dir),
        vec!["epel-release-9-12.el9.noarch.rpm".to_string()]
    );
    include_package(&app, "epel-release", "noarch").await;
    let after = view(&app).await;
    let pkg = view_package(&after, "epel-release");
    assert_eq!(pkg["uploaded"], true);
    assert_eq!(
        pkg["cache_path"],
        dir.join("epel-release-9-12.el9.noarch.rpm")
            .to_string_lossy()
            .as_ref()
    );
    // Full-line equality, not `contains`: the pre-excluded form of this
    // same package renders `# COPY repoless-packages/... /tmp/`, which
    // contains the substring, so a substring search cannot tell a build
    // step from advice about one.
    let preview = after["containerfile_preview"].as_str().unwrap();
    assert!(
        preview
            .lines()
            .any(|l| l == "COPY repoless-packages/epel-release-9-12.el9.noarch.rpm /tmp/"),
        "{preview}"
    );
}

/// Include a repo-less package, which refine pre-excludes.
async fn include_package(app: &axum::Router, name: &str, arch: &str) {
    let (status, _) = post_json(
        app,
        "/api/op",
        serde_json::json!({
            "kind": "Op",
            "op": "SetInclude",
            "target": {
                "item_id": {"kind": "Package", "key": {"name": name, "arch": arch}},
                "include": true
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn export_copies_a_staged_upload_from_the_canonical_directory() {
    // The one production reader of `upload_dir_canon()` is the export at
    // `handlers.rs`, and until this test nothing in the suite exercised
    // it with a staged upload -- the confinement it passes had no reader
    // outside an assertion. This drives the real route: upload, include,
    // export, and read the member out of the tarball the response body
    // carries.
    let (state, tmp) = upload_state();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    let app = app(state);

    let name = "epel-release-9-12.el9.noarch.rpm";
    let (status, _) = post_upload(&app, &[file_part(name, b"staged rpm bytes")]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(dir_entries(&dir), vec![name.to_string()]);
    include_package(&app, "epel-release", "noarch").await;

    let (status, headers, body) = post_tarball(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get("x-export-warnings")
            .and_then(|v| v.to_str().ok()),
        Some("[]")
    );

    let member = tarball_member(&body, &format!("repoless-packages/{name}"))
        .expect("the staged upload must be in the export");
    assert_eq!(member, b"staged rpm bytes".to_vec());

    // And the Containerfile in the same tarball names it as a build
    // step, by full line: the pre-excluded form of this package renders
    // the same text behind a `#`.
    let containerfile = String::from_utf8(tarball_member(&body, "Containerfile").unwrap()).unwrap();
    assert!(
        containerfile
            .lines()
            .any(|l| l == format!("COPY repoless-packages/{name} /tmp/")),
        "{containerfile}"
    );
}

#[tokio::test]
async fn export_warns_when_a_staged_upload_cannot_be_verified() {
    // The same route, with the staged file removed behind the session's
    // back: the export demotes rather than failing, and says so on the
    // response. Without this the warnings header is a surface nobody can
    // tell is working.
    let (state, tmp) = upload_state();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    let app = app(state);

    let name = "epel-release-9-12.el9.noarch.rpm";
    let (status, _) = post_upload(&app, &[file_part(name, b"staged rpm bytes")]).await;
    assert_eq!(status, StatusCode::OK);
    include_package(&app, "epel-release", "noarch").await;
    std::fs::remove_file(dir.join(name)).unwrap();

    let (status, headers, body) = post_tarball(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get("x-export-warnings")
            .and_then(|v| v.to_str().ok()),
        Some(r#"["epel-release.noarch: staged RPM not verified, not exported"]"#)
    );
    assert!(tarball_member(&body, &format!("repoless-packages/{name}")).is_none());

    let containerfile = String::from_utf8(tarball_member(&body, "Containerfile").unwrap()).unwrap();
    assert!(
        containerfile.lines().any(|l| l
            == "# FIXME: upload the RPM for epel-release.noarch again in refine; \
                the staged file could not be verified for export"),
        "{containerfile}"
    );
}

/// `POST /api/tarball` at the current generation: status, headers, body.
async fn post_tarball(app: &axum::Router) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let generation = view(app).await["generation"].as_u64().unwrap();
    let request = Request::builder()
        .method("POST")
        .uri("/api/tarball")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({ "generation": generation }).to_string(),
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, body.to_vec())
}

/// One member's bytes from a gzipped export tarball, by its path below
/// the top-level directory.
fn tarball_member(tarball: &[u8], rel: &str) -> Option<Vec<u8>> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tarball));
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let full = entry.path().unwrap().to_string_lossy().into_owned();
        if full.split_once('/').map(|(_, rest)| rest) == Some(rel) {
            let mut bytes = Vec::new();
            std::io::copy(&mut entry, &mut bytes).unwrap();
            return Some(bytes);
        }
    }
    None
}

#[tokio::test]
async fn upload_response_carries_the_server_verdict() {
    // § 8.6 + § 8.13.
    let (state, tmp) = upload_state();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    let app = app(state);

    let (status, body) = post_upload(
        &app,
        &[file_part("epel-release-latest-9.noarch.RPM", b"an rpm")],
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "matched");
    assert_eq!(body["matched"], "epel-release.noarch");
    assert_eq!(body["match_kind"], "version_differs");
    assert_eq!(body["host_version"], "9-11.el9");
    assert_eq!(body["uploaded_version"], "9");
    assert_eq!(body["parsed"]["name"], "epel-release-latest");
    assert_eq!(body["parsed"]["arch"], "noarch");
    assert_eq!(body["parsed"]["version_release"], "9");
    assert_eq!(body["staged_as"], "epel-release-latest-9.noarch.RPM");
    assert_eq!(body["uploaded"], 1);
    assert_eq!(
        dir_entries(&dir),
        vec!["epel-release-latest-9.noarch.RPM".to_string()]
    );
    assert_eq!(
        std::fs::read(dir.join("epel-release-latest-9.noarch.RPM")).unwrap(),
        b"an rpm".as_slice()
    );
}

#[tokio::test]
async fn unmatched_upload_is_staged_and_changes_no_package() {
    let (state, tmp) = upload_state();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    let app = app(state);
    let before = view(&app).await;

    let (status, body) =
        post_upload(&app, &[file_part("nginx-1.0-1.el9.x86_64.rpm", b"an rpm")]).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "unmatched");
    assert_eq!(body["matched"], serde_json::Value::Null);
    assert_eq!(body["match_kind"], serde_json::Value::Null);
    assert_eq!(body["staged_as"], "nginx-1.0-1.el9.x86_64.rpm");
    assert_eq!(
        dir_entries(&dir),
        vec!["nginx-1.0-1.el9.x86_64.rpm".to_string()],
        "an unmatched file is still staged for the modal's remove-or-retry"
    );
    let after = view(&app).await;
    assert_eq!(after["generation"], before["generation"]);
    assert_eq!(after["packages"], before["packages"]);
}

#[tokio::test]
async fn upload_refuses_a_filename_another_package_has_staged() {
    // `cache_path` deserializes from the snapshot, so a resumed session
    // can hold a staged name for a package the uploaded file does not
    // match. No rename ever replaces another package's staged file.
    let (state, tmp) = upload_state();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    std::fs::create_dir(&dir).unwrap();
    let contested = dir.join("epel-release-9-12.el9.noarch.rpm");
    std::fs::write(&contested, b"custom-tool's file").unwrap();
    {
        let mut session = state.session.lock().unwrap();
        session.set_uploaded(
            "custom-tool.x86_64",
            contested.clone(),
            Some("9-12.el9".into()),
        );
    }
    let app = app(state);
    let before = view(&app).await;

    let (status, body) = post_upload(
        &app,
        &[file_part(
            "epel-release-9-12.el9.noarch.rpm",
            b"epel's file",
        )],
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["error"],
        "epel-release-9-12.el9.noarch.rpm is already staged for custom-tool.x86_64"
    );
    assert_eq!(
        std::fs::read(&contested).unwrap(),
        b"custom-tool's file".as_slice()
    );
    assert_eq!(
        dir_entries(&dir),
        vec!["epel-release-9-12.el9.noarch.rpm".to_string()],
        "the refused upload's part file is dropped"
    );
    let after = view(&app).await;
    assert_eq!(after["generation"], before["generation"]);
    assert_eq!(
        view_package(&after, "epel-release")["uploaded"],
        serde_json::Value::Null
    );
}

#[tokio::test]
async fn upload_replacing_an_upload_removes_the_superseded_file() {
    let (state, tmp) = upload_state();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    let app = app(state);

    for name in [
        "epel-release-9-12.el9.noarch.rpm",
        "epel-release-9-13.el9.noarch.rpm",
    ] {
        let (status, body) = post_upload(&app, &[file_part(name, name.as_bytes())]).await;
        assert_eq!(status, StatusCode::OK, "{name}: {body}");
    }

    assert_eq!(
        dir_entries(&dir),
        vec!["epel-release-9-13.el9.noarch.rpm".to_string()],
        "the superseded file is deleted after the record step"
    );
    let pkg = view_package(&view(&app).await, "epel-release");
    assert_eq!(
        pkg["cache_path"],
        dir.join("epel-release-9-13.el9.noarch.rpm")
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(pkg["uploaded_version"], "9-13.el9");
}

/// A session whose package state was **deserialized**, the way refine
/// loads a scan. `uploaded` and `cache_path` are `#[serde(default)]`
/// fields that arrive from the scanned host, so "uploaded implies
/// `set_uploaded` wrote this path" is not something the collector or the
/// type system enforces.
fn upload_state_from_scan(tmp: &tempfile::TempDir, package: serde_json::Value) -> Arc<AppState> {
    let snap: InspectionSnapshot = serde_json::from_value(serde_json::json!({
        "schema_version": inspectah_core::snapshot::SCHEMA_VERSION,
        "rpm": { "packages_added": [package] },
    }))
    .expect("the scan snapshot deserializes");
    let mut session = RefineSession::new(snap);
    session.set_upload_dir(tmp.path().join(UPLOAD_SUBDIR));
    Arc::new(AppState {
        session: Arc::new(Mutex::new(session)),
        sections_cache: OnceLock::new(),
    })
}

const SNAPSHOT_SUPPLIED_SECRET: &[u8] = b"a private key the operator can unlink";

#[tokio::test]
async fn upload_does_not_unlink_a_snapshot_supplied_path_outside_the_upload_directory() {
    // A scanned host sets `uploaded: true` with a `cache_path` pointing
    // anywhere the operator can write. One ordinary upload for that
    // package used to make the superseded-file deletion unlink it.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    let outside = tmp.path().join("id_ed25519");
    std::fs::write(&outside, SNAPSHOT_SUPPLIED_SECRET).unwrap();
    let app = app(upload_state_from_scan(
        &tmp,
        serde_json::json!({
            "name": "foo",
            "version": "1",
            "release": "1",
            "arch": "noarch",
            "repoless_annotation": "No repo source",
            "repoless_cached": true,
            "uploaded": true,
            "cache_path": outside.to_string_lossy(),
        }),
    ));

    let (status, body) = post_upload(&app, &[file_part("foo-1-1.noarch.rpm", b"an rpm")]).await;

    assert!(
        outside.exists(),
        "an upload must not unlink a file outside the upload directory"
    );
    assert_eq!(
        std::fs::read(&outside).unwrap(),
        SNAPSHOT_SUPPLIED_SECRET,
        "the file outside the upload directory is untouched"
    );
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["error"],
        "foo.noarch records a staged file outside the upload directory; nothing was staged"
    );
    assert_eq!(
        dir_entries(&dir),
        Vec::<String>::new(),
        "a refused upload places nothing"
    );
}

#[tokio::test]
async fn first_upload_for_a_dnf_cached_package_keeps_the_host_rpm() {
    // `superseded_upload` filters on `uploaded`, so the first upload for
    // a package the collector found in the host's dnf cache supersedes
    // nothing: that file is the one `prior_cache` records for removal to
    // restore, and deleting it would make the restore name a file that
    // is gone. epel-release arrives this way on the basis scan.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    let host_cached = tmp.path().join("epel-release-9-11.el9.noarch.rpm");
    std::fs::write(&host_cached, b"the host's cached rpm").unwrap();
    let app = app(upload_state_from_scan(
        &tmp,
        serde_json::json!({
            "name": "epel-release",
            "version": "9",
            "release": "11.el9",
            "arch": "noarch",
            "repoless_annotation": "No repo source",
            "repoless_cached": true,
            "cache_path": host_cached.to_string_lossy(),
        }),
    ));

    let (status, body) = post_upload(
        &app,
        &[file_part("epel-release-9-12.el9.noarch.rpm", b"an rpm")],
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        std::fs::read(&host_cached).unwrap(),
        b"the host's cached rpm".as_slice(),
        "the host's cached RPM is not this upload's to delete"
    );
    assert_eq!(
        dir_entries(&dir),
        vec!["epel-release-9-12.el9.noarch.rpm".to_string()]
    );
}

/// Two repo-less packages whose NEVRA filenames overlap the matcher:
/// `epel-release-extras` is cached from the host's dnf cache and
/// classified (empty `repoless_annotation`, so the matcher never offers
/// it), and `epel-release` is the one an upload named for the first will
/// match, because the matcher shortens `epel-release-extras` to
/// `epel-release` before it gives up.
fn colliding_nevra_state(tmp: &tempfile::TempDir, dnf_cache: &std::path::Path) -> Arc<AppState> {
    let snap: InspectionSnapshot = serde_json::from_value(serde_json::json!({
        "schema_version": inspectah_core::snapshot::SCHEMA_VERSION,
        "rpm": { "packages_added": [
            {
                "name": "epel-release-extras",
                "version": "9",
                "release": "11.el9",
                "arch": "noarch",
                "repoless_cached": true,
                "cache_path": dnf_cache.to_string_lossy(),
            },
            {
                "name": "epel-release",
                "version": "9",
                "release": "11.el9",
                "arch": "noarch",
                "repoless_annotation": "No repo source",
            },
        ] },
    }))
    .expect("the scan snapshot deserializes");
    let mut session = RefineSession::new(snap);
    session.set_upload_dir(tmp.path().join(UPLOAD_SUBDIR));
    Arc::new(AppState {
        session: Arc::new(Mutex::new(session)),
        sections_cache: OnceLock::new(),
    })
}

#[tokio::test]
async fn upload_refuses_a_filename_a_cached_package_already_owns() {
    // The collision the export discovers at `create_new`, refused where
    // the operator can still act on it. The cached package's `cache_path`
    // is its dnf-cache path, not this destination, so the staged-file
    // check sees nothing; what collides is the NEVRA filename the export
    // extracts for it from the source tarball.
    //
    // `a_destination_another_step_already_wrote_fails_the_export` in the
    // refine crate is the other end: that is what this prevents.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    let dnf_cache = tmp.path().join("epel-release-extras-9-11.el9.noarch.rpm");
    std::fs::write(&dnf_cache, b"the host's cached rpm").unwrap();
    let app = app(colliding_nevra_state(&tmp, &dnf_cache));
    let before = view(&app).await;

    let (status, body) = post_upload(
        &app,
        &[file_part(
            "epel-release-extras-9-11.el9.noarch.rpm",
            b"an rpm",
        )],
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["error"],
        "epel-release-extras-9-11.el9.noarch.rpm is the cached RPM filename of \
         epel-release-extras.noarch; upload it under a different name or remove \
         epel-release-extras.noarch from the export"
    );
    assert_eq!(
        dir_entries(&dir),
        Vec::<String>::new(),
        "a refused upload places nothing"
    );
    assert_eq!(
        std::fs::read(&dnf_cache).unwrap(),
        b"the host's cached rpm".as_slice()
    );
    let after = view(&app).await;
    assert_eq!(after["generation"], before["generation"]);
    assert_eq!(after["packages"], before["packages"]);

    // The same bytes under a name nothing else owns are accepted, so the
    // refusal is about the name and not about the pair of packages.
    let (status, body) = post_upload(
        &app,
        &[file_part("epel-release-9-12.el9.noarch.rpm", b"an rpm")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["matched"], "epel-release.noarch");
}

#[tokio::test]
async fn deleting_a_dnf_cached_package_is_refused_not_reported_missing() {
    // § 3.3: the precondition is `uploaded`, and a package whose RPM the
    // host's dnf cache supplies has one -- the row shows it. 404 would
    // read as "that package is not here"; 409 says the file is not
    // refine's to unlink.
    let tmp = tempfile::tempdir().unwrap();
    let dnf_cache = tmp.path().join("epel-release-9-11.el9.noarch.rpm");
    std::fs::write(&dnf_cache, b"the host's cached rpm").unwrap();
    let app = app(upload_state_from_scan(
        &tmp,
        serde_json::json!({
            "name": "epel-release",
            "version": "9",
            "release": "11.el9",
            "arch": "noarch",
            "repoless_annotation": "No repo source",
            "repoless_cached": true,
            "cache_path": dnf_cache.to_string_lossy(),
        }),
    ));

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/upload-rpm/epel-release.noarch")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["error"],
        "only uploads can be removed; epel-release.noarch is cached from the host's dnf cache"
    );
    assert!(dnf_cache.exists(), "and nothing is deleted");
    assert_eq!(
        view_package(&view(&app).await, "epel-release")["repoless_cached"],
        true
    );

    // A package with no RPM at all is still 404: there is nothing to
    // refuse, which is a different answer from refusing.
    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/upload-rpm/nothing.noarch")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// Stage one part file through the real `stage`, without committing it.
async fn stage_part(
    dir: &std::path::Path,
    filename: &str,
    content: &[u8],
) -> inspectah_web::upload::StagedPart {
    use axum::extract::{FromRequest, Multipart};
    let body = Body::from(multipart_body(&[file_part(filename, content)]));
    let mut multipart = Multipart::from_request(upload_request(body), &())
        .await
        .unwrap();
    let mut field = multipart.next_field().await.unwrap().unwrap();
    inspectah_web::upload::stage(dir, &mut field).await.unwrap()
}

#[tokio::test]
async fn overlapping_uploads_are_decided_by_the_last_commit() {
    // § 8.2: `stage` and `commit` are split so two uploads can be
    // interleaved deterministically. Both are staged before either
    // commits, which is the state two concurrent requests reach.
    const FILENAME: &str = "epel-release-9-12.el9.noarch.rpm";

    for (first, second) in [(&b"AAAA"[..], &b"BBBB"[..]), (&b"BBBB"[..], &b"AAAA"[..])] {
        let (state, tmp) = upload_state();
        let dir = tmp.path().join(UPLOAD_SUBDIR);
        {
            state.session.lock().unwrap().ensure_upload_dir().unwrap();
        }
        let a = stage_part(&dir, FILENAME, first).await;
        let b = stage_part(&dir, FILENAME, second).await;
        assert_eq!(
            part_files(&dir).len(),
            2,
            "each request stages its own part file"
        );

        let name = inspectah_core::rpm_filename::StagedName::parse(FILENAME).unwrap();
        {
            let mut session = state.session.lock().unwrap();
            inspectah_web::upload::commit(&mut session, a, &name).unwrap();
            inspectah_web::upload::commit(&mut session, b, &name).unwrap();
        }

        assert_eq!(dir_entries(&dir), vec![FILENAME.to_string()]);
        assert_eq!(
            std::fs::read(dir.join(FILENAME)).unwrap(),
            second,
            "the commit that ran last wins"
        );
    }

    // A dropped part takes only its own file: the other upload is
    // untouched and still commits.
    let (state, tmp) = upload_state();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    {
        state.session.lock().unwrap().ensure_upload_dir().unwrap();
    }
    let a = stage_part(&dir, FILENAME, b"AAAA").await;
    let b = stage_part(&dir, FILENAME, b"BBBB").await;
    drop(a);
    assert_eq!(part_files(&dir).len(), 1, "only A's part file is gone");
    let name = inspectah_core::rpm_filename::StagedName::parse(FILENAME).unwrap();
    {
        let mut session = state.session.lock().unwrap();
        inspectah_web::upload::commit(&mut session, b, &name).unwrap();
    }
    assert_eq!(dir_entries(&dir), vec![FILENAME.to_string()]);
    assert_eq!(
        std::fs::read(dir.join(FILENAME)).unwrap(),
        b"BBBB".as_slice()
    );
}

/// A request body that yields `prologue`, then `chunk` `full` times, then
/// a `rest`-byte tail, and then either ends or stalls forever -- the
/// client that stops sending. Lazy, so a 500 MiB body costs one chunk of
/// memory.
struct ChunkStream {
    prologue: Option<axum::body::Bytes>,
    chunk: axum::body::Bytes,
    full: usize,
    rest: usize,
    stall: bool,
}

impl futures_core::Stream for ChunkStream {
    type Item = Result<axum::body::Bytes, std::io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;
        if let Some(prologue) = self.prologue.take() {
            return Poll::Ready(Some(Ok(prologue)));
        }
        if self.full > 0 {
            self.full -= 1;
            return Poll::Ready(Some(Ok(self.chunk.clone())));
        }
        if self.rest > 0 {
            let rest = self.rest;
            self.rest = 0;
            return Poll::Ready(Some(Ok(self.chunk.slice(..rest))));
        }
        if self.stall {
            return Poll::Pending;
        }
        Poll::Ready(None)
    }
}

/// Chunk size the oversize-body test streams with: large enough that 500
/// MiB is a few hundred polls, small enough that the test holds one of
/// them in memory.
const TEST_CHUNK_BYTES: usize = 1024 * 1024;

#[tokio::test]
async fn upload_over_the_route_limit_is_413_and_leaves_no_part_file() {
    let (state, tmp) = upload_state();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    let app = app(state);

    // A 2 MiB body goes through, which the router's 1 MiB default limit
    // would refuse: the route's own UPLOAD_LIMIT_BYTES is the one in
    // force.
    let (status, body) = post_upload(
        &app,
        &[file_part(
            "epel-release-9-12.el9.noarch.rpm",
            &vec![b'x'; 2 * TEST_CHUNK_BYTES],
        )],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    std::fs::remove_file(dir.join("epel-release-9-12.el9.noarch.rpm")).unwrap();

    // One byte over the limit, counting the multipart framing.
    let header = format!(
        "--{UPLOAD_BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; \
         filename=\"epel-release-9-12.el9.noarch.rpm\"\r\n\r\n"
    );
    let truncated_body = |total: usize| {
        let filler = total - header.len();
        Body::from_stream(ChunkStream {
            prologue: Some(axum::body::Bytes::from(header.clone())),
            chunk: axum::body::Bytes::from(vec![b'x'; TEST_CHUNK_BYTES]),
            full: filler / TEST_CHUNK_BYTES,
            rest: filler % TEST_CHUNK_BYTES,
            stall: false,
        })
    };

    // Exactly at the limit the body is not refused for its size: it runs
    // out of multipart framing instead, which is the 400 below. Without
    // this case the 413 above could be any failure on a large body.
    let (status, body) =
        post_upload_body(&app, truncated_body(inspectah_web::UPLOAD_LIMIT_BYTES)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(dir_entries(&dir), Vec::<String>::new());

    let (status, body) =
        post_upload_body(&app, truncated_body(inspectah_web::UPLOAD_LIMIT_BYTES + 1)).await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert_eq!(
        dir_entries(&dir),
        Vec::<String>::new(),
        "the part file of a body that overran the limit does not survive"
    );
}

/// How long a test waits for a concurrent upload to make progress
/// before it calls the server stuck.
const UPLOAD_WAIT: std::time::Duration = std::time::Duration::from_secs(5);
/// How often it looks.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);

/// A body that sends a complete multipart header and the first bytes of
/// the file, then stops: the client that goes away mid-transfer.
fn stalled_upload_body(filename: &str) -> Body {
    let mut first = format!(
        "--{UPLOAD_BOUNDARY}\r\nContent-Disposition: form-data; \
         name=\"file\"; filename=\"{filename}\"\r\n\r\n"
    )
    .into_bytes();
    first.extend_from_slice(b"the first half of an rpm");
    Body::from_stream(ChunkStream {
        prologue: Some(axum::body::Bytes::from(first)),
        chunk: axum::body::Bytes::new(),
        full: 0,
        rest: 0,
        stall: true,
    })
}

/// Wait until the stalled upload's part file is on disk, so what follows
/// is measuring a transfer in flight rather than one that never started.
async fn await_part_file(dir: &std::path::Path) {
    tokio::time::timeout(UPLOAD_WAIT, async {
        while part_files(dir).len() != 1 {
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    })
    .await
    .expect("the upload never reached its part file");
}

#[tokio::test]
async fn upload_abandoned_mid_body_leaves_no_part_file() {
    let (state, tmp) = upload_state();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    let app = app(state);

    let body = stalled_upload_body("epel-release-9-12.el9.noarch.rpm");
    let handle = tokio::spawn(async move { app.oneshot(upload_request(body)).await });

    // The part file exists while the transfer is in flight.
    await_part_file(&dir).await;

    // The client goes away: axum drops the handler future.
    handle.abort();
    let _ = handle.await;

    assert_eq!(
        dir_entries(&dir),
        Vec::<String>::new(),
        "an abandoned transfer leaves nothing behind"
    );
}

// Two workers, so that a `stage` moved inside the lock hold blocks one
// of them and leaves the other free to fire the timeout. On the
// single-threaded runtime the other tests use, that mutation would hang
// the suite instead of failing this test.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stalled_upload_does_not_block_another_request() {
    // What the stage/commit split exists for: the streaming happens
    // outside the session lock, so a client that stops sending holds its
    // own part file and nothing else. The interleaving test cannot show
    // this, because it takes the lock itself.
    let (state, tmp) = upload_state();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    let app = app(state);

    let stalled = tokio::spawn({
        let app = app.clone();
        let body = stalled_upload_body("epel-release-latest-9.noarch.rpm");
        async move { app.oneshot(upload_request(body)).await }
    });
    await_part_file(&dir).await;

    let (status, body) = tokio::time::timeout(
        UPLOAD_WAIT,
        post_upload(
            &app,
            &[file_part("epel-release-9-12.el9.noarch.rpm", b"an rpm")],
        ),
    )
    .await
    .expect("a stalled upload must not hold the session lock");

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        dir_entries(&dir).len(),
        2,
        "the staged file, and the stalled request's part file beside it"
    );
    stalled.abort();
}

#[tokio::test]
async fn upload_directory_is_swept_of_dead_part_files_on_first_open() {
    let (state, tmp) = upload_state();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join(".upload-dead.part"), b"half an rpm").unwrap();
    let app = app(state);

    let (status, body) = post_upload(
        &app,
        &[file_part("epel-release-9-12.el9.noarch.rpm", b"an rpm")],
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        dir_entries(&dir),
        vec!["epel-release-9-12.el9.noarch.rpm".to_string()],
        "the part file a dead process left is gone, and this upload's is too"
    );
}
