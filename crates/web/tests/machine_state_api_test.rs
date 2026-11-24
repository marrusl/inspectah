//! The machine-state class's refine API contract (spec § 8), through the
//! real router: each refused op is a 422 that records nothing, on
//! sessions built from collector output by the scan tests' harness.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::rpm::RpmInspector;
use inspectah_core::aggregate::merge_snapshots;
use inspectah_core::machine_state::ScreenedExecutor;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::inspector::Inspector;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefinementOp, TimelineEntry};
use inspectah_web::handlers::AppState;
use tower::ServiceExt;

use crate::sysctl_hosts::{SysctlHost, reload, scan_exec, scan_with};

const IQN: &str = "/etc/iscsi/initiatorname.iscsi";
const KEYTAB: &str = "/etc/krb5.keytab";
const KEY: &str = "-----BEGIN ENCRYPTED PRIVATE KEY-----\nMIIFapikeymarker\n\
                   -----END ENCRYPTED PRIVATE KEY-----\n";

fn inspectors() -> Vec<Box<dyn Inspector>> {
    vec![
        Box::new(RpmInspector::new()),
        Box::new(ConfigInspector::new()),
    ]
}

fn app(snap: InspectionSnapshot) -> axum::Router {
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
    });
    inspectah_web::router(state, "http://localhost:8642")
}

async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(axum::http::header::HOST, "localhost:8642")
        .header("content-type", "application/json");
    let body = body.map_or_else(Body::empty, |b| Body::from(serde_json::to_vec(&b).unwrap()));
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

/// `POST /api/op` with `op`, and `/api/ops` before and after.
async fn post_op(
    app: &axum::Router,
    op: RefinementOp,
) -> (
    StatusCode,
    serde_json::Value,
    serde_json::Value,
    serde_json::Value,
) {
    let (_, before) = send(app, "GET", "/api/ops", None).await;
    assert!(before.is_array(), "{before}");
    let body = serde_json::to_value(TimelineEntry::Op(op)).unwrap();
    let (status, json) = send(app, "POST", "/api/op", Some(body)).await;
    let (_, after) = send(app, "GET", "/api/ops", None).await;
    (status, json, before, after)
}

fn set_include(item_id: ItemId, include: bool) -> RefinementOp {
    RefinementOp::SetInclude { item_id, include }
}

fn config(path: &str) -> ItemId {
    ItemId::Config { path: path.into() }
}

/// 422 with an `error` starting `prefix`, and `/api/ops` unchanged.
async fn assert_refused(app: &axum::Router, op: RefinementOp, prefix: &str) {
    let (status, json, before, after) = post_op(app, op.clone()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{op:?}: {json}");
    let error = json["error"].as_str().unwrap_or_default();
    assert!(error.starts_with(prefix), "{op:?}: {error}");
    assert!(!error.contains("apikeymarker"), "{op:?}: {error}");
    assert_eq!(before, after, "{op:?}: nothing recorded");
}

fn fleet_host(hostname: &str, path: &str, content: &str) -> InspectionSnapshot {
    reload(&scan_with(
        SysctlHost::new()
            .file("/etc/hostname", &format!("{hostname}\n"))
            .file(path, content),
        inspectors(),
    ))
}

fn aggregate(hosts: Vec<InspectionSnapshot>) -> InspectionSnapshot {
    reload(
        &merge_snapshots(hosts, None, None)
            .expect("the hosts merge")
            .0,
    )
}

/// Collect case 17 over HTTP: a credential's include is refused with
/// `not carried:`, and its view row is `never` with no `item_id`.
#[tokio::test]
async fn a_credential_include_is_refused_over_http() {
    let snap = scan_with(
        SysctlHost::new().file_bytes(KEYTAB, &[0x05, 0x02, 0x00, 0xff]),
        inspectors(),
    );
    let app = app(snap);
    assert_refused(&app, set_include(config(KEYTAB), true), "not carried:").await;
    let (_, view) = send(&app, "GET", "/api/view", None).await;
    let row = view["machine_state"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["path"] == KEYTAB)
        .expect("a view row");
    assert_eq!(row["override"], "never");
    assert!(
        row.get("item_id").is_none_or(serde_json::Value::is_null),
        "{row}"
    );
}

/// A carriable row's decision is refused with `nothing to change` when
/// it already holds, before and after an include, and an include that
/// changes it records one op.
#[tokio::test]
async fn a_redundant_carriable_decision_is_refused_over_http() {
    let snap = scan_with(
        SysctlHost::new()
            .rpm_owned(IQN, "iscsi-initiator-utils")
            .rpm_va("S.5....T.", IQN)
            .file(IQN, "InitiatorName=iqn.1994-05.com.redhat:api\n"),
        inspectors(),
    );
    let app = app(snap);
    assert_refused(&app, set_include(config(IQN), false), "nothing to change:").await;
    let (status, json, before, after) = post_op(&app, set_include(config(IQN), true)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        after.as_array().map(Vec::len),
        before.as_array().map(|ops| ops.len() + 1),
        "one op recorded: {after}"
    );
    assert_refused(&app, set_include(config(IQN), true), "nothing to change:").await;
}

/// Collect case 31 over HTTP: an include of a collected withheld
/// unmanaged row is refused with `not carried:`.
#[tokio::test]
async fn a_withheld_unmanaged_include_is_refused_over_http() {
    let exec = SysctlHost::new()
        .file_bytes("/opt/app/client.kt", &[0x05, 0x02, 0x00, 0xff])
        .file("/opt/app/app.conf", "a = 1\n")
        .build();
    let mut snap = scan_exec(&exec, inspectors());
    snap.unmanaged_files = Some(inspectah_collect::inspectors::nonrpm::scan_unmanaged_files(
        &ScreenedExecutor::new(&exec),
        &[],
        &[],
    ));
    let snap = reload(&snap);
    let withheld = &snap.unmanaged_files.as_ref().unwrap().withheld;
    assert_eq!(withheld.len(), 1, "{withheld:?}");
    let app = app(snap);
    assert_refused(
        &app,
        set_include(
            ItemId::UnmanagedFile {
                path: "/opt/app/client.kt".into(),
            },
            true,
        ),
        "not carried:",
    )
    .await;
}

/// Collect cases 21 and 27 over HTTP, on fleet aggregates: an include of
/// a class row is refused, and so is an edit whose content is a key.
#[tokio::test]
async fn fleet_class_rows_and_key_edits_are_refused_over_http() {
    let iqns = aggregate(vec![
        fleet_host("host-a", IQN, "InitiatorName=iqn.2000-01.com.example:a\n"),
        fleet_host("host-b", IQN, "InitiatorName=iqn.2000-01.com.example:b\n"),
    ]);
    assert_refused(&app(iqns), set_include(config(IQN), true), "not carried:").await;

    let cert = |n: u8| format!("-----BEGIN CERTIFICATE-----\nMIIB{n}\n-----END CERTIFICATE-----\n");
    let certs = aggregate(vec![
        fleet_host("host-a", "/etc/myapp/server.pem", &cert(1)),
        fleet_host("host-b", "/etc/myapp/server.pem", &cert(2)),
    ]);
    assert_refused(
        &app(certs),
        RefinementOp::EditVariant {
            item_id: config("/etc/myapp/server.pem"),
            content: KEY.into(),
            based_on: None,
        },
        "not carried:",
    )
    .await;
}

/// The NVMe host id row carries its own notice, the audit report's text,
/// in place of its member's generic first-boot line; a member whose
/// generic line holds carries none.
#[tokio::test]
async fn the_nvme_host_id_row_carries_the_audit_reports_notice() {
    use inspectah_core::types::machine_state::{CarriableMember, MachineStateMember};
    const HOSTNQN: &str = "/etc/nvme/hostnqn";
    let snap = scan_with(
        SysctlHost::new()
            .rpm_owned(IQN, "iscsi-initiator-utils")
            .rpm_va("S.5....T.", IQN)
            .file(IQN, "InitiatorName=iqn.1994-05.com.redhat:api\n")
            .rpm_owned(HOSTNQN, "nvme-cli")
            .rpm_va("S.5....T.", HOSTNQN)
            .file(HOSTNQN, "nqn.2014-08.org.nvmexpress:uuid:api\n"),
        inspectors(),
    );
    let (_, view) = send(&app(snap), "GET", "/api/view", None).await;
    let row = |path: &str| {
        view["machine_state"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["path"] == path)
            .unwrap_or_else(|| panic!("a view row for {path}: {view}"))
            .clone()
    };
    let expected = inspectah_pipeline::render::machine_state::member_notice(
        MachineStateMember::Carriable(CarriableMember::NvmeHostId),
    )
    .expect("the NVMe host id member has its own notice");
    let nvme = row(HOSTNQN);
    assert_eq!(nvme["member_notice"], expected, "{nvme}");
    assert!(!expected.contains("first boot"), "{expected}");
    let iqn = row(IQN);
    assert!(iqn.get("member_notice").is_none(), "{iqn}");
}
