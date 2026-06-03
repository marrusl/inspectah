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
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind};
use inspectah_core::types::containers::{ContainerSection, QuadletUnit, RunningContainer};
use inspectah_core::types::kernelboot::{ConfigSnippet, KernelBootSection, SysctlOverride};
use inspectah_core::types::network::{NMConnection, NetworkSection};
use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection, PipPackage};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection, VersionChange};
use inspectah_core::types::scheduled::{CronJob, ScheduledTaskSection};
use inspectah_core::types::selinux::{CarryForwardFile, SelinuxSection};
use inspectah_core::types::services::{
    PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
};
use inspectah_core::types::storage::{FstabEntry, StorageSection};
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
            locked: false,
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
            locked: false,
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

    snap.scheduled_tasks = Some(ScheduledTaskSection {
        cron_jobs: vec![CronJob {
            path: "/etc/cron.d/logrotate".into(),
            source: "file".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![NonRpmItem {
            name: "node".into(),
            path: "/usr/local/bin/node".into(),
            method: "binary".into(),
            confidence: "high".into(),
            lang: "javascript".into(),
            version: "20.11.0".into(),
            packages: vec![PipPackage {
                name: "express".into(),
                version: "4.18.2".into(),
            }],
            ..Default::default()
        }],
        env_files: vec![ConfigFileEntry {
            path: "/etc/sysconfig/node-app".into(),
            kind: ConfigFileKind::Unowned,
            content: "NODE_ENV=production".into(),
            ..Default::default()
        }],
    });

    snap.selinux = Some(SelinuxSection {
        mode: "enforcing".into(),
        fips_mode: false,
        boolean_overrides: vec![serde_json::json!({
            "name": "httpd_can_network_connect",
            "state": true,
        })],
        custom_modules: vec!["myapp_policy".into()],
        audit_rules: vec![CarryForwardFile {
            path: "etc/audit/rules.d/10-custom.rules".into(),
            content: "-w /etc/shadow -p wa -k shadow_changes".into(),
        }],
        pam_configs: vec![CarryForwardFile {
            path: "etc/pam.d/sshd-custom".into(),
            content: "auth required pam_google_authenticator.so".into(),
        }],
        ..Default::default()
    });

    snap.network = Some(NetworkSection {
        connections: vec![NMConnection {
            name: "eth0".into(),
            conn_type: "802-3-ethernet".into(),
            method: "auto".into(),
            path: "/etc/NetworkManager/system-connections/eth0.nmconnection".into(),
            ..Default::default()
        }],
        resolv_provenance: "NetworkManager".into(),
        hosts_additions: vec!["10.0.0.50 app-server".into()],
        ..Default::default()
    });

    snap.storage = Some(StorageSection {
        fstab_entries: vec![FstabEntry {
            device: "/dev/mapper/rhel-root".into(),
            mount_point: "/".into(),
            fstype: "xfs".into(),
            options: "defaults".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    snap.kernel_boot = Some(KernelBootSection {
        cmdline: "quiet crashkernel=auto rd.lvm.lv=rhel/root".into(),
        sysctl_overrides: vec![SysctlOverride {
            key: "net.ipv4.ip_forward".into(),
            runtime: "1".into(),
            default: "0".into(),
            source: "/etc/sysctl.d/k8s.conf".into(),
            ..Default::default()
        }],
        modules_load_d: vec![ConfigSnippet {
            path: "/etc/modules-load.d/br_netfilter.conf".into(),
            content: "br_netfilter".into(),
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
