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
use crate::unit_hosts;

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

/// A local template whose instance runs only when its private key is
/// there: `tls-proxy@web.service`, enabled by its link, names
/// `/etc/pki/tls/private/web.key` as its configuration, and that path is
/// a machine-state row (a host credential), not a config row. The units
/// section links the path to that row.
#[tokio::test]
async fn machine_state_unit_host() {
    use unit_hosts::UnitHost;
    const TEMPLATE: &str = "tls-proxy@.service";
    let host = UnitHost::new()
        .base_package("glibc")
        .unit(TEMPLATE, "indirect")
        .preset("disable *")
        .etc_unit(
            TEMPLATE,
            "[Unit]\nDescription=TLS proxy for %i\n\
             ConditionPathExists=/etc/pki/tls/private/%i.key\n\n\
             [Service]\nExecStart=/usr/local/bin/tls-proxy %i\n\n\
             [Install]\nWantedBy=multi-user.target\n",
        )
        .link(
            "/etc/systemd/system/multi-user.target.wants/tls-proxy@web.service",
            "/etc/systemd/system/tls-proxy@.service",
        )
        .file(
            "/etc/pki/tls/private/web.key",
            "-----BEGIN PRIVATE KEY-----\nnot a real key\n-----END PRIVATE KEY-----\n",
        );
    let snap = unit_hosts::scan(host);
    record(
        "machine-state-unit",
        unit_hosts::session(&snap),
        &[("view", "/api/view")],
    )
    .await;
}

/// A public key block, as the scan accepts it.
const REPO_PGP: &str =
    "-----BEGIN PGP PUBLIC KEY BLOCK-----\nmQENBFtest\n-----END PGP PUBLIC KEY BLOCK-----\n";

/// A host for the refine UI's Repositories section and repo-owned
/// Configuration rows, with one repo per control and one owned entry per
/// export state a single host can show:
///
/// - `custom`: a full switch with members `tool-a` and `tool-b`, its
///   unowned `custom.repo` and key;
/// - `epel`: a full switch over a modified `epel.repo` owned by
///   `epel-release`, with `[epel-debuginfo]` disabled beside it, and its
///   RPM-owned key;
/// - `baseos`: a carriage-only switch over an unowned `company.repo` that
///   mirrors it, serving `site-tool`;
/// - `a` and `b`: two zero-member repos naming one key, `b` with a `//`
///   in the reference, so the key's recorded paths hold two spellings;
/// - `rej`: a repo naming a key the scan refuses (not a PGP key);
/// - `vend`: a repo whose `gpgkey=` redaction rewrites, so its key has no
///   owner;
/// - `dis`: a repo file whose one section is disabled, with no package,
///   and its key: a carriage-only switch is the only control over them;
/// - an unowned `redhat.repo` with a distro section and no member: a
///   default repo file no switch decides.
fn repos_host() -> unit_hosts::UnitHost {
    let repo = |id: &str, key: &str| {
        format!(
            "[{id}]\nname={id}\nbaseurl=https://{id}.example/repo\nenabled=1\ngpgkey=file://{key}\n"
        )
    };
    unit_hosts::UnitHost::new()
        .base_package("bash")
        .file(
            "/etc/yum.repos.d/custom.repo",
            &repo("custom", "/etc/pki/rpm-gpg/RPM-GPG-KEY-custom"),
        )
        .file("/etc/pki/rpm-gpg/RPM-GPG-KEY-custom", REPO_PGP)
        .owned_file(
            "/etc/yum.repos.d/epel.repo",
            "[epel]\nname=EPEL\nmetalink=https://mirrors.example/epel\nenabled=1\n\
             gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n\n[epel-debuginfo]\n\
             name=EPEL debug\nmetalink=https://mirrors.example/epel-debug\nenabled=0\n\
             gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n",
            "epel-release",
        )
        .owned_file(
            "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9",
            REPO_PGP,
            "epel-release",
        )
        .rpm_va("S.5....T.  c /etc/yum.repos.d/epel.repo\n", 1)
        .file(
            "/etc/yum.repos.d/company.repo",
            "[baseos]\nname=BaseOS mirror\nbaseurl=https://mirror.corp.example/baseos/\n\
             enabled=1\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-company\n",
        )
        .file("/etc/pki/rpm-gpg/RPM-GPG-KEY-company", REPO_PGP)
        .file(
            "/etc/yum.repos.d/a.repo",
            &repo("a", "/etc/pki/rpm-gpg/RPM-GPG-KEY-shared"),
        )
        .file(
            "/etc/yum.repos.d/b.repo",
            &repo("b", "/etc/pki/rpm-gpg//RPM-GPG-KEY-shared"),
        )
        .file("/etc/pki/rpm-gpg/RPM-GPG-KEY-shared", REPO_PGP)
        .file(
            "/etc/yum.repos.d/reject.repo",
            &repo("rej", "/etc/pki/rpm-gpg/RPM-GPG-KEY-notpgp"),
        )
        .file(
            "/etc/pki/rpm-gpg/RPM-GPG-KEY-notpgp",
            "plain text, not a key\n",
        )
        .file(
            "/etc/yum.repos.d/vend.repo",
            &repo("vend", "/etc/pki/rpm-gpg/token=vendor"),
        )
        .file("/etc/pki/rpm-gpg/token=vendor", REPO_PGP)
        .file(
            "/etc/yum.repos.d/dis.repo",
            "[dis]\nname=dis\nbaseurl=https://dis.example/repo\nenabled=0\n\
             gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-dis\n",
        )
        .file("/etc/pki/rpm-gpg/RPM-GPG-KEY-dis", REPO_PGP)
        .file(
            "/etc/yum.repos.d/redhat.repo",
            "[rhel-9-for-x86_64-baseos-rpms]\nname=RHEL 9 BaseOS\n\
             baseurl=https://cdn.redhat.example/baseos\nenabled=1\n",
        )
        .enabled_repos(&["custom", "epel", "baseos", "a", "b", "rej", "vend"])
        .package("epel-release")
        .package_from("tool-a", "custom")
        .package_from("tool-b", "custom")
        .package_from("bat", "epel")
        .package_from("site-tool", "baseos")
}

/// The repo host as scanned: every repo kept, every carriable file and key
/// shipped.
#[tokio::test]
async fn repos_host_as_scanned() {
    let snap = unit_hosts::reload(&unit_hosts::scan(repos_host()));
    record("repos", unit_hosts::session(&snap), SINGLE_HOST_ENDPOINTS).await;
}

/// The repo host after the operator excluded `custom` (its members, file
/// and key leave), `a` (the shared key ships, kept by `b`) and `baseos`
/// (a carriage-only switch: `company.repo` and its key leave, and
/// `site-tool` stays included and loses its route).
#[tokio::test]
async fn repos_host_with_repos_excluded() {
    use inspectah_refine::types::{ItemId, RefinementOp};
    let snap = unit_hosts::reload(&unit_hosts::scan(repos_host()));
    let mut session = unit_hosts::session(&snap);
    for id in ["custom", "a", "baseos"] {
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Repo { path: id.into() },
                include: false,
            })
            .unwrap_or_else(|e| panic!("repo {id} is excluded: {e}"));
    }
    record("repos-excluded", session, &[("view", "/api/view")]).await;
}

/// Two fleet hosts, `custom.repo` and its key on one only: the aggregate
/// narrows both out, so `custom`'s row is kept with nothing carried.
#[tokio::test]
async fn repos_aggregate_of_two_hosts() {
    let repo = "[custom]\nname=custom\nbaseurl=https://custom.example/repo\nenabled=1\n\
                gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-custom\n";
    let host = |name: &str, with_custom: bool| {
        let mut h = unit_hosts::UnitHost::new()
            .base_package("bash")
            .file("/etc/hostname", &format!("{name}\n"))
            .enabled_repos(&["custom"]);
        if with_custom {
            h = h
                .file("/etc/yum.repos.d/custom.repo", repo)
                .file("/etc/pki/rpm-gpg/RPM-GPG-KEY-custom", REPO_PGP);
        }
        unit_hosts::reload(&unit_hosts::scan(h))
    };
    let hosts = vec![host("host-a", true), host("host-b", false)];
    let (mut merged, _warnings) = merge_snapshots(hosts, None, None).expect("the two hosts merge");
    merged
        .aggregate_meta
        .as_mut()
        .expect("a merge records aggregate meta")
        .merged_at = FIXED_MERGED_AT.into();
    let merged = unit_hosts::reload(&merged);
    record(
        "repos-aggregate",
        unit_hosts::session(&merged),
        &[("aggregate-view", "/api/aggregate/view")],
    )
    .await;
}
