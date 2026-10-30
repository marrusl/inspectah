use inspectah_core::types::FindingKind;
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
use inspectah_core::types::config::{
    ConfigCategory, ConfigFileEntry, ConfigFileKind, ConfigSection,
};
use inspectah_core::types::containers::{ContainerSection, QuadletUnit, RunningContainer};
use inspectah_core::types::kernelboot::{KernelBootSection, SysctlOverride};
use inspectah_core::types::network::{NMConnection, NetworkSection};
use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection, PipPackage};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::rpm::{
    InstalledGroup, PackageEntry, PackageState, RpmSection, VersionChange,
};
use inspectah_core::types::scheduled::{CronJob, ScheduledTaskSection};
use inspectah_core::types::selinux::SelinuxSection;
use inspectah_core::types::services::{
    PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
};
use inspectah_core::types::storage::{FstabEntry, StorageSection, VarDirBacking, VarDirectory};
use inspectah_core::types::{AdvisoryType, config::ConfigShadow};
use inspectah_refine::projection::project_reference;
use inspectah_refine::session::RefineSession;
use inspectah_web::adapter::{VAR_SUMMARY_ID, build_web_sections};
use inspectah_web::handlers::AppState;
use inspectah_web::web_types::ContextItem;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tower::ServiceExt;

/// A reference row with neither `decision` nor `advisory` serializes
/// exactly as it did before those two fields existed (§ 3.8). The inline
/// snapshot below was hand-written against the five-field `ContextItem`
/// and must not be re-accepted: it is the only byte-identity this spec
/// promises, and re-capturing it after the change would make it vacuous.
///
/// `serde_json::to_string` rather than `assert_json_snapshot!` on
/// purpose — the latter round-trips through a sorted map and would hide
/// both field order and any newly serialized key.
#[test]
fn plain_context_item_is_byte_identical() {
    let item = ContextItem {
        id: "net.ipv4.ip_forward".to_string(),
        title: "net.ipv4.ip_forward".to_string(),
        subtitle: Some("\"1\" (default: \"0\")".to_string()),
        detail: None,
        searchable_text: "net.ipv4.ip_forward 1 0 /etc/sysctl.d/k8s.conf".to_string(),
        decision: None,
        advisory: None,
    };
    insta::assert_snapshot!(
        serde_json::to_string(&item).expect("ContextItem serializes"),
        @r#"{"id":"net.ipv4.ip_forward","title":"net.ipv4.ip_forward","subtitle":"\"1\" (default: \"0\")","detail":null,"searchable_text":"net.ipv4.ip_forward 1 0 /etc/sysctl.d/k8s.conf"}"#
    );
}

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
            disposition: FindingKind::included(),
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
            disposition: FindingKind::excluded(),
            locked: false,
            owning_package: None,
            aggregate: None,
            attention_reason: None,
            shadow_type: None,
            shadow_rationale: None,
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
                pinned: false,
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
        ..Default::default()
    });

    // The audit rule and the PAM config are `config.files` entries (§ 3.6);
    // Security & Access Control carries a pointer row for each.
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry {
                path: "/etc/audit/rules.d/10-custom.rules".into(),
                kind: ConfigFileKind::Unowned,
                category: ConfigCategory::Audit,
                content: "-w /etc/shadow -p wa -k shadow_changes".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/pam.d/sshd-custom".into(),
                kind: ConfigFileKind::Unowned,
                category: ConfigCategory::Pam,
                content: "auth required pam_google_authenticator.so".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/modules-load.d/br_netfilter.conf".into(),
                kind: ConfigFileKind::Unowned,
                category: ConfigCategory::KernelModule,
                content: "br_netfilter".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
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

/// Grouped view contract: exercises package groups with mixed base/added dependencies.
/// httpd and mod_ssl are user-added, apr and apr-util are from base image.
/// Expected: added_count=2, member_count=4, members sorted new-first, in_base_image flag set correctly.
#[tokio::test]
async fn grouped_view_contract() {
    let mut snap = InspectionSnapshot::new();

    snap.meta.insert(
        "hostname".to_string(),
        serde_json::json!("grouped-host.example.com"),
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
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                version: "2.4.57".into(),
                release: "11.el9".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            },
            PackageEntry {
                name: "mod_ssl".into(),
                arch: "x86_64".into(),
                version: "2.4.57".into(),
                release: "11.el9".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            },
        ],
        // apr/apr-util are declared group members that ship with the base
        // image rather than being added by this host: `in_base_image` is
        // computed from this membership, not from absence in
        // `packages_added` (§ 3.7 position 5).
        baseline_suppressed: Some(vec!["apr.x86_64".into(), "apr-util.x86_64".into()]),
        installed_groups: Some(vec![InstalledGroup {
            name: "Web Server".into(),
            members: vec![
                "httpd".into(),
                "mod_ssl".into(),
                "apr".into(),
                "apr-util".into(),
            ],
            optional_installed: vec![],
        }]),
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(session)),
        sections_cache: OnceLock::new(),
    });
    let app = app(state);
    let (status, json) = get_json(&app, "/api/view").await;
    assert_eq!(status, StatusCode::OK);

    // Verify contract expectations before snapshotting
    let groups = json["package_groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "expected exactly one package group");

    let group = &groups[0];
    assert_eq!(
        group["added_count"].as_u64().unwrap(),
        2,
        "expected added_count=2 (httpd, mod_ssl)"
    );
    assert_eq!(
        group["member_count"].as_u64().unwrap(),
        4,
        "expected member_count=4"
    );

    let members = group["members"].as_array().unwrap();
    assert_eq!(members.len(), 4);

    // First two should be new (httpd, mod_ssl), sorted alphabetically
    assert_eq!(members[0]["name"].as_str().unwrap(), "httpd");
    assert_eq!(
        members[0]["in_base_image"].as_bool().unwrap(),
        false,
        "httpd should NOT be in_base_image"
    );
    assert_eq!(members[1]["name"].as_str().unwrap(), "mod_ssl");
    assert_eq!(
        members[1]["in_base_image"].as_bool().unwrap(),
        false,
        "mod_ssl should NOT be in_base_image"
    );

    // Last two should be from base (apr, apr-util), sorted alphabetically
    assert_eq!(members[2]["name"].as_str().unwrap(), "apr");
    assert_eq!(
        members[2]["in_base_image"].as_bool().unwrap(),
        true,
        "apr should be in_base_image"
    );
    assert_eq!(members[3]["name"].as_str().unwrap(), "apr-util");
    assert_eq!(
        members[3]["in_base_image"].as_bool().unwrap(),
        true,
        "apr-util should be in_base_image"
    );

    insta::assert_json_snapshot!("grouped_view", json);
}

// -- The two assertions Tasks 19 and 21 deferred to this one ---------------
//
// Both are function-level: `build_web_sections` is the unit that gained
// the fields. `RefineSession::reference()` still projects `self.original`
// and `AppState.sections_cache` is still a never-cleared `OnceLock`, so
// the same transition over the live endpoint is Task 24's, which deletes
// both caches (§ 3.9).

/// The Kernel & Boot section of a snapshot's reference projection.
fn kernel_boot_customizations(snap: &InspectionSnapshot) -> Vec<ContextItem> {
    let sections = build_web_sections(&project_reference(snap));
    let kb = sections
        .iter()
        .find(|s| s.id == "kernel_boot")
        .expect("kernel_boot section");
    kb.subsections
        .iter()
        .find(|s| s.id == "customizations")
        .map(|s| s.items.clone())
        .unwrap_or_default()
}

/// The Storage section's items for a snapshot's reference projection.
fn storage_items(snap: &InspectionSnapshot) -> Vec<ContextItem> {
    build_web_sections(&project_reference(snap))
        .into_iter()
        .find(|s| s.id == "storage")
        .expect("storage section")
        .items
}

/// § 8.10, deferred from Task 21. On the degraded-but-shadowing fixture
/// each shadowed key decides on its own row, and the file whose keys
/// they are decides nowhere: Task 21 made that entry `Inventory`, and a
/// row that cannot be toggled must not offer a toggle.
///
/// `/etc/modprobe.d/driftify.conf` is the same-path positive control.
/// Without it the `decision.is_none()` assertion below would hold just
/// as well if config-backed rows never decided at all, and the only
/// positive cases in this test reach `ContextDecision` by the sysctl
/// route rather than this one.
#[test]
fn shadowed_sysctl_keys_decide_and_the_shadowing_file_does_not() {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry {
                path: "/etc/sysctl.d/99-driftify.conf".into(),
                kind: ConfigFileKind::Unowned,
                category: ConfigCategory::Sysctl,
                content: "vm.swappiness = 10\nnet.ipv4.ip_forward = 1".into(),
                disposition: FindingKind::inventory(),
                shadowed_by: Some(ConfigShadow::Sysctl),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/modprobe.d/driftify.conf".into(),
                kind: ConfigFileKind::Unowned,
                category: ConfigCategory::KernelModule,
                content: "blacklist nouveau".into(),
                disposition: FindingKind::excluded(),
                locked: true,
                ..Default::default()
            },
        ],
    });
    snap.kernel_boot = Some(KernelBootSection {
        sysctl_overrides: vec![
            SysctlOverride {
                key: "vm.swappiness".into(),
                runtime: "10".into(),
                default: "60".into(),
                source: "/etc/sysctl.d/99-driftify.conf".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            SysctlOverride {
                key: "net.ipv4.ip_forward".into(),
                runtime: "1".into(),
                default: "0".into(),
                source: "/etc/sysctl.d/99-driftify.conf".into(),
                disposition: FindingKind::excluded(),
                locked: true,
                ..Default::default()
            },
        ],
        ..Default::default()
    });

    let items = kernel_boot_customizations(&snap);

    let swappiness = items
        .iter()
        .find(|i| i.id == "vm.swappiness")
        .expect("a row for each shadowed key");
    let decision = swappiness.decision.as_ref().expect("vm.swappiness decides");
    assert_eq!(
        serde_json::to_value(&decision.item_id).unwrap(),
        serde_json::json!({"kind": "Sysctl", "key": {"key": "vm.swappiness"}})
    );
    assert!(decision.include);
    assert!(!decision.locked);

    let forward = items
        .iter()
        .find(|i| i.id == "net.ipv4.ip_forward")
        .expect("a row for each shadowed key");
    let decision = forward
        .decision
        .as_ref()
        .expect("net.ipv4.ip_forward decides");
    assert_eq!(
        serde_json::to_value(&decision.item_id).unwrap(),
        serde_json::json!({"kind": "Sysctl", "key": {"key": "net.ipv4.ip_forward"}})
    );
    assert!(!decision.include);
    assert!(decision.locked);

    let shadowing = items
        .iter()
        .find(|i| i.id == "/etc/sysctl.d/99-driftify.conf")
        .expect("the shadowing file keeps its pointer row");
    assert!(
        shadowing.decision.is_none(),
        "the shadowed entry is Inventory and decides nowhere, got {:?}",
        shadowing.decision
    );
    assert_eq!(
        shadowing.subtitle.as_deref(),
        Some("merged into 99-inspectah-migrated.conf")
    );

    // The positive control on the same path: an actionable, locked,
    // excluded config entry decides on its pointer row, through
    // `ItemId::Config` and carrying the entry's real lock state.
    let modprobe = items
        .iter()
        .find(|i| i.id == "/etc/modprobe.d/driftify.conf")
        .expect("an unshadowed config entry keeps its pointer row");
    let decision = modprobe
        .decision
        .as_ref()
        .expect("a config-backed row decides under ItemId::Config");
    assert_eq!(
        serde_json::to_value(&decision.item_id).unwrap(),
        serde_json::json!({"kind": "Config", "key": {"path": "/etc/modprobe.d/driftify.conf"}})
    );
    assert!(!decision.include);
    assert!(
        decision.locked,
        "a locked Config target is refused as a silent no-op, so the row \
         must not report an enabled control"
    );
}

/// § 3.4 View, deferred from Task 19. The Storage rows follow the
/// export, not the snapshot verdict: a directory whose `/etc` fragment
/// the export drops becomes an advisory row naming the lost creator,
/// and carrying the fragment again returns it to the `Carried` summary.
#[test]
fn excluding_the_tmpfiles_fragment_turns_a_carried_var_dir_into_an_advisory_row() {
    let var_dir_snapshot = |fragment_included: bool| {
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/tmpfiles.d/appone.conf".into(),
                kind: ConfigFileKind::Unowned,
                category: ConfigCategory::Tmpfiles,
                content: "d /var/lib/appone/cache 0750 appone appone -".into(),
                disposition: FindingKind::from_bool(fragment_included),
                ..Default::default()
            }],
        });
        snap.storage = Some(StorageSection {
            var_directories: vec![VarDirectory {
                path: "/var/lib/appone/cache".into(),
                backing: Some(VarDirBacking::Tmpfiles {
                    conf: "/etc/tmpfiles.d/appone.conf".into(),
                    line: "d /var/lib/appone/cache 0750 appone appone -".into(),
                }),
                mode: Some("0750".into()),
                owner_name: Some("appone".into()),
                group_name: Some("appone".into()),
                ..Default::default()
            }],
            ..Default::default()
        });
        snap
    };

    let excluded = storage_items(&var_dir_snapshot(false));
    let advisory_row = excluded
        .iter()
        .find(|i| i.id == "/var/lib/appone/cache")
        .expect("the dropped fragment leaves the directory unbacked");
    let advisory = advisory_row
        .advisory
        .as_ref()
        .expect("an unbacked directory carries an advisory");
    assert_eq!(advisory.advisory_type, AdvisoryType::UnbackedVarDir);
    assert!(
        advisory.rationale.contains("appone.conf"),
        "the rationale names the lost creator, got {:?}",
        advisory.rationale
    );
    assert_eq!(
        summary_state(&excluded),
        "1 unbacked",
        "the summary counts the unbacked directory"
    );
    // The summary's creator column and its outcome column must not
    // disagree: the line under `1 unbacked` names the host's tmpfiles.d
    // fragment and then says the export does not carry it.
    assert_eq!(
        summary_detail(&excluded),
        "/var/lib/appone/cache  0750 appone:appone  \
         tmpfiles.d (/etc/tmpfiles.d/appone.conf)  provisioned by this export"
    );

    // Carrying the fragment again drops the advisory. The directory
    // keeps a row only because 0750 appone:appone departs from the
    // default the image would otherwise reproduce (§ 6.6), and that row
    // is inventory: nothing to decide and nothing to warn about.
    let included = storage_items(&var_dir_snapshot(true));
    let carried_row = included
        .iter()
        .find(|i| i.id == "/var/lib/appone/cache")
        .expect("a non-default carried directory keeps an inventory row");
    assert!(
        carried_row.advisory.is_none(),
        "carrying the fragment again drops the advisory, got {:?}",
        carried_row.advisory
    );
    assert!(
        carried_row.decision.is_none(),
        "an inventory row decides nothing"
    );
    assert_eq!(
        summary_state(&included),
        "all backed by tmpfiles.d, unit directives, or package ownership"
    );
    assert_eq!(
        summary_detail(&included),
        "/var/lib/appone/cache  0750 appone:appone  \
         tmpfiles.d (/etc/tmpfiles.d/appone.conf)  carried",
        "the carried directory collapses into the summary's list"
    );
}

/// § 3.4 View. An `Unresolved` directory is the same row kind with the
/// same `advisory_type`, its rationale carries the reason the export
/// could not decide, and it is counted apart from the directories the
/// export knows are unbacked: calling it unbacked would assert of it
/// exactly what its own rationale says is unknown.
#[test]
fn an_unresolved_var_dir_is_counted_as_unconfirmed_not_unbacked() {
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![VarDirectory {
            path: "/var/lib/vendor".into(),
            backing: Some(VarDirBacking::Tmpfiles {
                conf: "/usr/lib/tmpfiles.d/vendor.conf".into(),
                line: "d /var/lib/vendor 0755 root root -".into(),
            }),
            mode: Some("0755".into()),
            owner_name: Some("root".into()),
            group_name: Some("root".into()),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["filesystem".into()]),
        ..Default::default()
    });

    let items = storage_items(&snap);
    let row = items
        .iter()
        .find(|i| i.id == "/var/lib/vendor")
        .expect("an unresolved directory is a row of its own");
    let advisory = row.advisory.as_ref().expect("unresolved carries advisory");
    assert_eq!(
        serde_json::to_value(&advisory.advisory_type).unwrap(),
        serde_json::json!("unbacked_var_dir"),
        "§ 8.12 pins the wire string, not the Rust variant"
    );
    assert_eq!(
        advisory.rationale,
        "created on the host by /usr/lib/tmpfiles.d/vendor.conf; this export cannot tell \
         whether the base image carries it (no package owns that file)"
    );
    // The badge reads "Advisory: Unbacked /var directory", which claims
    // more than this outcome knows, so the row's own state text says
    // what is actually established.
    assert_eq!(row.subtitle.as_deref(), Some("backing unconfirmed"));
    assert_eq!(summary_state(&items), "1 unconfirmed");
    assert_eq!(
        summary_detail(&items),
        "/var/lib/vendor  0755 root:root  \
         tmpfiles.d (/usr/lib/tmpfiles.d/vendor.conf)  backing unconfirmed"
    );
}

/// A `Provision` directory with no recorded ownership gets `RUN mkdir
/// -p` rather than a tmpfiles.d line, which lands it in the image
/// instead of re-asserting it on every boot. The bare `-` the summary
/// used to print was tmpfiles.d's own "leave the default" token, which
/// means the opposite of "the scan could not read it".
#[test]
fn a_var_dir_with_no_recorded_ownership_says_so_rather_than_printing_a_dash() {
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![VarDirectory {
            path: "/var/lib/pgsql".into(),
            backing: Some(VarDirBacking::Unbacked),
            ..Default::default()
        }],
        ..Default::default()
    });

    let items = storage_items(&snap);
    let row = items
        .iter()
        .find(|i| i.id == "/var/lib/pgsql")
        .expect("an unbacked directory is a row of its own");
    assert_eq!(
        row.advisory.as_ref().map(|a| a.rationale.as_str()),
        Some(
            "nothing on the host declares this directory: no tmpfiles.d entry, \
             unit directive, or package owns it. This export provisions it."
        )
    );
    assert_eq!(
        row.subtitle.as_deref(),
        Some("ownership not recorded: created in the image, not by tmpfiles.d")
    );
    assert_eq!(
        summary_detail(&items),
        "/var/lib/pgsql  ownership not recorded  nothing declares it  provisioned by this export"
    );
    assert_eq!(summary_state(&items), "1 unbacked");
}

/// Both counts at once, so neither branch of the summary's state text
/// can be the other's.
#[test]
fn the_summary_counts_provisioned_and_unconfirmed_apart() {
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![
            VarDirectory {
                path: "/var/lib/plain".into(),
                backing: Some(VarDirBacking::Unbacked),
                mode: Some("0755".into()),
                owner_name: Some("root".into()),
                group_name: Some("root".into()),
                ..Default::default()
            },
            VarDirectory {
                path: "/var/lib/vendor".into(),
                backing: Some(VarDirBacking::Tmpfiles {
                    conf: "/usr/lib/tmpfiles.d/vendor.conf".into(),
                    line: "d /var/lib/vendor 0755 root root -".into(),
                }),
                mode: Some("0755".into()),
                owner_name: Some("root".into()),
                group_name: Some("root".into()),
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["filesystem".into()]),
        ..Default::default()
    });

    assert_eq!(
        summary_state(&storage_items(&snap)),
        "1 unbacked, 1 unconfirmed"
    );
}

/// The state text of the collapsed `/var` summary row (§ 6.6).
fn summary_state(items: &[ContextItem]) -> String {
    items
        .iter()
        .find(|i| i.id == VAR_SUMMARY_ID)
        .and_then(|i| i.subtitle.clone())
        .expect("a /var summary row")
}

/// The collapsed summary's directory list (§ 6.6).
fn summary_detail(items: &[ContextItem]) -> String {
    items
        .iter()
        .find(|i| i.id == VAR_SUMMARY_ID)
        .and_then(|i| i.detail.clone())
        .expect("a /var summary row with a list")
}

// -- `empty_reason` on an empty section (§ 3.8) ---------------------------
//
// The contract fixture's eight sections all have items or subsections,
// so neither branch is reachable from the endpoint snapshots.

/// The `empty_reason` of every section an empty snapshot produces.
fn empty_reasons(snap: &InspectionSnapshot) -> Vec<(String, Option<String>)> {
    build_web_sections(&project_reference(snap))
        .into_iter()
        .map(|s| (s.id, s.empty_reason))
        .collect()
}

/// A section the inspector never collected says so, rather than
/// claiming the host had nothing to report.
#[test]
fn an_uncollected_section_is_data_unavailable() {
    let reasons = empty_reasons(&InspectionSnapshot::new());
    assert_eq!(
        reasons,
        vec![
            ("services".to_string(), Some("data_unavailable".to_string())),
            (
                "containers".to_string(),
                Some("data_unavailable".to_string())
            ),
            ("network".to_string(), Some("data_unavailable".to_string())),
            ("storage".to_string(), Some("data_unavailable".to_string())),
            (
                "scheduled_tasks".to_string(),
                Some("data_unavailable".to_string())
            ),
            (
                "non_rpm_software".to_string(),
                Some("data_unavailable".to_string())
            ),
            (
                "kernel_boot".to_string(),
                Some("data_unavailable".to_string())
            ),
            ("selinux".to_string(), Some("data_unavailable".to_string())),
        ]
    );
}

/// A section the inspector collected and found nothing in is a
/// different answer, and the two must not read alike.
#[test]
fn a_collected_but_empty_section_is_zero_drift() {
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection::default());
    snap.containers = Some(ContainerSection::default());

    let reasons: HashMap<String, Option<String>> = empty_reasons(&snap).into_iter().collect();
    assert_eq!(reasons["storage"].as_deref(), Some("zero_drift"));
    assert_eq!(reasons["containers"].as_deref(), Some("zero_drift"));
    // Untouched sections keep the other answer, so the test cannot pass
    // by the adapter stamping one token on everything.
    assert_eq!(reasons["network"].as_deref(), Some("data_unavailable"));
    assert_eq!(reasons["selinux"].as_deref(), Some("data_unavailable"));
}

/// A section with rows carries no `empty_reason` at all.
#[test]
fn a_populated_section_has_no_empty_reason() {
    let sections = build_web_sections(&project_reference(&contract_snapshot()));
    let empty: Vec<&str> = sections
        .iter()
        .filter(|s| s.empty_reason.is_some())
        .map(|s| s.id.as_str())
        .collect();
    assert!(
        empty.is_empty(),
        "the contract fixture populates all eight sections, got {empty:?}"
    );
}
