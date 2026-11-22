//! The System Tuning fixtures the frontend's navigation test loads, held
//! to what the real handlers return (spec § 8 case 22).
//!
//! Each session is built by the collector-driven harness the scan tests
//! use, then served by the real router. Every response body is compared
//! with `crates/web/ui/src/__tests__/fixtures/system-tuning/
//! <session>.<endpoint>.json`, so a backend change that alters what the
//! frontend receives fails here until the fixtures are re-recorded:
//!
//! ```text
//! UPDATE_SYSTEM_TUNING_FIXTURES=1 cargo test -p inspectah-web --test integration system_tuning_fixtures_test::
//! ```

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use inspectah_core::aggregate::merge_snapshots;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_refine::session::RefineSession;
use inspectah_web::handlers::AppState;
use tower::ServiceExt;

use crate::sysctl_hosts::{SysctlHost, reload, scan, session};

/// Set to write the fixtures instead of comparing against them.
const UPDATE_VAR: &str = "UPDATE_SYSTEM_TUNING_FIXTURES";
/// Where the frontend's tests read the fixtures, under this crate.
const FIXTURE_DIR: &str = "ui/src/__tests__/fixtures/system-tuning";
/// The fleet merge stamps the wall clock into `merged_at`, which the
/// health body reports. The aggregate session is pinned to this value so
/// its fixtures are the same on every run.
const FIXED_MERGED_AT: &str = "2026-11-11T00:00:00+00:00";

/// The endpoints every session is recorded at, by fixture name.
const SINGLE_HOST_ENDPOINTS: &[(&str, &str)] = &[
    ("health", "/api/health"),
    ("groups", "/api/groups"),
    ("view", "/api/view"),
];
/// An aggregate session also serves the aggregate view.
const AGGREGATE_ENDPOINTS: &[(&str, &str)] = &[
    ("health", "/api/health"),
    ("groups", "/api/groups"),
    ("view", "/api/view"),
    ("aggregate-view", "/api/aggregate/view"),
];

fn app(state: Arc<AppState>) -> axum::Router {
    inspectah_web::router(state, "http://localhost:8642")
        .layer(axum::middleware::map_request(with_loopback_host))
}

/// A `oneshot` request carries no `Host`, which the router refuses; a
/// real client always sends one.
async fn with_loopback_host(mut req: Request<Body>) -> Request<Body> {
    req.headers_mut()
        .entry(axum::http::header::HOST)
        .or_insert(axum::http::HeaderValue::from_static("localhost:8642"));
    req
}

async fn get_json(app: &axum::Router, path: &str) -> serde_json::Value {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        StatusCode::OK,
        "{path}: {}",
        String::from_utf8_lossy(&body)
    );
    serde_json::from_slice(&body).unwrap_or_else(|e| panic!("{path} returns JSON: {e}"))
}

/// Serve `session` and compare (or, with [`UPDATE_VAR`] set, write) the
/// body of each endpoint as `<name>.<endpoint>.json`.
async fn record(name: &str, session: RefineSession, endpoints: &[(&str, &str)]) {
    let app = app(Arc::new(AppState {
        session: Arc::new(Mutex::new(session)),
    }));
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_DIR);
    let update = std::env::var_os(UPDATE_VAR).is_some();
    for (endpoint, path) in endpoints {
        let body = get_json(&app, path).await;
        let actual = serde_json::to_string_pretty(&body).expect("the body serializes") + "\n";
        let file = dir.join(format!("{name}.{endpoint}.json"));
        if update {
            std::fs::create_dir_all(&dir).expect("the fixture directory is created");
            std::fs::write(&file, &actual)
                .unwrap_or_else(|e| panic!("{} is written: {e}", file.display()));
            continue;
        }
        let expected = std::fs::read_to_string(&file).unwrap_or_else(|e| {
            panic!(
                "fixture {} is missing ({e}); record it with {UPDATE_VAR}=1",
                file.display()
            )
        });
        assert!(
            expected == actual,
            "{} differs from what {path} returns; re-record with {UPDATE_VAR}=1 and review the diff\n\
             --- returned ---\n{actual}",
            file.display()
        );
    }
}

/// A single host's scan, served from the session its tarball opens.
async fn record_host(name: &str, host: SysctlHost) {
    let snap = reload(&scan(host));
    record(name, session(&snap), SINGLE_HOST_ENDPOINTS).await;
}

/// Case 15, host A: only a mask and the vendor file it masks.
#[tokio::test]
async fn mask_only_host() {
    record_host(
        "mask-only",
        SysctlHost::new()
            .file("/usr/lib/sysctl.d/50-x.conf", "vm.swappiness = 10\n")
            .link("/etc/sysctl.d/50-x.conf", "/dev/null")
            .sysctl_a("vm.swappiness = 60\n"),
    )
    .await;
}

/// Case 15, host B: only a glob line.
#[tokio::test]
async fn glob_only_host() {
    record_host(
        "glob-only",
        SysctlHost::new().file(
            "/etc/sysctl.d/50-rp.conf",
            "net.ipv4.conf.*.rp_filter = 2\n",
        ),
    )
    .await;
}

/// Case 16: `sysctl -a` exits non-zero, so the explanation is unavailable
/// while the `/etc` file is still carried.
#[tokio::test]
async fn sysctl_a_fails_host() {
    record_host(
        "sysctl-a-fails",
        SysctlHost::new()
            .file("/etc/sysctl.d/10-keep.conf", "vm.swappiness = 10\n")
            .sysctl_a_fails(1),
    )
    .await;
}

/// Case 17, overlapping assignment: an `/etc` link out of the sysctl
/// directories makes the explanation unknown.
#[tokio::test]
async fn outside_link_overlap_host() {
    record_host(
        "outside-link-overlap",
        SysctlHost::new()
            .file("/usr/lib/sysctl.d/50-default.conf", "vm.swappiness = 10\n")
            .link("/etc/sysctl.d/90-site.conf", "/usr/share/site/tuning.conf")
            .file("/usr/share/site/tuning.conf", "vm.swappiness = 60\n")
            .sysctl_a("vm.swappiness = 60\n"),
    )
    .await;
}

/// A host with sysctl files, one key drifted, and no tuned selection.
#[tokio::test]
async fn files_without_tuned_host() {
    record_host(
        "files-no-tuned",
        SysctlHost::new()
            .file(
                "/etc/sysctl.d/90-tuning.conf",
                "vm.swappiness = 10\nnet.ipv4.ip_forward = 1\n",
            )
            .sysctl_a("vm.swappiness = 60\nnet.ipv4.ip_forward = 1\n"),
    )
    .await;
}

/// A fleet host named `hostname` whose one `/etc/sysctl.d` file holds
/// `content`.
fn fleet_host(hostname: &str, content: &str) -> InspectionSnapshot {
    reload(&scan(
        SysctlHost::new()
            .file("/etc/hostname", &format!("{hostname}\n"))
            .file("/etc/sysctl.d/90-x.conf", content)
            .sysctl_a("vm.swappiness = 10\n"),
    ))
}

/// Case 21: two hosts with different content at one path and no tuned
/// selection, merged.
#[tokio::test]
async fn aggregate_of_two_hosts() {
    let hosts = vec![
        fleet_host("host-a", "vm.swappiness = 10\n"),
        fleet_host("host-b", "vm.swappiness = 20\n"),
    ];
    let (mut merged, _warnings) = merge_snapshots(hosts, None, None).expect("the two hosts merge");
    merged
        .aggregate_meta
        .as_mut()
        .expect("a merge records aggregate meta")
        .merged_at = FIXED_MERGED_AT.into();
    let merged = reload(&merged);
    record("aggregate", session(&merged), AGGREGATE_ENDPOINTS).await;
}

/// A host with one machine-state file per override level and per notice
/// kind, for the refine UI's machine-state block: the hostname (`plain`,
/// set at provisioning), the iSCSI IQN (`warn`, first boot), a keytab and
/// an SSH host key (`never`, one owed step and one first-boot), the
/// Insights machine-id (`never`, owed) and `00_tuned` (`plain`, no notice).
fn machine_state_snapshot() -> InspectionSnapshot {
    reload(&scan(
        SysctlHost::new()
            .file("/etc/hostname", "web-01.example.com\n")
            .file(
                "/etc/iscsi/initiatorname.iscsi",
                "InitiatorName=iqn.1994-05.com.redhat:web01\n",
            )
            .file_bytes("/etc/krb5.keytab", &[0x05, 0x02, 0, 0, 0, 0x40])
            .file("/etc/ssh/ssh_host_ed25519_key", "not a real key\n")
            .file("/etc/insights-client/machine-id", "0f3c\n")
            .file("/etc/grub.d/00_tuned", "#!/bin/sh\n")
            .file("/etc/sysctl.d/90-x.conf", "vm.swappiness = 10\n")
            .sysctl_a("vm.swappiness = 10\n"),
    ))
}

/// The machine-state host as scanned: every class row left out.
#[tokio::test]
async fn machine_state_host() {
    record(
        "machine-state",
        session(&machine_state_snapshot()),
        SINGLE_HOST_ENDPOINTS,
    )
    .await;
}

/// The machine-state host after the operator included the IQN (`warn`)
/// and the hostname (`plain`): the rows the UI shows as included by the
/// operator, and the `warn` notice the export adds.
#[tokio::test]
async fn machine_state_host_with_overrides() {
    use inspectah_refine::types::{ItemId, RefinementOp};
    let mut session = session(&machine_state_snapshot());
    for path in ["/etc/iscsi/initiatorname.iscsi", "/etc/hostname"] {
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Config { path: path.into() },
                include: true,
            })
            .expect("a carriable machine-state row is included");
    }
    record("machine-state-included", session, &[("view", "/api/view")]).await;
}

/// The machine-state host after the operator excluded its one ordinary
/// config, the sysctl drop-in: Configuration's decision list is all
/// excluded while the machine-state block still has every row.
#[tokio::test]
async fn machine_state_host_with_configs_excluded() {
    use inspectah_refine::types::{ItemId, RefinementOp};
    let mut session = session(&machine_state_snapshot());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Config {
                path: "/etc/sysctl.d/90-x.conf".into(),
            },
            include: false,
        })
        .expect("an ordinary config is excluded");
    record(
        "machine-state-configs-excluded",
        session,
        &[("view", "/api/view")],
    )
    .await;
}
