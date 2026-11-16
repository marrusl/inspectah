#[path = "../../cli/tests/support/unit_hosts.rs"]
mod unit_hosts;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::config::{
    ConfigCategory, ConfigFileEntry, ConfigFileKind, ConfigSection,
};
use inspectah_core::types::containers::{
    ComposeFile, ComposeService, ContainerSection, FlatpakApp, QuadletUnit, RunningContainer,
};
use inspectah_core::types::kernelboot::KernelBootSection;
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
use inspectah_core::types::storage::{FstabEntry, StorageSection};
use inspectah_core::types::units::{
    PresetDefault, SystemdDropIn, UnitInventoryEntry, UnitSection, UnitState, UnitStateChange,
};
use inspectah_core::types::users::UserGroupSection;
use inspectah_core::types::warnings::{Warning, WarningSeverity};
use inspectah_refine::session::RefineSession;
use inspectah_web::adapter::build_web_sections;
use inspectah_web::handlers::AppState;
use std::sync::{Arc, Mutex};
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
    })
}

fn app(state: Arc<AppState>) -> axum::Router {
    inspectah_web::router(state, "http://localhost:8642")
        .layer(axum::middleware::map_request(with_loopback_host))
}

/// A `oneshot` request carries no `Host` unless the test sets one, and the
/// router refuses a request without a loopback `Host`. A real client
/// always sends one, so the helper supplies it unless the test chose its
/// own.
async fn with_loopback_host(mut req: Request<Body>) -> Request<Body> {
    req.headers_mut()
        .entry(axum::http::header::HOST)
        .or_insert(axum::http::HeaderValue::from_static("localhost:8642"));
    req
}

async fn status_with_host(app: axum::Router, method: &str, path: &str, host: &str) -> StatusCode {
    app.oneshot(
        Request::builder()
            .method(method)
            .uri(path)
            .header("host", host)
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await
    .unwrap()
    .status()
}

/// DNS rebinding: a page on `attacker.example:8642` whose name now
/// resolves to 127.0.0.1 is same-origin with the server as far as the
/// browser is concerned, and its `Host` header is the only thing that
/// still names the attacker. The static-asset fallback is covered too:
/// it is installed after the API layers, so a guard placed with them
/// would not reach it.
#[tokio::test]
async fn a_foreign_host_is_refused_on_api_routes_and_the_fallback() {
    for path in ["/api/view", "/api/health", "/", "/assets/x.js", "/api/nope"] {
        let status =
            status_with_host(app(test_state()), "GET", path, "attacker.example:8642").await;
        assert_eq!(status, StatusCode::FORBIDDEN, "GET {path}");
    }
    let status = status_with_host(app(test_state()), "POST", "/api/undo", "attacker.example").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "POST /api/undo");
}

#[tokio::test]
async fn loopback_hosts_pass_on_any_port() {
    for host in [
        "127.0.0.1:8642",
        "localhost:8642",
        "127.0.0.1:9000",
        "[::1]:8642",
        "LocalHost:8642",
        "localhost",
        "127.0.0.1",
        "[::1]",
    ] {
        let status = status_with_host(app(test_state()), "GET", "/api/health", host).await;
        assert_eq!(status, StatusCode::OK, "Host: {host}");
    }
}

#[tokio::test]
async fn loopback_lookalike_hosts_are_refused() {
    for host in [
        "localhost.attacker.example",
        "localhost.attacker.example:8642",
        "127.0.0.1.nip.io:8642",
        "attacker.example:8642@127.0.0.1",
        "[::1].attacker.example",
        "[::1]x:8642",
        "127.0.0.1:86x2",
        "127.0.0.1:",
        "localhost.",
        "",
    ] {
        let status = status_with_host(app(test_state()), "GET", "/api/health", host).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "Host: {host:?}");
    }
}

#[tokio::test]
async fn a_request_without_host_is_refused() {
    // The raw router, without the helper that supplies a Host.
    let response = inspectah_web::router(test_state(), "http://localhost:8642")
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// Sends `req` to the raw router, without the helper that supplies a Host,
/// so the test controls every Host field and the HTTP version.
async fn raw_status(req: Request<Body>) -> StatusCode {
    inspectah_web::router(test_state(), "http://localhost:8642")
        .oneshot(req)
        .await
        .unwrap()
        .status()
}

async fn status_with_two_hosts(first: &str, second: &str) -> StatusCode {
    raw_status(
        Request::builder()
            .uri("/api/health")
            .header("host", first)
            .header("host", second)
            .body(Body::empty())
            .unwrap(),
    )
    .await
}

/// HTTP/1.1 requires exactly one Host field (RFC 9112 §3.2). A foreign
/// second value must not hide behind a loopback first one.
#[tokio::test]
async fn a_foreign_second_host_field_is_refused() {
    let status = status_with_two_hosts("127.0.0.1:8642", "attacker.example:8642").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// Two Host fields are refused even when both name the loopback, rather
/// than picking one.
#[tokio::test]
async fn two_loopback_host_fields_are_refused() {
    let status = status_with_two_hosts("127.0.0.1:8642", "localhost:8642").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// An HTTP/1.1 absolute-form target still needs its Host field; the
/// target's own authority does not stand in for it.
#[tokio::test]
async fn an_http1_absolute_form_target_without_host_is_refused() {
    let status = raw_status(
        Request::builder()
            .uri("http://127.0.0.1:8642/api/health")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// HTTP/2 carries the authority in `:authority`, which hyper maps into the
/// request URI, and a client need not send Host as well (RFC 9113 §8.3.1).
/// That authority is still checked.
#[tokio::test]
async fn an_http2_request_with_only_an_authority_is_checked_on_it() {
    for (authority, expected) in [
        ("127.0.0.1:8642", StatusCode::OK),
        ("attacker.example:8642", StatusCode::FORBIDDEN),
    ] {
        let status = raw_status(
            Request::builder()
                .version(axum::http::Version::HTTP_2)
                .uri(format!("http://{authority}/api/health"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, expected, ":authority {authority}");
    }
}

/// An absolute-form request target names its own authority, which takes
/// precedence over `Host`; a loopback `Host` must not launder it.
#[tokio::test]
async fn an_absolute_form_target_with_a_foreign_authority_is_refused() {
    let status = status_with_host(
        app(test_state()),
        "GET",
        "http://attacker.example:8642/api/health",
        "127.0.0.1:8642",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// A host where `apr` is a non-leaf package the operator starts with
/// excluded. Re-including it is a decision only `original_includes` can
/// carry into the Containerfile: the leaf filter drops every package not
/// in `leaf_packages`, and the bypass for an operator-changed disposition
/// is the one thing that keeps `apr` in the install list.
fn non_leaf_reinclude_state() -> Arc<AppState> {
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
                name: "apr".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                disposition: FindingKind::excluded(),
                locked: false,
                // Without a repo source the renderer emits a TODO comment
                // instead of an install, and the assertion below could not
                // tell the two apart.
                source_repo: "appstream".into(),
                ..Default::default()
            },
        ],
        // `apr` is deliberately absent, so only the original-includes
        // bypass can keep it.
        leaf_packages: Some(vec!["httpd.x86_64".into()]),
        ..Default::default()
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
    })
}

/// The arguments `POST /api/tarball` renders with are pinned by the
/// artifact it returns, not by a second copy of the argument list.
///
/// **This test exists because there was no such pin.** Until 2026-11-04
/// the route assembled its own argument list and the only test that
/// exercised the same render reimplemented that list rather than driving
/// the route, so the two could never disagree in a way a test could see.
/// A mutation that made the route pass `None` for `original_includes`
/// passed the entire workspace suite. It fails here: dropping that
/// argument lets the leaf filter remove `apr`, and the operator's
/// re-inclusion never reaches the image they download.
#[tokio::test]
async fn the_download_carries_the_operators_decision_not_just_the_leaf_set() {
    let app = app(non_leaf_reinclude_state());

    let (status, _) = post_json(
        &app,
        "/api/op",
        serde_json::json!({
            "kind": "Op",
            "op": "SetInclude",
            "target": {
                "item_id": {"kind": "Package", "key": {"name": "apr", "arch": "x86_64"}},
                "include": true
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "re-including apr must succeed");

    let (status, _headers, body) = post_tarball(&app).await;
    assert_eq!(status, StatusCode::OK, "the download must succeed");

    let containerfile =
        String::from_utf8(tarball_member(&body, "Containerfile").expect("Containerfile")).unwrap();
    // Only the active install block counts. A package the renderer drops
    // still appears as a `# TODO:` follow-up comment, so a substring
    // search over the whole file cannot tell "installed" from "mentioned".
    let installed: Vec<&str> = containerfile
        .lines()
        .skip_while(|l| !l.starts_with("RUN dnf install -y"))
        .skip(1)
        .take_while(|l| l.starts_with("    "))
        .map(|l| l.trim().trim_end_matches(" \\"))
        .collect();

    assert!(
        installed.contains(&"apr"),
        "the operator re-included `apr`, so the downloaded Containerfile must \
         install it. Its absence means the route rendered without \
         `original_includes` and the leaf filter silently discarded the \
         decision.\nactive install list: {installed:?}\n{containerfile}"
    );
}

/// A host where the one leaf, `httpd`, needs nothing, so the auto package
/// `firewalld`, which owns the enabled `firewalld.service`, is not
/// carried.
fn not_carried_state() -> Arc<AppState> {
    let mut snap = InspectionSnapshot::new();
    let pkg = |name: &str, arch: &str, repo: &str| PackageEntry {
        name: name.into(),
        arch: arch.into(),
        state: PackageState::Added,
        source_repo: repo.into(),
        ..Default::default()
    };
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            pkg("httpd", "x86_64", "appstream"),
            pkg("firewalld", "noarch", "baseos"),
        ],
        leaf_packages: Some(vec!["httpd.x86_64".into()]),
        auto_packages: Some(vec!["firewalld.noarch".into()]),
        leaf_dep_tree: serde_json::json!({ "httpd.x86_64": [] }),
        file_ownership: vec![inspectah_core::types::rpm::FileOwnershipEntry {
            package_name: "firewalld".into(),
            paths: vec!["/usr/lib/systemd/system/firewalld.service".into()],
        }],
        ..Default::default()
    });
    snap.units = Some(UnitSection {
        inventory: vec![UnitInventoryEntry {
            unit: "firewalld.service"
                .to_owned()
                .try_into()
                .expect("valid unit name"),
            state: UnitState::Enabled,
            preset: None,
        }],
        ..Default::default()
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
    })
}

/// `POST /api/op` for a package `SetInclude`, as the packages view sends it.
async fn set_package_include(app: &axum::Router, name: &str, arch: &str, include: bool) {
    let (status, json) = post_json(
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
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
}

/// The view carries the not-carried rows and the notice the export's
/// header will carry, and both follow an include and a re-exclude.
#[tokio::test]
async fn the_view_carries_the_not_carried_rows_and_notice() {
    let app = app(not_carried_state());

    let (status, json) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["not_carried_packages"],
        serde_json::json!([{
            "item_id": {"kind": "Package", "key": {"name": "firewalld", "arch": "noarch"}},
            "display_name": "firewalld",
            "source_repo": "baseos",
            "enabled_units": ["firewalld.service"],
            "owns_enabled_unit": true
        }])
    );
    let notice = json["not_carried_notice"].clone();
    assert_eq!(notice["kind"], "notice", "{notice}");
    assert!(
        notice["text"].as_str().unwrap().contains("firewalld"),
        "{notice}"
    );

    // The view's notice is the header's notice, byte for byte, so the
    // Export dialog can recognise the one it already showed.
    let (status, headers, _) = post_tarball(&app).await;
    assert_eq!(status, StatusCode::OK);
    let header: serde_json::Value =
        serde_json::from_str(headers["x-export-warnings"].to_str().unwrap()).unwrap();
    assert_eq!(header, serde_json::json!([notice.clone()]));

    set_package_include(&app, "firewalld", "noarch", true).await;
    let (_, json) = get_json(&app, "/api/view").await;
    assert_eq!(json["not_carried_packages"], serde_json::json!([]));
    assert_eq!(json["not_carried_notice"], serde_json::Value::Null);

    set_package_include(&app, "firewalld", "noarch", false).await;
    let (_, json) = get_json(&app, "/api/view").await;
    assert_eq!(json["not_carried_packages"].as_array().unwrap().len(), 1);
    assert_eq!(json["not_carried_notice"], notice);
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

/// Path of the full-shadow unit file in [`units_view_state`].
const FULL_SHADOW_PATH: &str = "/etc/systemd/system/sshd.service";

/// A unit whose state differs from its preset, with a drop-in, and an
/// enabled unit at its preset whose only finding is a full shadow. The
/// collector writes no state change for the second one, and the config
/// entry at the shadow's path is owned by the drop-in.
fn units_view_state() -> Arc<AppState> {
    let mut snap = InspectionSnapshot::new();
    snap.units = Some(UnitSection {
        state_changes: vec![
            UnitStateChange::new(
                test_carried_unit(
                    "kdump.service".to_owned().try_into().unwrap(),
                    Some("kexec-tools".into()),
                ),
                UnitState::Disabled,
                Some(PresetDefault::Enable),
            )
            .unwrap(),
        ],
        drop_ins: vec![
            SystemdDropIn {
                unit: "kdump.service".into(),
                path: "/etc/systemd/system/kdump.service.d/override.conf".into(),
                content: "[Service]\nTimeoutSec=600\n".into(),
                disposition: FindingKind::included(),
                locked: false,
                shadow_type: Some(inspectah_core::types::ShadowType::DropIn),
                ..Default::default()
            },
            SystemdDropIn {
                unit: "sshd.service".into(),
                path: FULL_SHADOW_PATH.into(),
                content: "[Service]\nExecStart=/usr/sbin/sshd -D\n".into(),
                disposition: FindingKind::included(),
                locked: false,
                shadow_type: Some(inspectah_core::types::ShadowType::FullShadow),
                shadow_rationale: Some("Full unit override replaces vendor sshd.service".into()),
                ..Default::default()
            },
        ],
        inventory: vec![],
        mount_findings: Vec::new(),
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
    });
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: FULL_SHADOW_PATH.into(),
            kind: ConfigFileKind::Unowned,
            disposition: FindingKind::inventory(),
            locked: false,
            shadowed_by: Some(inspectah_core::types::config::ConfigShadow::Units),
            ..Default::default()
        }],
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
    })
}

/// T3 and T4 through the production handlers: the view names the unit
/// rows `unit_states` and `unit_dropins`, a full shadow on a unit at its
/// preset is a drop-in row and not a state row, and the old keys are gone.
#[tokio::test]
async fn the_view_carries_unit_fields_and_no_service_fields() {
    let app = app(units_view_state());
    let (status, json) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);

    let states = json["unit_states"]
        .as_array()
        .expect("the view carries unit_states");
    let state_units: Vec<&str> = states.iter().filter_map(|s| s["unit"].as_str()).collect();
    assert_eq!(state_units, ["kdump.service"], "{json}");
    assert!(
        states[0].get("shadow_type").is_none(),
        "a state row carries no shadow: {}",
        states[0]
    );

    let dropins = json["unit_dropins"]
        .as_array()
        .expect("the view carries unit_dropins");
    assert_eq!(dropins.len(), 2, "{json}");
    let full_shadows: Vec<&str> = dropins
        .iter()
        .filter(|d| d["shadow_type"] == "full_shadow")
        .filter_map(|d| d["path"].as_str())
        .collect();
    assert_eq!(full_shadows, [FULL_SHADOW_PATH]);

    let shadowed = json["config_files"]
        .as_array()
        .expect("config_files")
        .iter()
        .find(|c| c["entry"]["path"] == FULL_SHADOW_PATH)
        .expect("the shadowed config entry is in the view");
    assert_eq!(shadowed["entry"]["shadowed_by"], "units");

    // The new keys are present with content above; the old ones are gone.
    assert!(json.get("service_states").is_none(), "{json}");
    assert!(json.get("service_dropins").is_none(), "{json}");
}

/// The context section list names the unit section `units`.
#[tokio::test]
async fn the_context_sections_name_the_unit_section_units() {
    let app = app(rich_state());
    let (status, json) = get_json(&app, "/api/snapshot/sections").await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = json
        .as_array()
        .expect("sections is an array")
        .iter()
        .filter_map(|s| s["id"].as_str())
        .collect();
    assert!(ids.contains(&"units"), "{ids:?}");
    assert!(!ids.contains(&"services"), "{ids:?}");
}

/// Path of the cron job in [`scheduled_timer_state`].
const TIMER_CRON_PATH: &str = "/etc/cron.d/backup";

/// A host with two collected timers and a cron job. `backup.timer` has a
/// state change, so a row in `unit_states`; `cleanup.timer` has none.
fn scheduled_timer_state() -> Arc<AppState> {
    let mut snap = InspectionSnapshot::new();
    snap.units = Some(UnitSection {
        state_changes: vec![
            UnitStateChange::new(
                test_carried_unit("backup.timer".to_owned().try_into().unwrap(), None),
                UnitState::Enabled,
                Some(PresetDefault::Disable),
            )
            .unwrap(),
        ],
        drop_ins: vec![],
        inventory: vec![],
        mount_findings: Vec::new(),
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
    });
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        cron_jobs: vec![CronJob {
            path: TIMER_CRON_PATH.into(),
            source: "0 2 * * * root /usr/local/bin/backup.sh".into(),
            ..Default::default()
        }],
        // The unit the cron job generates, which its operation carries.
        generated_timer_units: vec![generated_backup()],
        systemd_timers: vec![
            SystemdTimer {
                name: "backup".into(),
                on_calendar: "daily".into(),
                exec_start: "/usr/local/bin/backup.sh".into(),
                ..Default::default()
            },
            SystemdTimer {
                name: "cleanup".into(),
                on_calendar: "weekly".into(),
                exec_start: "/usr/local/bin/cleanup.sh".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
    })
}

/// The unit the scheduled inspector generates from [`TIMER_CRON_PATH`].
fn generated_backup() -> inspectah_core::types::scheduled::GeneratedTimerUnit {
    inspectah_core::types::scheduled::GeneratedTimerUnit {
        name: "cron-backup".into(),
        cron_expr: "0 2 * * *".into(),
        source_path: TIMER_CRON_PATH.into(),
        timer_content: "[Timer]\nOnCalendar=*-*-* 02:00:00\n[Install]\nWantedBy=timers.target\n"
            .into(),
        service_content: "[Service]\nType=oneshot\nExecStart=/usr/local/bin/backup.sh\n".into(),
        disposition: FindingKind::included(),
        ..Default::default()
    }
}

/// T20 through the production handlers: a scheduled timer row names its
/// unit and whether that unit has a row, and offers no include flag; the
/// timer's unit row keeps its own.
#[tokio::test]
async fn a_scheduled_timer_row_names_its_unit_and_offers_no_include() {
    let app = app(scheduled_timer_state());
    let (status, sections) = get_json(&app, "/api/snapshot/sections").await;
    assert_eq!(status, StatusCode::OK);
    let items = sections
        .as_array()
        .expect("sections is an array")
        .iter()
        .find(|s| s["id"] == "scheduled_tasks")
        .expect("a scheduled_tasks section")["items"]
        .as_array()
        .expect("scheduled items")
        .clone();
    let row = |id: &str| {
        items
            .iter()
            .find(|i| i["id"] == id)
            .unwrap_or_else(|| panic!("no scheduled row {id}: {items:?}"))
            .clone()
    };

    let backup = row("timer:backup");
    assert_eq!(backup["unit"], "backup.timer", "{backup}");
    assert_eq!(backup["unit_row"], true, "{backup}");
    let cleanup = row("timer:cleanup");
    assert_eq!(cleanup["unit"], "cleanup.timer", "{cleanup}");
    assert_eq!(cleanup["unit_row"], false, "{cleanup}");
    for timer in [&backup, &cleanup] {
        assert!(timer.get("include").is_none(), "{timer}");
        assert!(timer.get("decision").is_none(), "{timer}");
    }
    // A cron row is not a unit and names none.
    let cron = row(&format!("cron:{TIMER_CRON_PATH}"));
    assert!(cron.get("unit").is_none(), "{cron}");
    assert!(cron.get("unit_row").is_none(), "{cron}");

    // The positive control: the timer's unit row carries the flag.
    let (status, view) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);
    let unit_row = view["unit_states"]
        .as_array()
        .expect("unit_states")
        .iter()
        .find(|s| s["unit"] == "backup.timer")
        .unwrap_or_else(|| panic!("no unit row for backup.timer: {view}"));
    assert_eq!(unit_row["include"], true, "{unit_row}");
    assert!(
        !view.to_string().contains("SystemdTimer"),
        "the view names no timer operation"
    );
}

/// The services and scheduling group toggle reaches the cron job and the
/// timer's unit row, and emits no timer operation.
#[tokio::test]
async fn the_scheduling_group_toggle_emits_no_timer_operation() {
    let state = scheduled_timer_state();
    let (status, body) = post_json(
        &app(state.clone()),
        "/api/batch-toggle/services-scheduling",
        serde_json::json!({"include": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = timeline_items(&state);
    assert!(
        items.contains(&inspectah_refine::types::ItemId::CronJob {
            path: TIMER_CRON_PATH.into()
        }),
        "{items:?}"
    );
    assert!(
        items.contains(&inspectah_refine::types::ItemId::Unit {
            unit: "backup.timer".to_owned().try_into().unwrap()
        }),
        "{items:?}"
    );
    let kinds: Vec<serde_json::Value> = items
        .iter()
        .map(|item| serde_json::to_value(item).expect("an item id serializes")["kind"].clone())
        .collect();
    assert!(
        !kinds.contains(&serde_json::json!("SystemdTimer")),
        "{kinds:?}"
    );
}

/// A scheduled section with the cron job of [`scheduled_timer_state`] and
/// its generated unit, a cron job whose file holds no entry, and an at job.
fn scheduled_bulk_state() -> Arc<AppState> {
    let mut snap = InspectionSnapshot::new();
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        cron_jobs: vec![
            CronJob {
                path: TIMER_CRON_PATH.into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            CronJob {
                path: QUIET_CRON_PATH.into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        generated_timer_units: vec![generated_backup()],
        at_jobs: vec![inspectah_core::types::scheduled::AtJob {
            file: "var/spool/at/a0000101b2c3d4".into(),
            command: "/usr/local/bin/one-off".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
    })
}

/// A cron job whose file holds no entry, so it generates no unit.
const QUIET_CRON_PATH: &str = "/etc/cron.d/quiet";

/// What the projected snapshot of `state` exports of `cron-backup`: whether
/// its timer and service are staged, and whether the plan enables the
/// timer from the scheduled section.
fn generated_backup_exported(state: &AppState) -> (bool, bool, bool) {
    use inspectah_pipeline::render::configtree::staged_writes;
    use inspectah_pipeline::render::unit_plan::plan_units;
    let snap = state.session.lock().unwrap().snapshot_projected();
    let staged = staged_writes(&snap);
    let at = |name: &str| {
        staged
            .writes
            .keys()
            .any(|d| d.as_str() == format!("/etc/systemd/system/{name}"))
    };
    let enabled = plan_units(&snap, &staged)
        .scheduled
        .iter()
        .any(|p| p.name().as_str() == "cron-backup.timer");
    (at("cron-backup.timer"), at("cron-backup.service"), enabled)
}

/// § 8 through the production handler: the services and scheduling group
/// toggle emits the operations that change the export, a cron job that
/// generates a unit and the generated-timer row, and none that would be
/// refused, the at job and the cron job that generates nothing. Excluding
/// drops the generated pair and its enable from what the export stages and
/// plans; including again restores them. Presence first.
#[tokio::test]
async fn the_scheduling_group_toggle_carries_and_drops_generated_units() {
    use inspectah_refine::types::ItemId;
    let state = scheduled_bulk_state();
    assert_eq!(generated_backup_exported(&state), (true, true, true));

    let (status, body) = post_json(
        &app(state.clone()),
        "/api/batch-toggle/services-scheduling",
        serde_json::json!({"include": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = timeline_items(&state);
    assert_eq!(
        items,
        [
            ItemId::CronJob {
                path: TIMER_CRON_PATH.into()
            },
            ItemId::GeneratedTimer {
                name: "cron-backup".into()
            },
        ],
        "{items:?}"
    );
    assert_eq!(generated_backup_exported(&state), (false, false, false));

    let (status, body) = post_json(
        &app(state.clone()),
        "/api/batch-toggle/services-scheduling",
        serde_json::json!({"include": true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(generated_backup_exported(&state), (true, true, true));
}

/// A request naming the removed timer operation is refused with the
/// variant named; the same request on the cron job is applied.
#[tokio::test]
async fn api_op_refuses_the_removed_timer_operation() {
    let op = |item_id: serde_json::Value| {
        serde_json::json!({
            "kind": "Op",
            "op": "SetInclude",
            "target": {"item_id": item_id, "include": false}
        })
    };
    let app = app(scheduled_timer_state());
    let (status, json) = post_json(
        &app,
        "/api/op",
        op(serde_json::json!({"kind": "CronJob", "key": {"path": TIMER_CRON_PATH}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let (status, json) = post_json(
        &app,
        "/api/op",
        op(serde_json::json!({"kind": "SystemdTimer", "key": {"name": "backup"}})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    assert!(
        json["error"]
            .as_str()
            .is_some_and(|e| e.contains("unknown variant `SystemdTimer`")),
        "{json}"
    );
}

/// The generated cron timer in [`generated_collision_state`].
const GENERATED_TIMER: &str = "cron-backup";
/// The flatpak provisioning service inspectah generates.
const FLATPAK_PROVISION: &str = "flatpak-provision.service";

/// A host with a generated cron timer `cron-backup` and an included
/// flatpak application. With `collected`, the host also has a collected
/// unit of each generated unit's name, `cron-backup.timer` and
/// `flatpak-provision.service`, both enabled against a `disable` preset,
/// so both generated requests are declined for the name collision, and a
/// collected `cron-backup` timer row in the scheduled section.
fn generated_collision_state(collected: bool) -> Arc<AppState> {
    use inspectah_core::types::scheduled::GeneratedTimerUnit;
    let mut snap = InspectionSnapshot::new();
    if collected {
        let change = |unit: &str| {
            UnitStateChange::new(
                test_carried_unit(unit.to_owned().try_into().unwrap(), None),
                UnitState::Enabled,
                Some(PresetDefault::Disable),
            )
            .unwrap()
        };
        snap.units = Some(UnitSection {
            state_changes: vec![
                change(&format!("{GENERATED_TIMER}.timer")),
                change(FLATPAK_PROVISION),
            ],
            drop_ins: vec![],
            inventory: vec![],
            mount_findings: Vec::new(),
            manual_links: Vec::new(),
            undetermined_installs: Vec::new(),
            default_target: None,
        });
    }
    // With `collected`, the host's own timer of the generated name is a
    // collected timer row in the scheduled section too.
    let systemd_timers = if collected {
        vec![SystemdTimer {
            name: GENERATED_TIMER.into(),
            on_calendar: "daily".into(),
            exec_start: "/usr/bin/vendor-backup".into(),
            ..Default::default()
        }]
    } else {
        vec![]
    };
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        systemd_timers,
        generated_timer_units: vec![GeneratedTimerUnit {
            name: GENERATED_TIMER.into(),
            cron_expr: "0 2 * * *".into(),
            timer_content: "[Timer]\nOnCalendar=daily\n".into(),
            service_content: "[Service]\nExecStart=/usr/bin/backup\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap.containers = Some(ContainerSection {
        flatpak_apps: vec![FlatpakApp {
            app_id: "org.gimp.GIMP".into(),
            origin: "flathub".into(),
            branch: "stable".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
    })
}

/// The view's `declined` for a generated `unit` declined for the name
/// collision: the plan's own marker, reason name and text.
fn collision_declined(unit: &str) -> serde_json::Value {
    use inspectah_core::types::units::UnitAction;
    use inspectah_pipeline::render::unit_plan::{OmissionReason, Reservation, declined_line};
    let reason = OmissionReason::GeneratedNameCollision {
        name: unit.to_owned().try_into().unwrap(),
        reserved_by: Reservation::Row,
    };
    let line = declined_line(
        &unit.to_owned().try_into().unwrap(),
        UnitAction::Enable,
        &reason,
    );
    serde_json::json!({
        "renders": "fixme",
        "reason": "GeneratedNameCollision",
        "text": line.text,
    })
}

/// § 11.1 and T21's refine half through the production handlers: a
/// generated cron timer and the flatpak provisioning service, each named
/// for a collected unit, are declined on the scheduled row and on the
/// flatpak row that asked for them, with the plan's reason and text.
/// Control: without the collected units nothing is declined.
#[tokio::test]
async fn a_declined_generated_unit_is_carried_for_its_row() {
    let (status, view) = get_json(&app(generated_collision_state(true)), "/api/view").await;
    assert_eq!(status, StatusCode::OK);
    let timer = format!("{GENERATED_TIMER}.timer");
    assert_eq!(
        view["declined_generated"],
        serde_json::json!([
            {
                "row": {"row": "scheduled", "name": GENERATED_TIMER},
                "unit": timer,
                "declined": collision_declined(&timer),
            },
            {
                "row": {"row": "flatpak"},
                "unit": FLATPAK_PROVISION,
                "declined": collision_declined(FLATPAK_PROVISION),
            },
        ]),
        "{view}"
    );
    let text = view["declined_generated"][0]["declined"]["text"]
        .as_str()
        .expect("a text");
    assert!(
        text.starts_with("cron-backup.timer (inspectah generates this unit"),
        "{text}"
    );

    let (status, control) = get_json(&app(generated_collision_state(false)), "/api/view").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        control["declined_generated"],
        serde_json::json!([]),
        "{control}"
    );
}

/// Every scheduled row of [`scheduled_identity_state`]'s sections
/// response, as `(id, generated_timer)`.
async fn scheduled_rows(state: Arc<AppState>) -> Vec<(String, Option<String>)> {
    let (status, sections) = get_json(&app(state), "/api/snapshot/sections").await;
    assert_eq!(status, StatusCode::OK);
    sections
        .as_array()
        .expect("sections is an array")
        .iter()
        .find(|s| s["id"] == "scheduled_tasks")
        .expect("a scheduled_tasks section")["items"]
        .as_array()
        .expect("scheduled items")
        .iter()
        .map(|i| {
            (
                i["id"].as_str().expect("an id").to_owned(),
                i["generated_timer"].as_str().map(str::to_owned),
            )
        })
        .collect()
}

/// A host whose scheduled section holds every pair of rows that once
/// shared an id: a shadowed timer's `/etc` and vendor descriptions, a
/// collected timer named `generated:cron-backup.timer` beside the
/// generated `cron-backup` row, and two generated rows of one name from
/// two lines of one cron file.
fn scheduled_identity_state() -> Arc<AppState> {
    use inspectah_core::types::scheduled::{AtJob, GeneratedTimerUnit};
    let timer = |name: &str, path: &str| SystemdTimer {
        name: name.into(),
        path: path.into(),
        on_calendar: "daily".into(),
        ..Default::default()
    };
    let generated = |command: &str| GeneratedTimerUnit {
        name: "cron-backup".into(),
        cron_expr: "0 2 * * *".into(),
        source_path: "etc/cron.d/backup".into(),
        command: command.into(),
        disposition: FindingKind::included(),
        ..Default::default()
    };
    let mut snap = InspectionSnapshot::new();
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        cron_jobs: vec![CronJob {
            path: "/etc/cron.d/backup".into(),
            ..Default::default()
        }],
        systemd_timers: vec![
            timer("backup", "/etc/systemd/system/backup.timer"),
            timer("backup", "/usr/lib/systemd/system/backup.timer"),
            timer(
                "generated:cron-backup",
                "/etc/systemd/system/generated:cron-backup.timer",
            ),
        ],
        at_jobs: vec![AtJob {
            file: "a00001".into(),
            ..Default::default()
        }],
        generated_timer_units: vec![generated("/usr/bin/backup"), generated("/usr/bin/prune")],
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
    })
}

/// Each scheduled row's id is its kind and its source: the cron file, the
/// timer file's path, the at job's file, and the generated row's place in
/// the section. No two rows share an id, and only a generated row names
/// the generated unit it is, which is what a decline and the row's
/// control join on.
#[tokio::test]
async fn each_scheduled_row_has_its_own_id() {
    let rows = scheduled_rows(scheduled_identity_state()).await;
    let none = None::<String>;
    let generated = Some("cron-backup".to_owned());
    assert_eq!(
        rows,
        [
            ("cron:/etc/cron.d/backup".to_owned(), none.clone()),
            (
                "timer:/etc/systemd/system/backup.timer".to_owned(),
                none.clone()
            ),
            (
                "timer:/usr/lib/systemd/system/backup.timer".to_owned(),
                none.clone()
            ),
            (
                "timer:/etc/systemd/system/generated:cron-backup.timer".to_owned(),
                none.clone()
            ),
            ("at:a00001".to_owned(), none.clone()),
            ("generated:0:cron-backup".to_owned(), generated.clone()),
            ("generated:1:cron-backup".to_owned(), generated),
        ]
    );
    let mut ids: Vec<&String> = rows.iter().map(|(id, _)| id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), rows.len(), "{rows:?}");
}

/// A timer recorded without its file's path, as an imported snapshot may
/// be, is identified by its name.
#[tokio::test]
async fn a_scheduled_timer_without_a_path_is_identified_by_name() {
    let rows = scheduled_rows(scheduled_timer_state()).await;
    let ids: Vec<&str> = rows.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "cron:/etc/cron.d/backup",
            "timer:backup",
            "timer:cleanup",
            "generated:0:cron-backup"
        ]
    );
}

/// A timer template whose instances `a` and `b` are enabled by their links.
fn worker_instances_host() -> unit_hosts::UnitHost {
    let mut host = unit_hosts::UnitHost::new()
        .base_package("glibc")
        .unit("worker@.timer", "indirect")
        .unit("worker@.service", "static")
        .preset("disable *")
        .etc_unit(
            "worker@.timer",
            "[Unit]\nDescription=Worker %i\n[Timer]\nOnCalendar=daily\n\
             [Install]\nWantedBy=timers.target\n",
        )
        .etc_unit(
            "worker@.service",
            "[Service]\nType=oneshot\nExecStart=/usr/local/bin/worker %i\n",
        );
    for instance in ["a", "b"] {
        host = host.link(
            &format!("/etc/systemd/system/timers.target.wants/worker@{instance}.timer"),
            "/etc/systemd/system/worker@.timer",
        );
    }
    host
}

/// An instance timer is described from its template's file, so the
/// template's row and both instances' rows record one `path`. Through the
/// production sections handler each row still has its own id: the
/// template's row, which describes its own file, keeps `timer:<path>`, and
/// an instance's row is identified by that path and its own name.
#[tokio::test]
async fn a_timer_template_and_its_instances_have_their_own_ids() {
    const TEMPLATE_PATH: &str = "etc/systemd/system/worker@.timer";
    let snap = unit_hosts::scan(worker_instances_host());
    let described: Vec<(&str, &str)> = snap
        .scheduled_tasks
        .as_ref()
        .expect("a scheduled section")
        .systemd_timers
        .iter()
        .map(|t| (t.name.as_str(), t.path.as_str()))
        .collect();
    assert_eq!(
        described,
        [
            ("worker@", TEMPLATE_PATH),
            ("worker@a", TEMPLATE_PATH),
            ("worker@b", TEMPLATE_PATH),
        ],
        "presence: the collector records the template's path for each"
    );
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(unit_hosts::session(&snap))),
    });
    let ids: Vec<String> = scheduled_rows(state)
        .await
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        ids,
        [
            format!("timer:{TEMPLATE_PATH}"),
            format!(r#"timer-instance:["{TEMPLATE_PATH}","worker@a"]"#),
            format!(r#"timer-instance:["{TEMPLATE_PATH}","worker@b"]"#),
        ]
    );
}

/// A `SetInclude` on `item_id` through `/api/op`.
async fn set_include(
    app: &axum::Router,
    item_id: serde_json::Value,
    include: bool,
) -> (StatusCode, serde_json::Value) {
    post_json(
        app,
        "/api/op",
        serde_json::json!({
            "kind": "Op",
            "op": "SetInclude",
            "target": {"item_id": item_id, "include": include}
        }),
    )
    .await
}

/// D1 through the production handlers: a generated timer row is decided
/// on its own row. The view carries its state, excluding it takes its
/// enable out of the export preview, and including it again brings the
/// enable back. Excluding its cron job excludes it with the job, and the
/// view names that job; including the generated row is then refused.
#[tokio::test]
async fn a_generated_timer_row_is_decided_on_its_row() {
    let app = app(scheduled_timer_state());
    let generated = serde_json::json!({"kind": "GeneratedTimer", "key": {"name": "cron-backup"}});
    let enable = "'cron-backup.timer'";

    let (status, view) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        view["generated_timers"],
        serde_json::json!([{"name": "cron-backup", "include": true, "locked": false}])
    );
    let preview = view["containerfile_preview"].as_str().expect("a preview");
    assert!(preview.contains(enable), "{preview}");

    let (status, view) = set_include(&app, generated.clone(), false).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["generated_timers"][0]["include"], false, "{view}");
    let preview = view["containerfile_preview"].as_str().expect("a preview");
    assert!(!preview.contains(enable), "{preview}");

    let (status, view) = set_include(&app, generated.clone(), true).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["generated_timers"][0]["include"], true, "{view}");
    let preview = view["containerfile_preview"].as_str().expect("a preview");
    assert!(preview.contains(enable), "{preview}");

    let cron = serde_json::json!({"kind": "CronJob", "key": {"path": TIMER_CRON_PATH}});
    let (status, view) = set_include(&app, cron, false).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(
        view["generated_timers"],
        serde_json::json!([{
            "name": "cron-backup",
            "include": false,
            "locked": false,
            "cron_job_excluded": TIMER_CRON_PATH
        }])
    );
    let (status, json) = set_include(&app, generated, true).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
}

/// D4 through the production handlers: an included unmanaged file under
/// `/etc/systemd/system` carries the stager's `withheld` on its row, by the
/// row's own path as recorded. Controls: an unmanaged file elsewhere is
/// written and has none, and an excluded file at a unit path offers
/// nothing to withhold.
#[tokio::test]
async fn an_unmanaged_file_at_a_unit_path_is_withheld_on_its_row() {
    use inspectah_core::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};
    const UNIT_PATH: &str = "/etc/systemd/system/backup.service";
    // The same directory spelled with an interior `.`, as an imported
    // snapshot may record it: the row keeps that spelling.
    const DOTTED_UNIT_PATH: &str = "/etc/systemd/./system/report.service";
    const EXCLUDED_UNIT_PATH: &str = "/etc/systemd/system/other.service";
    const OPT_PATH: &str = "/opt/app/bin/run";
    let file = |path: &str, disposition: FindingKind| UnmanagedFile {
        path: path.into(),
        size: 10,
        disposition,
        ..Default::default()
    };
    let mut snap = InspectionSnapshot::new();
    snap.unmanaged_files = Some(UnmanagedFileSection {
        items: vec![
            file(UNIT_PATH, FindingKind::included()),
            file(DOTTED_UNIT_PATH, FindingKind::included()),
            file(EXCLUDED_UNIT_PATH, FindingKind::excluded()),
            file(OPT_PATH, FindingKind::included()),
        ],
        usr_entries: Vec::new(),
        total_size: 40,
        total_count: 4,
    });
    let app = app(Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
    }));
    let (status, view) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);
    let rows: Vec<&serde_json::Value> = view["unmanaged_files"]
        .as_array()
        .expect("unmanaged_files")
        .iter()
        .flat_map(|g| g["items"].as_array().expect("group items").iter())
        .collect();
    let row = |path: &str| {
        *rows
            .iter()
            .find(|r| r["path"] == path)
            .unwrap_or_else(|| panic!("no unmanaged row {path}: {view}"))
    };

    let withheld = &row(UNIT_PATH)["withheld"];
    assert_eq!(withheld["outcome"], "withheld", "{withheld}");
    assert_eq!(withheld["why"], "UnmanagedUnitPath", "{withheld}");
    assert!(
        withheld["text"].as_str().is_some_and(
            |t| t.contains("an unmanaged file is not written under /etc/systemd/system")
        ),
        "{withheld}"
    );
    assert_eq!(
        row(DOTTED_UNIT_PATH)["withheld"]["why"],
        "UnmanagedUnitPath",
        "{}",
        row(DOTTED_UNIT_PATH)
    );
    assert!(row(OPT_PATH).get("withheld").is_none(), "{}", row(OPT_PATH));
    assert!(
        row(EXCLUDED_UNIT_PATH).get("withheld").is_none(),
        "{}",
        row(EXCLUDED_UNIT_PATH)
    );
}

/// Two cron sources that generate one name, `/etc/cron.d/crontab` and
/// `/etc/crontab`, in the given order. Both generated records are
/// included.
fn two_source_generated_state(first: &str, second: &str) -> Arc<AppState> {
    use inspectah_core::types::scheduled::GeneratedTimerUnit;
    let generated = |source: &str| GeneratedTimerUnit {
        name: "cron-crontab".into(),
        cron_expr: "0 2 * * *".into(),
        source_path: source.trim_start_matches('/').into(),
        timer_content: "[Timer]\nOnCalendar=*-*-* 02:00:00\n[Install]\nWantedBy=timers.target\n"
            .into(),
        service_content: "[Service]\nType=oneshot\nExecStart=/usr/local/bin/job.sh\n".into(),
        disposition: FindingKind::included(),
        ..Default::default()
    };
    let cron = |path: &str| CronJob {
        path: path.into(),
        ..Default::default()
    };
    let mut snap = InspectionSnapshot::new();
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        cron_jobs: vec![cron(first), cron(second)],
        generated_timer_units: vec![generated(first), generated(second)],
        ..Default::default()
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
    })
}

/// Two cron sources generate `cron-crontab`; only `/etc/cron.d/crontab` is
/// excluded. The other record still exports the timer's files and enable,
/// so the row says included and offers exclude, whichever record comes
/// first. Excluding the name takes the enable out; including it again is
/// refused while the one source is excluded.
#[tokio::test]
async fn a_generated_name_with_one_included_source_shows_included() {
    const EXCLUDED: &str = "/etc/cron.d/crontab";
    const INCLUDED: &str = "/etc/crontab";
    let enable = "'cron-crontab.timer'";
    let generated = serde_json::json!({"kind": "GeneratedTimer", "key": {"name": "cron-crontab"}});
    for (first, second) in [(EXCLUDED, INCLUDED), (INCLUDED, EXCLUDED)] {
        let app = app(two_source_generated_state(first, second));
        let cron = serde_json::json!({"kind": "CronJob", "key": {"path": EXCLUDED}});
        let (status, view) = set_include(&app, cron, false).await;
        assert_eq!(status, StatusCode::OK, "{view}");

        let row = &view["generated_timers"];
        assert_eq!(row.as_array().map(Vec::len), Some(1), "{row}");
        assert_eq!(
            row[0]["include"], true,
            "order {first} then {second}: {row}"
        );
        assert_eq!(row[0]["cron_job_excluded"], EXCLUDED, "{row}");
        let preview = view["containerfile_preview"].as_str().expect("a preview");
        assert!(
            preview.contains(enable),
            "the export still enables it: {preview}"
        );
        assert!(
            preview.contains("COPY config/etc/systemd/system/ /etc/systemd/system/"),
            "the export still copies its files: {preview}"
        );

        let (status, view) = set_include(&app, generated.clone(), false).await;
        assert_eq!(status, StatusCode::OK, "{view}");
        assert_eq!(view["generated_timers"][0]["include"], false, "{view}");
        let preview = view["containerfile_preview"].as_str().expect("a preview");
        assert!(!preview.contains(enable), "{preview}");

        let (status, json) = set_include(&app, generated.clone(), true).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    }
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

/// Browsers send `Origin: null` from a sandboxed iframe, a `data:` URL or
/// after a cross-origin redirect. It is a present, foreign origin, not an
/// absent one, so it must not take the no-Origin path that `curl` uses.
#[tokio::test]
async fn null_origin_post_rejected() {
    let app = app(test_state());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/undo")
                .header("origin", "null")
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

    snap.units = Some(UnitSection {
        state_changes: vec![{
            let mut change = UnitStateChange::new(
                test_carried_unit("httpd.service".to_owned().try_into().unwrap(), None),
                inspectah_core::types::units::UnitState::Enabled,
                Some(inspectah_core::types::units::PresetDefault::Disable),
            )
            .unwrap();
            change.disposition = FindingKind::excluded();
            change
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
        inventory: vec![
            UnitInventoryEntry {
                unit: "sshd.service"
                    .to_owned()
                    .try_into()
                    .expect("valid unit name"),
                state: UnitState::Enabled,
                preset: None,
            },
            UnitInventoryEntry {
                unit: "cups.service"
                    .to_owned()
                    .try_into()
                    .expect("valid unit name"),
                state: UnitState::Disabled,
                preset: None,
            },
        ],
        mount_findings: Vec::new(),
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
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
        ..Default::default()
    });

    // The kernel module config is a `config.files` entry; the Kernel &
    // Boot section points at it.
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/modules-load.d/custom.conf".into(),
            kind: ConfigFileKind::Unowned,
            category: ConfigCategory::KernelModule,
            content: "br_netfilter".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
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
    assert!(ids.contains(&"units"));
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
async fn sections_empty_snapshot_returns_empty_items() {
    // A bare snapshot with no sections set — all 9 sections should exist
    // but with empty items vecs.
    let snap = InspectionSnapshot::new();
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
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
        serde_json::json!({"id": "units:httpd.service"}),
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
    assert!(id_strs.contains(&"units:httpd.service"));
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
        serde_json::json!({"id": "units:httpd.service"}),
    )
    .await;
    post_raw(
        &app,
        "/api/viewed",
        serde_json::json!({"id": "units:httpd.service"}),
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
    let sections = build_web_sections(&session.reference());
    let svc = sections.iter().find(|s| s.id == "units").unwrap();

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

    // enabled inventory entry with no preset
    let sshd = svc.items.iter().find(|i| i.id == "sshd.service").unwrap();
    assert_eq!(sshd.subtitle.as_deref(), Some("enabled (no preset rule)"));

    // disabled inventory entry with no preset
    let cups = svc.items.iter().find(|i| i.id == "cups.service").unwrap();
    assert_eq!(cups.subtitle.as_deref(), Some("disabled (no preset rule)"));
}

#[test]
fn normalize_containers_maps_all_types() {
    let snap = rich_snapshot();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(&session.reference());
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
    let sections = build_web_sections(&session.reference());
    assert!(
        sections.iter().all(|s| s.id != "users_groups"),
        "users_groups section should no longer appear in build_web_sections"
    );
}

#[test]
fn normalize_network_maps_connections_and_firewall() {
    let snap = rich_snapshot();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(&session.reference());
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
    let sections = build_web_sections(&session.reference());
    let stor = sections.iter().find(|s| s.id == "storage").unwrap();

    let boot = stor.items.iter().find(|i| i.id == "/boot").unwrap();
    assert!(boot.subtitle.as_ref().unwrap().contains("xfs"));
    assert_eq!(boot.detail.as_deref(), Some("defaults"));
}

#[test]
fn normalize_scheduled_tasks_maps_cron_and_timers() {
    let snap = rich_snapshot();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(&session.reference());
    let sched = sections.iter().find(|s| s.id == "scheduled_tasks").unwrap();

    assert_eq!(sched.items.len(), 2, "1 cron + 1 timer");

    let cron = sched
        .items
        .iter()
        .find(|i| i.id == "cron:/etc/cron.d/backup")
        .unwrap();
    assert_eq!(cron.title, "backup");

    let timer = sched
        .items
        .iter()
        .find(|i| i.id == "timer:logrotate.timer")
        .unwrap();
    assert_eq!(timer.subtitle.as_deref(), Some("daily"));
}

#[test]
fn normalize_selinux_includes_mode_and_modules() {
    let snap = rich_snapshot();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(&session.reference());
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
fn normalize_kernel_boot_maps_cmdline_and_config_pointers() {
    let snap = rich_snapshot();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(&session.reference());
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

    // A sysctl key is not a decision: no row stands for one.
    assert!(
        !customizations.items.iter().any(|i| i.id == "kernel.sysrq"),
        "no sysctl key row: {:?}",
        customizations.items
    );

    // The modules-load.d row points at the Configuration Files row that
    // decides the file. Its title is the whole path: two files named
    // `custom.conf` read alike as basenames.
    let modload = customizations
        .items
        .iter()
        .find(|i| i.id.contains("custom.conf"))
        .unwrap();
    assert_eq!(modload.title, "/etc/modules-load.d/custom.conf");
    assert_eq!(
        modload.subtitle.as_deref(),
        Some("decided under Configuration Files")
    );
    assert!(
        modload.detail.is_none(),
        "the content belongs to the row that can be decided"
    );
}

#[test]
fn normalize_non_rpm_maps_packages_and_version() {
    let snap = rich_snapshot();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(&session.reference());
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
    let sections = build_web_sections(&session.reference());
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
    let sections = build_web_sections(&session.reference());
    // Users & groups moved to ViewResponse, version_changes retired — 8 sections remain.
    assert_eq!(sections.len(), 8, "exactly 8 context sections");

    let expected_ids = [
        "units",
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
    let sections = build_web_sections(&session.reference());

    // Verify non-zero item counts for sections with data
    let svc = sections.iter().find(|s| s.id == "units").unwrap();
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
    snap.units = Some(UnitSection {
        state_changes: vec![
            UnitStateChange::new(
                test_carried_unit(
                    "custom-app.service".to_owned().try_into().unwrap(),
                    Some("custom-app".into()),
                ),
                UnitState::Enabled,
                Some(PresetDefault::Disable),
            )
            .unwrap(),
            UnitStateChange::new(
                test_carried_unit(
                    "sssd-kcm.service".to_owned().try_into().unwrap(),
                    Some("sssd".into()),
                ),
                UnitState::Disabled,
                Some(PresetDefault::Enable),
            )
            .unwrap(),
        ],
        inventory: vec![],
        drop_ins: vec![],
        mount_findings: Vec::new(),
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
    });
    snap.warnings.push(Warning {
        inspector: "units".into(),
        message: "unit linked.service has state 'linked'".into(),
        severity: Some(WarningSeverity::Warning),
        extra: std::collections::HashMap::from([
            ("unit".into(), serde_json::json!("linked.service")),
            ("raw_state".into(), serde_json::json!("linked")),
        ]),
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
    })
}

#[tokio::test]
async fn sections_include_service_subsections() {
    let app = app(service_subsection_state());
    let (status, json) = get_json(&app, "/api/snapshot/sections").await;
    assert_eq!(status, StatusCode::OK);

    let sections = json.as_array().unwrap();
    let svc = sections.iter().find(|s| s["id"] == "units").unwrap();

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
    snap.units = Some(UnitSection {
        state_changes: vec![],
        inventory: vec![],
        drop_ins: vec![],
        mount_findings: Vec::new(),
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
    });
    snap.config = Some(ConfigSection { files: vec![] });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
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
        Some(
            r#"[{"kind":"failure","text":"epel-release.noarch: staged RPM not verified, not exported"}]"#
        )
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
///
/// `extras_rpm` is where `epel-release-extras` keeps its RPM, and
/// `extras_uploaded` says whether that is a staged upload rather than the
/// host's dnf cache. An uploaded package's export name is its staged
/// basename, so it no longer owns its NEVRA and the collision is gone.
fn colliding_nevra_state(
    tmp: &tempfile::TempDir,
    extras_rpm: &std::path::Path,
    extras_uploaded: bool,
) -> Arc<AppState> {
    let snap: InspectionSnapshot = serde_json::from_value(serde_json::json!({
        "schema_version": inspectah_core::snapshot::SCHEMA_VERSION,
        "rpm": { "packages_added": [
            {
                "name": "epel-release-extras",
                "version": "9",
                "release": "11.el9",
                "arch": "noarch",
                "repoless_cached": true,
                "uploaded": extras_uploaded,
                "cache_path": extras_rpm.to_string_lossy(),
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
    let app = app(colliding_nevra_state(&tmp, &dnf_cache, false));
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
async fn a_nevra_whose_owner_is_itself_an_upload_is_not_a_collision() {
    // The reaching input for `CachedNevra`'s `!pkg.uploaded`. Once
    // `epel-release-extras` is an upload, the export writes it under its
    // staged basename and stops writing its NEVRA, so the name is free.
    // Without the condition this upload is refused for a collision that
    // no longer exists.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(UPLOAD_SUBDIR);
    std::fs::create_dir(&dir).unwrap();
    let staged = dir.join("epel-release-extras-latest-9.noarch.rpm");
    std::fs::write(&staged, b"the extras upload").unwrap();
    let app = app(colliding_nevra_state(&tmp, &staged, true));

    let (status, body) = post_upload(
        &app,
        &[file_part(
            "epel-release-extras-9-11.el9.noarch.rpm",
            b"an rpm",
        )],
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["matched"], "epel-release.noarch");
    assert_eq!(
        dir_entries(&dir),
        vec![
            "epel-release-extras-9-11.el9.noarch.rpm".to_string(),
            "epel-release-extras-latest-9.noarch.rpm".to_string(),
        ],
        "and the extras package's own upload is still there"
    );
}

/// An upload directory the session reaches by a different spelling from
/// the canonical one: `<tmp>/link` is a symlink to `<tmp>/real`.
///
/// `prepare_upload_dir` refuses a symlink at the final component, so the
/// link is one level up -- which is where a real one would be, in a home
/// directory or a `/var` reached through one. Every `cache_path` the
/// handler records goes in under the `link` spelling while every
/// confinement check resolves to the `real` one.
fn symlinked_upload_dir(tmp: &tempfile::TempDir) -> std::path::PathBuf {
    let real = tmp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, tmp.path().join("link")).unwrap();
    let given = tmp.path().join("link").join(UPLOAD_SUBDIR);
    std::fs::create_dir(&given).unwrap();
    assert_ne!(
        given,
        real.canonicalize().unwrap().join(UPLOAD_SUBDIR),
        "the fixture is only a fixture if the two spellings differ"
    );
    given
}

/// A session over `upload_dir` holding `epel-release` and `custom-tool`,
/// both repo-less.
fn state_over_upload_dir(upload_dir: &std::path::Path) -> Arc<AppState> {
    let (state, _tmp) = upload_state();
    state
        .session
        .lock()
        .unwrap()
        .set_upload_dir(upload_dir.to_path_buf());
    state
}

#[tokio::test]
async fn upload_refuses_a_contested_name_through_a_symlinked_directory() {
    // The 409's `Staged` arm, with the two spellings that defeat a text
    // comparison. `custom-tool` holds the file under the `link`
    // spelling; the destination this upload builds is the same spelling,
    // but nothing in the code may rely on that -- a snapshot carries
    // whichever spelling the session that wrote it was given.
    let tmp = tempfile::tempdir().unwrap();
    let dir = symlinked_upload_dir(&tmp);
    let contested = dir.join("epel-release-9-12.el9.noarch.rpm");
    std::fs::write(&contested, b"custom-tool's file").unwrap();
    let state = state_over_upload_dir(&dir);
    {
        let mut session = state.session.lock().unwrap();
        let canonical = contested.canonicalize().unwrap();
        session.set_uploaded("custom-tool.x86_64", canonical, Some("9-12.el9".into()));
    }
    let app = app(state);

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
        b"custom-tool's file".as_slice(),
        "and the file it names is untouched"
    );
}

#[tokio::test]
async fn replacing_an_upload_recorded_under_another_spelling_keeps_the_file() {
    // The worse half of the same mistake. `superseded_upload` compares
    // `cache_path` to the destination; `resolve_staged_file`, which runs
    // next, resolves. When the comparison said "different file" and the
    // resolution said "this one", the handler staged the upload and then
    // deleted it, answering 200 with `uploaded: true` and nothing on
    // disk.
    let tmp = tempfile::tempdir().unwrap();
    let dir = symlinked_upload_dir(&tmp);
    const FILENAME: &str = "epel-release-9-12.el9.noarch.rpm";
    let staged = dir.join(FILENAME);
    std::fs::write(&staged, b"the first upload").unwrap();
    let state = state_over_upload_dir(&dir);
    {
        let mut session = state.session.lock().unwrap();
        let canonical = staged.canonicalize().unwrap();
        session.set_uploaded("epel-release.noarch", canonical, Some("9-12.el9".into()));
    }
    let app = app(state);

    let (status, body) = post_upload(&app, &[file_part(FILENAME, b"the replacement")]).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["matched"], "epel-release.noarch");
    assert_eq!(
        std::fs::read(&staged).expect("the file the response says is staged exists"),
        b"the replacement".as_slice()
    );
    assert_eq!(dir_entries(&dir), vec![FILENAME.to_string()]);
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

#[tokio::test]
async fn deleting_an_upload_recorded_outside_the_upload_directory_names_only_the_basename() {
    // A scanned host can set `uploaded: true` with a `cache_path` outside
    // the upload directory. The removal refuses it, and the operator is
    // told which file was refused -- the basename, which is the name they
    // chose -- and nothing about where the upload directory lives.
    let tmp = tempfile::tempdir().unwrap();
    let elsewhere = tmp.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    let outside = elsewhere.join("epel-release-9-12.el9.noarch.rpm");
    std::fs::write(&outside, SNAPSHOT_SUPPLIED_SECRET).unwrap();
    let state = upload_state_from_scan(
        &tmp,
        serde_json::json!({
            "name": "epel-release",
            "version": "9",
            "release": "11.el9",
            "arch": "noarch",
            "repoless_annotation": "No repo source",
            "repoless_cached": true,
            "uploaded": true,
            "cache_path": outside.to_string_lossy(),
        }),
    );
    state.session.lock().unwrap().ensure_upload_dir().unwrap();
    let app = app(state);

    let response = app
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
    let raw = String::from_utf8(bytes.to_vec()).unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["error"],
        "refusing to remove epel-release-9-12.el9.noarch.rpm: \
         the snapshot records it outside this session's upload directory"
    );
    let tmp_path = tmp.path().to_string_lossy();
    assert!(
        !raw.contains(tmp_path.as_ref()),
        "the response names no host directory: {raw}"
    );
    assert!(
        !raw.contains('/'),
        "the response carries no path separator at all: {raw}"
    );
    assert!(outside.exists(), "and nothing is deleted");
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

// -- System Tuning section --------------------------------------------------

#[tokio::test]
async fn groups_list_system_tuning_in_system_config_after_kernel_boot() {
    let app = app(test_state());
    let (status, groups) = get_json(&app, "/api/groups").await;
    assert_eq!(status, StatusCode::OK);

    let system_config = groups
        .as_array()
        .expect("groups array")
        .iter()
        .find(|g| g["slug"] == "system-config")
        .expect("the System Configuration group");
    let ids: Vec<&str> = system_config["sections"]
        .as_array()
        .expect("sections array")
        .iter()
        .map(|s| s["id"].as_str().expect("id"))
        .collect();
    let kernel = ids
        .iter()
        .position(|id| *id == "kernel_boot")
        .expect("kernel_boot is listed");
    assert_eq!(ids.get(kernel + 1), Some(&"system_tuning"), "{ids:?}");

    let tuning = &system_config["sections"][kernel + 1];
    assert_eq!(tuning["label"], "System Tuning");
    assert_eq!(tuning["is_triage"], true);
}

const TUNED_PROFILE: &str = "site-throughput";

/// A host with one config file and an included custom tuned profile.
fn tuned_state() -> Arc<AppState> {
    let state = test_state();
    let mut snap = state.session.lock().unwrap().snapshot().clone();
    snap.kernel_boot = Some(KernelBootSection {
        tuned_active: TUNED_PROFILE.into(),
        tuned_disposition: FindingKind::included(),
        ..Default::default()
    });
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
    })
}

/// The item of every op the session's timeline holds, in order.
fn timeline_items(state: &AppState) -> Vec<inspectah_refine::types::ItemId> {
    use inspectah_refine::types::{RefinementOp, TimelineEntry};
    state
        .session
        .lock()
        .unwrap()
        .timeline_history()
        .into_iter()
        .map(|annotated| match annotated.entry {
            TimelineEntry::Op(RefinementOp::SetInclude { item_id, .. }) => item_id,
            other => panic!("a batch toggle records only SetInclude ops, got {other:?}"),
        })
        .collect()
}

fn tuned_item() -> inspectah_refine::types::ItemId {
    inspectah_refine::types::ItemId::TunedSelection {
        profile: TUNED_PROFILE.into(),
    }
}

#[tokio::test]
async fn batch_toggle_system_tuning_produces_exactly_the_tuned_op() {
    let state = tuned_state();
    let (status, body) = post_json(
        &app(state.clone()),
        "/api/batch-toggle/system_tuning",
        serde_json::json!({"include": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(timeline_items(&state), [tuned_item()]);
}

#[tokio::test]
async fn batch_toggle_kernel_boot_no_longer_reaches_the_tuned_row() {
    let state = tuned_state();
    let (status, body) = post_json(
        &app(state.clone()),
        "/api/batch-toggle/kernel_boot",
        serde_json::json!({"include": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !timeline_items(&state).contains(&tuned_item()),
        "the tuned row belongs to system_tuning"
    );
}

#[tokio::test]
async fn batch_toggle_system_config_group_still_reaches_the_tuned_row() {
    let state = tuned_state();
    let (status, body) = post_json(
        &app(state.clone()),
        "/api/batch-toggle/system-config",
        serde_json::json!({"include": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = timeline_items(&state);
    assert!(items.contains(&tuned_item()), "{items:?}");
    assert!(
        items.contains(&inspectah_refine::types::ItemId::Config {
            path: "/etc/httpd/conf/httpd.conf".into()
        }),
        "the group toggle reaches the other sections too: {items:?}"
    );
}

/// A carried unit from the vendor directory, owned as given.
fn test_carried_unit(
    name: inspectah_core::types::units::UnitName,
    owning_package: Option<String>,
) -> inspectah_core::types::units::CarriedUnit {
    use inspectah_core::types::units::{CarriedUnit, UnitFragment, UnitSubject};
    let fragment = UnitFragment::Vendor {
        file: name.clone(),
        owning_package,
    };
    CarriedUnit::try_from(UnitSubject::new(name, fragment).expect("file is the unit"))
        .expect("a test unit is carried")
}

// ---------------------------------------------------------------------
// Every unit kind and its reasons on `/api/view` (§ 11.1).
// ---------------------------------------------------------------------

/// A token the secret pattern matches, planted in a local mount unit's
/// `What=` (T10).
const MOUNT_TOKEN: &str = "t10-mount-source-7f3a";
const MOUNT_UNIT: &str = "srv-data.mount";
const MOUNT_UNIT_PATH: &str = "/etc/systemd/system/srv-data.mount";
/// A protected unit masked on the host: the plan declines its mask.
const PROTECTED_UNIT: &str = "ostree-remount.service";
/// T15's pair of imported config entries, a file and a file under it.
const CONFLICT_FILE: &str = "/etc/app/conf";
const CONFLICT_CHILD: &str = "/etc/app/conf/x";
/// An imported zero-length unit file, its path spelled with an interior
/// `.`: the stager withholds it, and its row is found by that spelling.
const ZERO_LENGTH_UNIT: &str = "/etc/systemd/system/./zero.service";
/// An imported config entry the export writes: the control.
const WRITTEN_CONFIG: &str = "/etc/app.d/written.conf";

/// A host, through the scan against a base image (so refine keeps the
/// socket's owner included), with a socket enabled against its preset, a
/// masked protected unit and an administrator's mount unit whose `What=`
/// holds a credential; then the config entries above added as an imported
/// snapshot would carry them, since the collector cannot produce them.
fn every_kind_state() -> Arc<AppState> {
    let service = "[Service]\nExecStart=/usr/bin/true\n[Install]\nWantedBy=multi-user.target\n";
    let mut snap = unit_hosts::scan(
        unit_hosts::UnitHost::new()
            .base_package("glibc")
            .package("cockpit-ws")
            .unit("cockpit.socket", "enabled")
            .unit(PROTECTED_UNIT, "masked")
            .unit(MOUNT_UNIT, "enabled")
            .preset("disable *")
            .vendor_unit(
                "cockpit.socket",
                "[Socket]\nListenStream=9090\n[Install]\nWantedBy=sockets.target\n",
                Some("cockpit-ws"),
            )
            .vendor_unit(PROTECTED_UNIT, service, Some("ostree"))
            .mask(PROTECTED_UNIT)
            .etc_unit(
                MOUNT_UNIT,
                &format!(
                    "[Unit]\nDescription=Data volume\n\n[Mount]\n\
                     What=https://files.example/volume?token={MOUNT_TOKEN}\n\
                     Where=/srv/data\nOptions=defaults,nofail\n\n\
                     [Install]\nWantedBy=multi-user.target\n"
                ),
            ),
    );
    let files = &mut snap.config.as_mut().expect("a config section").files;
    for (path, content) in [
        (CONFLICT_FILE, "a=1\n"),
        (CONFLICT_CHILD, "b=2\n"),
        (ZERO_LENGTH_UNIT, ""),
        (WRITTEN_CONFIG, "c=3\n"),
    ] {
        files.push(ConfigFileEntry {
            path: path.into(),
            category: ConfigCategory::Other,
            content: content.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        });
    }
    Arc::new(AppState {
        session: Arc::new(Mutex::new(unit_hosts::session(&snap))),
    })
}

/// The row of `rows` whose `key` is `value`.
fn row_by<'a>(rows: &'a serde_json::Value, key: &str, value: &str) -> &'a serde_json::Value {
    rows.as_array()
        .expect("an array")
        .iter()
        .find(|r| r[key] == value)
        .unwrap_or_else(|| panic!("no row with {key} {value}: {rows}"))
}

/// § 11.1 through the production handlers: a socket row has its kind and
/// no `declined` until its owner is excluded, then the plan's reason with
/// the Containerfile's own text and no `attention_reason`; a protected
/// unit's row has its FIXME,
/// the config rows that offered a withheld file say so (and a written one
/// does not), and the mount finding is a row with its advisories and no
/// `include` key at all.
#[tokio::test]
async fn the_view_serves_unit_kinds_reasons_and_mount_findings() {
    let app = app(every_kind_state());
    let (status, json) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);
    let preview = json["containerfile_preview"].as_str().expect("a preview");

    let socket = row_by(&json["unit_states"], "unit", "cockpit.socket");
    assert_eq!(socket["kind"], "socket", "{socket}");
    assert_eq!(socket["fragment"], "vendor", "{socket}");
    assert!(socket.get("declined").is_none(), "{socket}");

    let protected = row_by(&json["unit_states"], "unit", PROTECTED_UNIT);
    assert_eq!(protected["kind"], "service");
    assert_eq!(protected["declined"]["renders"], "fixme", "{protected}");
    assert_eq!(protected["declined"]["reason"], "TargetNeedsUnit");
    let text = protected["declined"]["text"].as_str().expect("a text");
    assert!(
        preview.contains(&format!("# FIXME: {text}")),
        "the row's text is the Containerfile's line: {text}\n{preview}"
    );

    // Excluding the socket's owner declines its enable, on a row whose
    // `attention_reason` is null: `declined` stands on its own.
    set_package_include(&app, "cockpit-ws", "x86_64", false).await;
    let (_, after) = get_json(&app, "/api/view").await;
    let socket = row_by(&after["unit_states"], "unit", "cockpit.socket");
    assert!(socket.get("attention_reason").is_none(), "{socket}");
    assert_eq!(
        socket["declined"],
        serde_json::json!({
            "renders": "omitted",
            "reason": "PackageExcluded",
            "text": "cockpit.socket (package 'cockpit-ws' was excluded)",
        })
    );
    let after_preview = after["containerfile_preview"].as_str().expect("a preview");
    assert!(
        after_preview.contains("# Omitted: cockpit.socket (package 'cockpit-ws' was excluded)"),
        "{after_preview}"
    );

    let config = |path: &str| -> serde_json::Value {
        json["config_files"]
            .as_array()
            .expect("config_files")
            .iter()
            .find(|c| c["entry"]["path"] == path)
            .unwrap_or_else(|| panic!("no config row {path}"))
            .clone()
    };
    let conflict = |other: &str| {
        serde_json::json!({
            "outcome": "withheld",
            "why": "Conflict",
            "text": format!(
                "{other} is also staged, and one of the two paths is a directory of the \
                 other; exclude one of the two"
            ),
        })
    };
    assert_eq!(config(CONFLICT_FILE)["withheld"], conflict(CONFLICT_CHILD));
    assert_eq!(config(CONFLICT_CHILD)["withheld"], conflict(CONFLICT_FILE));
    assert_eq!(
        config(ZERO_LENGTH_UNIT)["withheld"]["why"],
        "ZeroLengthUnitFile",
        "the row is found by its own spelling"
    );
    // Control: a config row the export writes has no `withheld` key.
    assert!(config(WRITTEN_CONFIG).get("withheld").is_none());
    assert!(config(WRITTEN_CONFIG).get("entry").is_some());

    let finding = row_by(&json["mount_findings"], "unit", MOUNT_UNIT);
    assert_eq!(finding["kind"], "mount", "{finding}");
    assert_eq!(finding["state"], "enabled");
    assert_eq!(finding["mount_point"], "/srv/data");
    assert_eq!(finding["fragment"], "local");
    assert_eq!(finding["unit_file_path"], MOUNT_UNIT_PATH);
    let advisories = finding["advisories"].as_array().expect("advisories");
    assert!(!advisories.is_empty(), "{finding}");
    for line in advisories {
        let line = line.as_str().expect("a string");
        assert!(preview.contains(line), "{line}\n{preview}");
    }
    // Presence of a toggle key on the unit rows first, then its absence.
    assert!(socket.get("include").is_some());
    for row in json["mount_findings"].as_array().expect("an array") {
        assert!(row.get("include").is_none(), "{row}");
    }
}

/// T10 on the wire: the finding row exists with its mount point and
/// advisories, and the planted mount source is nowhere in `/api/view`.
#[tokio::test]
async fn the_mount_source_is_nowhere_in_the_view_response() {
    let app = app(every_kind_state());
    let (status, json) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);
    let finding = row_by(&json["mount_findings"], "unit", MOUNT_UNIT);
    assert_eq!(finding["mount_point"], "/srv/data");
    assert!(
        !finding["advisories"]
            .as_array()
            .expect("advisories")
            .is_empty()
    );
    let body = json.to_string();
    assert!(body.contains(MOUNT_UNIT_PATH), "presence first");
    assert!(!body.contains(MOUNT_TOKEN), "the mount source leaked");
}

/// T10 for a `What=` continued onto the next line: systemd reads the
/// `Where=/opt/token=...` line after it as part of the mount source. The
/// finding row has the mount point systemd uses, and the token is nowhere
/// in the `/api/view` body as the handler sends it.
#[tokio::test]
async fn a_continued_mount_source_is_nowhere_in_the_view_response() {
    use unit_hosts::{CONTINUED_MOUNT_POINT, CONTINUED_MOUNT_TOKEN, CONTINUED_MOUNT_UNIT};
    // Presence first: the host's unit file holds the token before redaction.
    let raw = unit_hosts::scan_unredacted(unit_hosts::continued_mount_host());
    assert!(
        serde_json::to_string(&raw)
            .expect("the snapshot serializes")
            .contains(CONTINUED_MOUNT_TOKEN),
        "the fixture plants the token"
    );
    let snap = unit_hosts::scan(unit_hosts::continued_mount_host());
    let app = app(Arc::new(AppState {
        session: Arc::new(Mutex::new(unit_hosts::session(&snap))),
    }));

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/view")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(bytes.to_vec()).expect("the view is UTF-8");
    let json: serde_json::Value = serde_json::from_str(&body).expect("the view is JSON");

    let finding = row_by(&json["mount_findings"], "unit", CONTINUED_MOUNT_UNIT);
    assert_eq!(finding["mount_point"], CONTINUED_MOUNT_POINT, "{finding}");
    assert!(body.contains(CONTINUED_MOUNT_UNIT), "presence first");
    assert!(
        !body.contains(CONTINUED_MOUNT_TOKEN),
        "the continued mount source leaked"
    );
}

/// Two aggregate variants at one unit path, the selected one with content
/// and the alternative zero-length. The projection's `config_withheld`
/// lists one entry per included row at the path; the view puts each on
/// an included row at that path and never on an excluded one. Excluding
/// the path (which reaches the first variant) leaves the zero-length
/// alternative as the one included row, and only it carries `withheld`.
#[test]
fn withheld_lands_on_the_included_variant_rows_only() {
    use inspectah_core::types::aggregate::VariantSelection;
    use inspectah_refine::types::{ItemId, RefinementOp};
    const PATH: &str = "/etc/systemd/system/variant.service";
    let variant = |content: &str, selection| ConfigFileEntry {
        path: PATH.into(),
        kind: ConfigFileKind::Unowned,
        category: ConfigCategory::Other,
        content: content.into(),
        disposition: FindingKind::included(),
        variant_selection: selection,
        ..Default::default()
    };
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![
            variant("[Unit]\n", VariantSelection::Selected),
            variant("", VariantSelection::Alternative),
        ],
    });
    let mut session = RefineSession::new(snap);
    // (content, included, withheld) per config row, in view order.
    let rows = |session: &RefineSession| -> Vec<(String, bool, Option<serde_json::Value>)> {
        let json = serde_json::to_value(inspectah_web::adapter::build_web_view(session))
            .expect("the view serializes");
        json["config_files"]
            .as_array()
            .expect("config_files")
            .iter()
            .map(|c| {
                (
                    c["entry"]["content"].as_str().expect("content").to_owned(),
                    c["entry"]["disposition"]["include"] == true,
                    c.get("withheld").cloned(),
                )
            })
            .collect()
    };

    let both = rows(&session);
    assert!(both.iter().all(|(_, included, _)| *included), "{both:?}");
    let listed: Vec<serde_json::Value> = session
        .decisions()
        .config_withheld
        .iter()
        .filter(|c| c.path == PATH)
        .map(|c| serde_json::to_value(&c.withheld).expect("serializes"))
        .collect();
    let carried: Vec<serde_json::Value> = both.iter().filter_map(|(_, _, w)| w.clone()).collect();
    assert_eq!(listed.len(), 2, "both included variants offer the file");
    assert_eq!(carried, listed);

    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Config { path: PATH.into() },
            include: false,
        })
        .expect("the config row toggles");
    let one = rows(&session);
    assert_eq!(
        one.iter()
            .map(|(content, included, w)| (content.as_str(), *included, w.is_some()))
            .collect::<Vec<_>>(),
        [("[Unit]\n", false, false), ("", true, true)],
        "{one:?}"
    );
    assert_eq!(
        one[1].2.as_ref().expect("withheld")["why"],
        "ZeroLengthUnitFile"
    );
}

/// Plan review focus 1 at the web response: an escaped instance name, the
/// root slice `-.slice` and a 255-byte name round-trip through the
/// production `/api/view` handler byte for byte, and a `SetInclude` naming
/// each through `/api/op` is accepted and moves that row alone.
///
/// Built by hand, with the instance a vendor unit whose file is its
/// template, which the state change type admits for an enabled instance.
#[tokio::test]
async fn unusual_unit_names_round_trip_through_the_view_and_an_op() {
    use inspectah_core::types::units::{CarriedUnit, UnitFragment, UnitName, UnitSubject};
    let long = format!("{}.service", "a".repeat(255 - ".service".len()));
    assert_eq!(long.len(), 255);
    let names = [r"probe@a\x2db.service", "-.slice", long.as_str()];
    let name = |s: &str| UnitName::try_from(s.to_owned()).expect("a unit name");
    let change = |unit: &str, file: &str| {
        let subject = UnitSubject::new(
            name(unit),
            UnitFragment::Vendor {
                file: name(file),
                owning_package: None,
            },
        )
        .expect("a subject");
        UnitStateChange::new(
            CarriedUnit::try_from(subject).expect("a carried unit"),
            UnitState::Enabled,
            None,
        )
        .expect("a state change")
    };
    let mut snap = InspectionSnapshot::new();
    snap.units = Some(UnitSection {
        state_changes: vec![
            change(names[0], "probe@.service"),
            change(names[1], names[1]),
            change(names[2], names[2]),
        ],
        ..Default::default()
    });
    let app = app(Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
    }));

    let row = |json: &serde_json::Value, unit: &str| -> serde_json::Value {
        json["unit_states"]
            .as_array()
            .expect("the view carries unit_states")
            .iter()
            .find(|s| s["unit"].as_str() == Some(unit))
            .cloned()
            .unwrap_or_else(|| panic!("no unit_states row named {unit:?}: {json}"))
    };
    let (status, json) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);
    for unit in names {
        assert_eq!(row(&json, unit)["unit"], unit, "byte for byte");
        assert_eq!(row(&json, unit)["include"], true, "{unit}");
    }

    for (generation, unit) in (1..).zip(names) {
        let (status, response) = post_json(
            &app,
            "/api/op",
            serde_json::json!({
                "kind": "Op",
                "op": "SetInclude",
                "target": {
                    "item_id": {"kind": "Unit", "key": {"unit": unit}},
                    "include": false
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{unit}: {response}");
        assert_eq!(response["generation"], generation, "{unit}");
        let (_, json) = get_json(&app, "/api/view").await;
        assert_eq!(row(&json, unit)["include"], false, "{unit} is excluded");
        for later in names.iter().skip(generation) {
            assert_eq!(row(&json, later)["include"], true, "{later} is untouched");
        }
    }
}

// ---------------------------------------------------------------------
// Links, `UndeterminedInstall` records, `Also=` members and the default
// target on `/api/view` (§ 11.1, Task 4.11), from scanned hosts.
// ---------------------------------------------------------------------

/// The package that owns the vendor units of the link hosts.
const LINK_PACKAGE: &str = "apppkg";
/// A service with no `[Install]` section.
const NO_INSTALL_SERVICE: &str = "[Service]\nExecStart=/bin/true\n";
/// A service wanted by `multi-user.target`.
const WANTED_SERVICE: &str =
    "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\n";

/// An app over a refine session opened from `host`'s scan.
fn scanned_app(host: unit_hosts::UnitHost) -> axum::Router {
    let snap = unit_hosts::scan(host);
    app(Arc::new(AppState {
        session: Arc::new(Mutex::new(unit_hosts::session(&snap))),
    }))
}

/// A host with `LINK_PACKAGE` installed over a base image holding glibc,
/// so refine keeps the package included.
fn link_host() -> unit_hosts::UnitHost {
    unit_hosts::UnitHost::new()
        .base_package("glibc")
        .package(LINK_PACKAGE)
}

/// `host` with a link to `unit` in `<parent>.wants` under `/etc`.
fn with_wants_link(host: unit_hosts::UnitHost, parent: &str, unit: &str) -> unit_hosts::UnitHost {
    host.link(
        &format!("/etc/systemd/system/{parent}.wants/{unit}"),
        &format!("/usr/lib/systemd/system/{unit}"),
    )
}

/// The view's `containerfile_preview`.
fn preview_of(json: &serde_json::Value) -> &str {
    json["containerfile_preview"].as_str().expect("a preview")
}

/// The one row of `rows` matching every `(key, value)` of `by`.
fn row_where<'a>(rows: &'a serde_json::Value, by: &[(&str, &str)]) -> &'a serde_json::Value {
    let found: Vec<_> = rows
        .as_array()
        .expect("an array")
        .iter()
        .filter(|r| by.iter().all(|(k, v)| r[*k] == *v))
        .collect();
    assert_eq!(found.len(), 1, "one row with {by:?}: {rows}");
    found[0]
}

/// Each of `row`'s `advisories`, which must be lines of `preview`.
fn advisories_in_preview(row: &serde_json::Value, preview: &str) -> Vec<String> {
    let lines: Vec<String> = row["advisories"]
        .as_array()
        .expect("advisories")
        .iter()
        .map(|l| l.as_str().expect("a string").to_owned())
        .collect();
    assert!(!lines.is_empty(), "{row}");
    for line in &lines {
        assert!(
            preview.lines().any(|l| l == line),
            "{line}\nis not a line of\n{preview}"
        );
    }
    lines
}

/// A `.wants` link to a unit with no `[Install]` is a `manual_links[]`
/// row with its relation, parent, the bare-string cause, the
/// Containerfile's lines and no `include` key, while the unit row beside
/// it has one. A link of an `UpheldBy=` unit carries the object cause
/// with its reason.
#[tokio::test]
async fn the_view_serves_hand_made_links_with_no_toggle() {
    let host = with_wants_link(
        link_host()
            .unit("backup.service", "static")
            .vendor_unit("backup.service", NO_INSTALL_SERVICE, None)
            .unit("app.service", "enabled")
            .preset("disable *")
            .vendor_unit("app.service", WANTED_SERVICE, Some(LINK_PACKAGE)),
        "multi-user.target",
        "backup.service",
    );
    let app = scanned_app(host);
    let (status, json) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);
    let link = row_where(&json["manual_links"], &[("unit", "backup.service")]);
    assert_eq!(link["relation"], "wants", "{link}");
    assert_eq!(link["parent"], "multi-user.target", "{link}");
    assert_eq!(link["cause"], "static_unit", "{link}");
    let lines = advisories_in_preview(link, preview_of(&json));
    assert!(
        lines.iter().any(|l| l.contains("add-wants")),
        "the command to carry it: {lines:?}"
    );
    assert!(link.get("not_applied").is_none(), "{link}");
    // Presence of a toggle key on the unit row first, then its absence.
    assert!(
        row_by(&json["unit_states"], "unit", "app.service")
            .get("include")
            .is_some()
    );
    assert!(link.get("include").is_none(), "{link}");

    let upheld = "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\n\
                  UpheldBy=multi-user.target\n";
    let host = with_wants_link(
        unit_hosts::UnitHost::new()
            .base_package("needpkg")
            .unit("need.service", "disabled")
            .preset("enable need.service")
            .vendor_unit("need.service", upheld, Some("needpkg")),
        "multi-user.target",
        "need.service",
    );
    let (_, json) = get_json(&scanned_app(host), "/api/view").await;
    let link = row_where(&json["manual_links"], &[("unit", "need.service")]);
    assert_eq!(
        link["cause"],
        serde_json::json!({"undetermined": {"reason": "upheld_by"}}),
        "{link}"
    );
    advisories_in_preview(link, preview_of(&json));
}

/// The install-difference rows of Task 4.5b: `also` with
/// `vendor_install_only`, and a `.wants` link each way (T41's host). With
/// one install file excluded through `/api/op`, both link rows hold its
/// path in `not_applied` and the unit's `# Omitted:` line, which is in the
/// preview once; included again, `not_applied` goes and the blocks return.
#[tokio::test]
async fn the_view_serves_install_difference_links_and_their_omission() {
    let vendor = "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\n\
                  Also=helper.service\n";
    let host = with_wants_link(
        link_host()
            .unit("app.service", "enabled")
            .preset("enable app.service")
            .vendor_unit("app.service", vendor, Some(LINK_PACKAGE)),
        "multi-user.target",
        "app.service",
    )
    .etc_unit("app.service", WANTED_SERVICE);
    let (_, json) = get_json(&scanned_app(host), "/api/view").await;
    let also = row_where(&json["manual_links"], &[("relation", "also")]);
    assert_eq!(also["parent"], "app.service", "{also}");
    assert_eq!(also["unit"], "helper.service", "{also}");
    assert_eq!(also["cause"], "vendor_install_only", "{also}");
    assert_eq!(
        advisories_in_preview(also, preview_of(&json)),
        [
            "# Advisory: app.service's package unit names helper.service in Also=, and its \
             files under /etc/systemd/system do not. The package install in the image acted \
             on the package's list.",
            "#   Check helper.service with systemctl is-enabled in the image.",
        ]
    );

    let reset = "/etc/systemd/system/app.service.d/10-reset.conf";
    let add = "/etc/systemd/system/app.service.d/20-add.conf";
    let host = with_wants_link(
        link_host()
            .unit("app.service", "enabled")
            .preset("enable app.service")
            .vendor_unit("app.service", WANTED_SERVICE, Some(LINK_PACKAGE)),
        "graphical.target",
        "app.service",
    )
    .file(reset, "[Install]\nWantedBy=\n")
    .file(add, "[Install]\nWantedBy=graphical.target\n");
    let app = scanned_app(host);
    let (_, json) = get_json(&app, "/api/view").await;
    let preview = preview_of(&json).to_owned();
    let host_only = row_where(
        &json["manual_links"],
        &[
            ("parent", "graphical.target"),
            ("cause", "host_install_only"),
        ],
    );
    let vendor_only = row_where(
        &json["manual_links"],
        &[
            ("parent", "multi-user.target"),
            ("cause", "vendor_install_only"),
        ],
    );
    let host_only_lines = advisories_in_preview(host_only, &preview);
    let vendor_only_lines = advisories_in_preview(vendor_only, &preview);
    for row in [host_only, vendor_only] {
        assert_eq!(row["relation"], "wants", "{row}");
        assert!(row.get("not_applied").is_none(), "{row}");
    }

    let drop_in = |path: &str| serde_json::json!({"kind": "DropIn", "key": {"path": path}});
    let (status, body) = set_include(&app, drop_in(add), false).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, json) = get_json(&app, "/api/view").await;
    let preview = preview_of(&json);
    let rows = json["manual_links"].as_array().expect("manual_links");
    assert_eq!(rows.len(), 2, "both rows stay: {rows:?}");
    let omitted = format!(
        "# Omitted: app.service links (excluded from the export: {add}; the [Install] the \
         image reads is not the one recorded, so the source host's links for app.service are \
         not reproduced, and links its package install made may remain in the image. Check \
         them with systemctl is-enabled in the image.)"
    );
    for row in rows {
        assert_eq!(row["not_applied"], serde_json::json!([add]), "{row}");
        assert_eq!(row["advisories"], serde_json::json!([omitted]), "{row}");
    }
    assert_eq!(
        preview.lines().filter(|l| *l == omitted).count(),
        1,
        "{preview}"
    );
    assert!(!preview.contains("add-wants"), "{preview}");

    let (status, body) = set_include(&app, drop_in(add), true).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, json) = get_json(&app, "/api/view").await;
    for row in json["manual_links"].as_array().expect("manual_links") {
        assert!(row.get("not_applied").is_none(), "{row}");
    }
    let preview = preview_of(&json);
    for line in host_only_lines.iter().chain(&vendor_only_lines) {
        assert!(preview.lines().any(|l| l == line), "{line}\n{preview}");
    }
    assert!(!preview.contains(&omitted), "{preview}");
}

/// T40's `UpheldBy=` drop-in unit: an `undetermined_installs[]` row with
/// its reason, its install file and the Containerfile's block, and no
/// `include` key. With the drop-in excluded, `not_applied` holds it and
/// `advisories` the unit's `# Omitted:` line, which the preview holds.
#[tokio::test]
async fn the_view_serves_undetermined_installs_and_their_omission() {
    const DROP_IN: &str = "/etc/systemd/system/need.service.d/10-reset.conf";
    let host = unit_hosts::UnitHost::new()
        .base_package("needpkg")
        .unit("need.service", "disabled")
        .preset("enable need.service")
        .vendor_unit(
            "need.service",
            "[Service]\nExecStart=/bin/true\n\n[Install]\nRequiredBy=a.target\n",
            Some("needpkg"),
        )
        .file(DROP_IN, "[Install]\nUpheldBy=a.target\n");
    let app = scanned_app(host);
    let (_, json) = get_json(&app, "/api/view").await;
    let row = row_by(&json["undetermined_installs"], "unit", "need.service");
    assert_eq!(row["reason"], "upheld_by", "{row}");
    assert_eq!(row["install_files"], serde_json::json!([DROP_IN]), "{row}");
    let block = advisories_in_preview(row, preview_of(&json));
    assert!(
        block[0].starts_with("# Advisory: need.service is not carried:"),
        "{block:?}"
    );
    assert!(row.get("not_applied").is_none(), "{row}");
    // Presence of a toggle key on the drop-in row first, then its absence.
    assert!(
        row_by(&json["unit_dropins"], "path", DROP_IN)
            .get("include")
            .is_some()
    );
    assert!(row.get("include").is_none(), "{row}");

    let (status, body) = set_include(
        &app,
        serde_json::json!({"kind": "DropIn", "key": {"path": DROP_IN}}),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, json) = get_json(&app, "/api/view").await;
    let row = row_by(&json["undetermined_installs"], "unit", "need.service");
    assert_eq!(row["not_applied"], serde_json::json!([DROP_IN]), "{row}");
    let lines = advisories_in_preview(row, preview_of(&json));
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("# Omitted: need.service"), "{lines:?}");
    assert!(lines[0].contains(DROP_IN), "{lines:?}");
}

/// A host whose base-image `systemd` ships `graphical.target`, with
/// `app.service` enabled and `default.target` linked to `link` when given.
fn default_target_host(link: Option<&str>) -> unit_hosts::UnitHost {
    let host = link_host()
        .base_package("systemd")
        .unit("app.service", "enabled")
        .preset("disable *")
        .vendor_unit("app.service", WANTED_SERVICE, Some(LINK_PACKAGE))
        .vendor_unit(
            "graphical.target",
            "[Unit]\nDescription=Graphical Interface\n",
            Some("systemd"),
        );
    match link {
        Some(target) => host.link("/etc/systemd/system/default.target", target),
        None => host,
    }
}

/// T26 through the handlers: the view's `default_target` row, and its
/// toggle, `ItemId::DefaultTarget` on the wire as a kind with no key,
/// takes the `set-default` line out of the preview and puts it back. With
/// no default target the key is absent and the operation is refused.
#[tokio::test]
async fn the_default_target_row_toggles_its_set_default_line() {
    const SET_DEFAULT: &str = "RUN systemctl set-default -- 'graphical.target'";
    let app = scanned_app(default_target_host(Some(
        "/usr/lib/systemd/system/graphical.target",
    )));
    let (_, json) = get_json(&app, "/api/view").await;
    assert_eq!(
        json["default_target"],
        serde_json::json!({
            "target": "graphical.target",
            "include": true,
            "locked": false,
            "declined": null,
        })
    );
    assert!(preview_of(&json).lines().any(|l| l == SET_DEFAULT));

    let item = serde_json::json!({"kind": "DefaultTarget"});
    let (status, body) = set_include(&app, item.clone(), false).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, json) = get_json(&app, "/api/view").await;
    assert_eq!(json["default_target"]["include"], false);
    assert!(!preview_of(&json).contains("set-default"), "{json}");

    let (status, body) = set_include(&app, item.clone(), true).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, json) = get_json(&app, "/api/view").await;
    assert_eq!(json["default_target"]["include"], true);
    assert!(preview_of(&json).lines().any(|l| l == SET_DEFAULT));

    let none = scanned_app(default_target_host(None));
    let (_, json) = get_json(&none, "/api/view").await;
    assert!(json.get("default_target").is_none(), "{json}");
    let (status, body) = set_include(&none, item, false).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("default target"), "{body}");
}

/// T26 (§ 10.5): a local target whose config row is excluded is declined,
/// and the row's `declined` text is the preview's `# Omitted:` line.
#[tokio::test]
async fn a_declined_default_target_shows_the_omitted_line() {
    const LOCAL: &str = "/etc/systemd/system/custom.target";
    let app = scanned_app(
        default_target_host(Some(LOCAL)).etc_unit("custom.target", "[Unit]\nDescription=Custom\n"),
    );
    let (_, json) = get_json(&app, "/api/view").await;
    assert_eq!(json["default_target"]["target"], "custom.target");
    assert!(json["default_target"]["declined"].is_null(), "{json}");

    let (status, body) = set_include(
        &app,
        serde_json::json!({"kind": "Config", "key": {"path": LOCAL}}),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, json) = get_json(&app, "/api/view").await;
    let declined = &json["default_target"]["declined"];
    assert_eq!(declined["renders"], "omitted", "{declined}");
    let text = declined["text"].as_str().expect("a text");
    assert!(
        preview_of(&json)
            .lines()
            .any(|l| l == format!("# Omitted: {text}")),
        "{text}\n{}",
        preview_of(&json)
    );
    assert!(
        json["default_target"]["include"] == true,
        "the record stays included"
    );
}

/// A unit row carries its `Also=` members with what their links say (T25
/// (a)'s owner with one member unlinked), and a unit whose `WantedBy=`
/// comes from an `/etc` drop-in carries that drop-in in
/// `install_drop_ins`. Each row lacks the other's list.
#[tokio::test]
async fn a_unit_row_serves_its_also_members_and_install_drop_ins() {
    let wanted_also = format!("{WANTED_SERVICE}Also=a.service b.service c.service\n");
    let mut host = link_host();
    for (name, state) in [
        ("bundle.service", "enabled"),
        ("a.service", "enabled"),
        ("b.service", "enabled"),
        ("c.service", "disabled"),
    ] {
        host = host.unit(name, state);
    }
    for rule in [
        "enable bundle.service",
        "enable a.service",
        "enable b.service",
        "disable *",
    ] {
        host = host.preset(rule);
    }
    for (name, text) in [
        ("bundle.service", wanted_also.as_str()),
        ("a.service", WANTED_SERVICE),
        ("b.service", WANTED_SERVICE),
        ("c.service", WANTED_SERVICE),
    ] {
        host = host.vendor_unit(name, text, Some(LINK_PACKAGE));
    }
    let (_, json) = get_json(&scanned_app(host), "/api/view").await;
    let owner = row_by(&json["unit_states"], "unit", "bundle.service");
    assert_eq!(
        owner["also"],
        serde_json::json!([
            {"unit": "a.service", "links": "installed"},
            {"unit": "b.service", "links": "installed"},
            {"unit": "c.service", "links": "unlinked"},
        ]),
        "{owner}"
    );
    assert!(owner.get("install_drop_ins").is_none(), "{owner}");

    const DROP_IN: &str = "/etc/systemd/system/app.service.d/10-install.conf";
    let host = link_host()
        .unit("app.service", "enabled")
        .preset("disable *")
        .vendor_unit("app.service", NO_INSTALL_SERVICE, Some(LINK_PACKAGE))
        .file(DROP_IN, "[Install]\nWantedBy=multi-user.target\n");
    let (_, json) = get_json(&scanned_app(host), "/api/view").await;
    let row = row_by(&json["unit_states"], "unit", "app.service");
    assert_eq!(
        row["install_drop_ins"],
        serde_json::json!([DROP_IN]),
        "{row}"
    );
    assert!(row.get("also").is_none(), "{row}");
}

/// The 389 Directory Server template, cut to what § 7.5 reads: the
/// instance's configuration is `/etc/dirsrv/slapd-%i` and its data
/// `/var/lib/dirsrv/slapd-%i`.
const DIRSRV_TEMPLATE: &str = "[Unit]\nDescription=389 Directory Server %i.\n\n[Service]\n\
     Type=notify\nExecStart=/usr/sbin/ns-slapd -D /etc/dirsrv/slapd-%i -i /run/dirsrv/slapd-%i.pid\n\n\
     [Install]\nWantedBy=multi-user.target\n";

/// § 11.1 through the production handlers: an instance row carries its
/// template in `instance_of`, its configuration in `required_config` and
/// its data in `data_paths`; a plain unit row carries both lists empty
/// and no `instance_of`.
#[tokio::test]
async fn an_instance_row_serves_its_template_configuration_and_data() {
    const INSTANCE: &str = "dirsrv@example.service";
    const TEMPLATE: &str = "dirsrv@.service";
    const PACKAGE: &str = "389-ds-base";
    let host = unit_hosts::UnitHost::new()
        .base_package("glibc")
        .package(PACKAGE)
        .unit(TEMPLATE, "indirect")
        .preset("disable *")
        .vendor_unit(TEMPLATE, DIRSRV_TEMPLATE, Some(PACKAGE))
        .link(
            &format!("/etc/systemd/system/multi-user.target.wants/{INSTANCE}"),
            &format!("/usr/lib/systemd/system/{TEMPLATE}"),
        )
        .file("/etc/dirsrv/slapd-example/dse.ldif", "dn: cn=config\n")
        .dir("/var/lib/dirsrv/slapd-example");
    let (_, json) = get_json(&scanned_app(host), "/api/view").await;
    let row = row_by(&json["unit_states"], "unit", INSTANCE);
    assert_eq!(row["instance_of"], TEMPLATE, "{row}");
    assert_eq!(
        row["required_config"],
        serde_json::json!(["/etc/dirsrv/slapd-example"]),
        "{row}"
    );
    assert_eq!(
        row["data_paths"],
        serde_json::json!(["/var/lib/dirsrv/slapd-example"]),
        "{row}"
    );

    let host = link_host()
        .unit("app.service", "enabled")
        .preset("disable *")
        .vendor_unit("app.service", WANTED_SERVICE, Some(LINK_PACKAGE));
    let (_, json) = get_json(&scanned_app(host), "/api/view").await;
    let row = row_by(&json["unit_states"], "unit", "app.service");
    assert!(row.get("instance_of").is_none(), "{row}");
    assert_eq!(row["required_config"], serde_json::json!([]), "{row}");
    assert_eq!(row["data_paths"], serde_json::json!([]), "{row}");
}
